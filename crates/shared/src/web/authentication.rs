//! Deny by default: every request needs a valid access token, except the few public paths.
//!
//! Java counterpart: the `apiSecurity` filter chain in `identity/internal/web/SecurityConfig.java`, with Spring
//! Security's `BearerTokenAuthenticationFilter` (and its `DefaultBearerTokenResolver`), the authorization rules
//! and `ProblemSecurityHandler` as the entry point.
//!
//! It runs before routing, as Spring Security's filters run before the `DispatcherServlet`: without a token any
//! path that is not public is 401, also one that does not exist, and only an authenticated caller learns that
//! a path is unknown (404) or takes another method (405).
//!
//! What Spring does, and so this does:
//! - an `Authorization` header that starts with `bearer` (any case) must be `Bearer <token>` with the token in
//!   RFC 6750's alphabet, or the request is 401, public path or not;
//! - a token is checked wherever it is sent, so a bad token is 401 on a public path too;
//! - any other `Authorization` scheme counts as no token;
//! - the 401 is `UNAUTHENTICATED` with `WWW-Authenticate: Bearer`, the same body for every reason.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::current_user::CurrentUser;
use super::problem::Problem;
use super::routing::percent_decode;
use crate::id::UserId;

/// What checking a bearer token found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenCheck {
    /// The token is valid and names this user.
    Valid(UserId),
    /// Not a token for this API, for whatever reason: 401.
    Invalid,
    /// The check itself failed (the database could not be asked): 500, as Spring lets such an exception
    /// through its filter chain.
    Failed,
}

/// A token check in progress.
pub type Checking<'a> = Pin<Box<dyn Future<Output = TokenCheck> + Send + 'a>>;

/// Checks an access token. Implemented by the identity module: for its own tokens, or, with `keycloak`, for the
/// identity provider's, which may need the provider's keys and the database (just-in-time provisioning).
pub trait TokenVerifier: fmt::Debug + Send + Sync {
    /// The user the token names, if it is valid.
    fn verify<'a>(&'a self, token: &'a str) -> Checking<'a>;
}

/// The authentication layer's state: who checks tokens, and which paths are public.
#[derive(Debug, Clone)]
pub struct Authentication {
    /// `None`: every token is refused. Only tests build it so; the app always has a verifier.
    verifier: Option<Arc<dyn TokenVerifier>>,
    /// The `local` profile: `GET /docs` and `GET /swagger-ui/**` are public.
    local_docs: bool,
}

impl Authentication {
    #[must_use]
    pub fn new(verifier: Option<Arc<dyn TokenVerifier>>, local_docs: bool) -> Self {
        Self { verifier, local_docs }
    }

    /// Java's `permitAll` rules: `/api/v1/auth/**` and `/.well-known/jwks.json` for any method; in `local` also
    /// `GET /docs` and `GET /swagger-ui/**` (not `HEAD`: Spring's method matcher takes only `GET`).
    ///
    /// Paths are compared segment by segment after percent-decoding, as Spring's `PathPattern` does.
    fn is_public(&self, method: &Method, path: &str) -> bool {
        let segments: Vec<String> = path
            .strip_prefix('/')
            .unwrap_or(path)
            .split('/')
            .map(percent_decode)
            .collect();
        let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
        match segments.as_slice() {
            ["api", "v1", "auth", ..] | [".well-known", "jwks.json"] => true,
            ["docs"] | ["swagger-ui", ..] => self.local_docs && method == Method::GET,
            _ => false,
        }
    }
}

/// Why a request has no usable bearer token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bearer<'a> {
    /// No `Authorization` header, or one with another scheme.
    Absent,
    /// `Authorization: Bearer …` that does not match RFC 6750: "Bearer token is malformed".
    Malformed,
    Token(&'a str),
}

/// `DefaultBearerTokenResolver.resolveFromAuthorizationHeader`: the first `Authorization` header, as the
/// servlet API's `getHeader` returns it.
fn bearer(headers: &HeaderMap) -> Bearer<'_> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Bearer::Absent;
    };
    let bytes = value.as_bytes();
    let starts_with_bearer = bytes
        .get(..6)
        .is_some_and(|start| start.eq_ignore_ascii_case(b"bearer"));
    if !starts_with_bearer {
        return Bearer::Absent;
    }
    // `^Bearer (?<token>[a-zA-Z0-9-._~+/]+=*)$`, case-insensitive.
    let Some(token) = bytes.get(7..).filter(|_| bytes.get(6) == Some(&b' ')) else {
        return Bearer::Malformed;
    };
    let body = token
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || b"-._~+/".contains(b))
        .count();
    let well_formed = body > 0
        && token
            .get(body..)
            .is_some_and(|padding| padding.iter().all(|&b| b == b'='));
    match std::str::from_utf8(token) {
        Ok(text) if well_formed => Bearer::Token(text),
        _ => Bearer::Malformed,
    }
}

/// Middleware: the caller as [`CurrentUser`] in the request's extensions, `userId` and `teamId` on the request's
/// log span, or 401.
///
/// It must run inside the problem rendering, so that the 401 gets `instance` and `traceId`, and outside the
/// routes.
pub async fn authenticate(State(authentication): State<Authentication>, mut request: Request, next: Next) -> Response {
    let user = match bearer(request.headers()) {
        Bearer::Malformed => return Problem::unauthenticated().into_response(),
        Bearer::Absent => None,
        Bearer::Token(token) => {
            let check = match &authentication.verifier {
                Some(verifier) => verifier.verify(token).await,
                None => TokenCheck::Invalid,
            };
            match check {
                TokenCheck::Valid(user) => Some(user),
                TokenCheck::Invalid => return Problem::unauthenticated().into_response(),
                TokenCheck::Failed => return Problem::internal_error().into_response(),
            }
        }
    };
    // Java's `MdcTeamFilter`, which sits after the token check but before the decision about a missing token.
    if let Some(team) = team_in_path(request.uri().path()) {
        tracing::Span::current().record("teamId", team);
    }
    match user {
        Some(user) => {
            // Java's `MdcUserFilter`: the id, never the email, on every log line of the request.
            tracing::Span::current().record("userId", tracing::field::display(user));
            request.extensions_mut().insert(CurrentUser::new(user));
        }
        None if authentication.is_public(request.method(), request.uri().path()) => {}
        None => return Problem::unauthenticated().into_response(),
    }
    next.run(request).await
}

/// The team id of `/api/v1/teams/{teamId}`, `…/{teamId}/…` and `…/{teamId}:…`, as the raw path has it. Java:
/// `MdcTeamFilter.TEAM_PATH`, `^/api/v1/teams/(<uuid>)(?:[/:].*)?$` on the undecoded request URI, with the
/// UUID's hex digits in either case.
fn team_in_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/api/v1/teams/")?;
    let (team, after) = rest.split_at_checked(36)?;
    let canonical = team.char_indices().all(|(index, c)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            c == '-'
        } else {
            c.is_ascii_hexdigit()
        }
    });
    let ends = after.is_empty() || after.starts_with('/') || after.starts_with(':');
    (canonical && ends).then_some(team)
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderValue, StatusCode};
    use axum::routing::any;
    use tower::ServiceExt;

    use super::*;

    /// Accepts exactly one token, `good`, for one user.
    #[derive(Debug)]
    struct OneToken(UserId);

    impl TokenVerifier for OneToken {
        fn verify<'a>(&'a self, token: &'a str) -> Checking<'a> {
            let check = match token {
                "good" => TokenCheck::Valid(self.0),
                "unanswerable" => TokenCheck::Failed,
                _ => TokenCheck::Invalid,
            };
            Box::pin(async move { check })
        }
    }

    fn router(user: UserId, local_docs: bool) -> Router {
        let authentication = Authentication::new(Some(Arc::new(OneToken(user))), local_docs);
        Router::new()
            .fallback(any(|request: Request| async move {
                request
                    .extensions()
                    .get::<CurrentUser>()
                    .map_or_else(|| "anonymous".to_owned(), |user| user.id().to_string())
            }))
            .layer(axum::middleware::from_fn_with_state(authentication, authenticate))
    }

    async fn call(router: &Router, method: Method, path: &str, authorization: Option<&[u8]>) -> (StatusCode, String) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(value) = authorization {
            request = request.header(header::AUTHORIZATION, HeaderValue::from_bytes(value).unwrap());
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let challenge = response.headers().get(header::WWW_AUTHENTICATE).cloned();
        let body = axum::body::to_bytes(response.into_body(), 1 << 16).await.unwrap();
        if status == StatusCode::UNAUTHORIZED {
            assert_eq!(challenge.as_ref().map(|v| v.to_str().unwrap()), Some("Bearer"));
        }
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_valid_token_names_the_caller_on_any_path() {
        let user = UserId::new_v7();
        let router = router(user, false);

        for path in [
            "/api/v1/me",
            "/api/v1/does-not-exist",
            "/whatever",
            "/api/v1/auth/login",
        ] {
            assert_eq!(
                call(&router, Method::GET, path, Some(b"Bearer good")).await,
                (StatusCode::OK, user.to_string()),
                "{path}"
            );
        }
    }

    #[tokio::test]
    async fn without_a_token_only_public_paths_pass() {
        let router = router(UserId::new_v7(), false);

        for (method, path) in [
            (Method::POST, "/api/v1/auth/login"),
            (Method::POST, "/api/v1/auth/refresh"),
            (Method::GET, "/api/v1/auth"),
            (Method::GET, "/api/v1/auth/"),
            (Method::DELETE, "/api/v1/auth/anything/below"),
            (Method::GET, "/.well-known/jwks.json"),
            (Method::POST, "/.well-known/jwks.json"),
            (Method::GET, "/api/v1/%61uth/login"),
        ] {
            assert_eq!(call(&router, method, path, None).await.0, StatusCode::OK, "{path}");
        }
        for (method, path) in [
            (Method::GET, "/api/v1/me"),
            (Method::GET, "/api/v1/authx"),
            (Method::GET, "/api/v1/teams/1/expenses"),
            (Method::GET, "/does-not-exist"),
            (Method::GET, "/actuator/health"),
            (Method::GET, "/.well-known/jwks.json/"),
            (Method::GET, "/.well-known/other.json"),
            (Method::GET, "/docs"),
            (Method::GET, "/swagger-ui/index.html"),
            (Method::GET, "/api/v2/auth/login"),
        ] {
            assert_eq!(
                call(&router, method, path, None).await.0,
                StatusCode::UNAUTHORIZED,
                "{path}"
            );
        }
    }

    /// Java: `SwaggerUiProfileTest`: in `local`, only reading the docs is open.
    #[tokio::test]
    async fn local_opens_only_get_on_the_docs() {
        let router = router(UserId::new_v7(), true);

        for path in ["/docs", "/swagger-ui/index.html", "/swagger-ui/openapi.yaml"] {
            assert_eq!(call(&router, Method::GET, path, None).await.0, StatusCode::OK, "{path}");
            assert_eq!(
                call(&router, Method::POST, path, None).await.0,
                StatusCode::UNAUTHORIZED,
                "{path}"
            );
            assert_eq!(
                call(&router, Method::HEAD, path, None).await.0,
                StatusCode::UNAUTHORIZED,
                "{path}"
            );
        }
        assert_eq!(
            call(&router, Method::GET, "/docsx", None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn a_bad_token_is_refused_even_on_a_public_path() {
        let router = router(UserId::new_v7(), true);

        for path in ["/api/v1/me", "/api/v1/auth/login", "/.well-known/jwks.json", "/docs"] {
            assert_eq!(
                call(&router, Method::GET, path, Some(b"Bearer bad")).await.0,
                StatusCode::UNAUTHORIZED,
                "{path}"
            );
        }
    }

    /// The verifier could not decide (with `keycloak`: the database is down while provisioning): 500, not 401,
    /// so that a client does not throw away a token that may well be valid.
    #[tokio::test]
    async fn a_check_that_fails_is_a_server_error() {
        let router = router(UserId::new_v7(), false);

        let (status, body) = call(&router, Method::GET, "/api/v1/me", Some(b"Bearer unanswerable")).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("INTERNAL_ERROR"), "{body}");
    }

    /// `DefaultBearerTokenResolver`: what starts with `bearer` must be well formed, anything else is no token.
    #[tokio::test]
    async fn the_bearer_header_is_read_as_spring_reads_it() {
        let router = router(UserId::new_v7(), false);
        let public = "/api/v1/auth/login";

        for malformed in [
            &b"Bearer"[..],
            b"Bearer ",
            b"Bearer  good",
            b"Bearergood",
            b"Bearer good bad",
            b"Bearer g\xf6od",
            b"Bearer =",
            b"Bearer good=x",
            b"bearer\tgood",
        ] {
            assert_eq!(
                call(&router, Method::GET, public, Some(malformed)).await.0,
                StatusCode::UNAUTHORIZED,
                "{}",
                String::from_utf8_lossy(malformed)
            );
        }
        // Another scheme: no token, so a public path passes and a protected one is 401.
        for other in [&b"Basic dXNlcjpwYXNz"[..], b"Token good", b""] {
            assert_eq!(call(&router, Method::GET, public, Some(other)).await.0, StatusCode::OK);
            assert_eq!(
                call(&router, Method::GET, "/api/v1/me", Some(other)).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
    }

    #[test]
    fn the_scheme_is_case_insensitive_and_padding_is_allowed() {
        let headers = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
            headers
        };

        assert_eq!(bearer(&headers("bEaReR good")), Bearer::Token("good"));
        assert_eq!(
            bearer(&headers("Bearer a-b.c_d~e+f/g==")),
            Bearer::Token("a-b.c_d~e+f/g==")
        );
        assert_eq!(bearer(&HeaderMap::new()), Bearer::Absent);
    }

    #[tokio::test]
    async fn without_a_verifier_every_token_is_refused() {
        let router = Router::new()
            .fallback(any(|| async { "reached" }))
            .layer(axum::middleware::from_fn_with_state(
                Authentication::new(None, false),
                authenticate,
            ));

        assert_eq!(
            call(&router, Method::GET, "/api/v1/auth/login", Some(b"Bearer good"))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&router, Method::GET, "/api/v1/auth/login", None).await.0,
            StatusCode::OK
        );
    }

    /// Java: `MdcTeamFilterTest`.
    #[test]
    fn team_scoped_paths_name_their_team_and_no_other_path_does() {
        let team = "11111111-2222-3333-4444-555555555555";
        assert_eq!(team_in_path(&format!("/api/v1/teams/{team}/expenses")), Some(team));
        assert_eq!(team_in_path(&format!("/api/v1/teams/{team}:leave")), Some(team));
        assert_eq!(team_in_path(&format!("/api/v1/teams/{team}")), Some(team));
        assert_eq!(
            team_in_path("/api/v1/teams/ABCDEF01-2222-3333-4444-555555555555/x"),
            Some("ABCDEF01-2222-3333-4444-555555555555")
        );

        assert_eq!(team_in_path("/api/v1/me"), None);
        assert_eq!(team_in_path("/api/v1/teams"), None);
        assert_eq!(team_in_path("/api/v1/teams/not-a-uuid/expenses"), None);
        assert_eq!(team_in_path("/api/v1/teams/{x}%0Afake=1/expenses"), None);
        assert_eq!(team_in_path(&format!("/api/v1/teams/{team}x")), None);
        assert_eq!(team_in_path("/api/v1/teams/11111111-2222-3333-4444-55555555555é"), None);
    }
}
