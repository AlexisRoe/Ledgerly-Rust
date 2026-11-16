//! What browsers are told: the security headers on every response, and which other sites may call the API
//! (CORS).
//!
//! Java counterpart: the `headers(…)` and `cors(…)` parts of the `apiSecurity` chain in
//! `identity/internal/web/SecurityConfig.java`, which is Spring Security's `HeaderWriterFilter` with its default
//! writers plus Ledgerly's settings, and Spring's `CorsFilter` with `DefaultCorsProcessor`.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

use super::forwarded;
use crate::config::ForwardHeaders;

/// `SecurityConfig.API_CSP`: a JSON API loads nothing.
const API_CSP: &str = "default-src 'none'; frame-ancestors 'none'";
/// `SecurityConfig.SWAGGER_CSP`, for `/swagger-ui/…` in the `local` profile.
const SWAGGER_CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
                           img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'";

/// Middleware: the API's security headers, written after the handler as `HeaderWriterFilter` writes them when
/// the response is committed: Spring Security's defaults ([`management_headers`]), plus HSTS on every request,
/// `Referrer-Policy: no-referrer` and the CSP. A header the handler already set stays, except the CSP, which
/// Ledgerly's writer always sets.
///
/// `local_docs` is the `local` profile, in which Swagger UI gets a CSP that lets it run.
pub async fn secure_headers(State(local_docs): State<bool>, request: Request, next: Next) -> Response {
    let swagger = local_docs && request.uri().path().starts_with("/swagger-ui/");
    let mut response = next.run(request).await;
    spring_security_defaults(&mut response);
    let headers = response.headers_mut();
    // Sent on every request: a TLS-terminating proxy makes every request look like plain http here.
    set_if_absent(
        headers,
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=31536000 ; includeSubDomains",
    );
    set_if_absent(headers, header::REFERRER_POLICY, "no-referrer");
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(if swagger { SWAGGER_CSP } else { API_CSP }),
    );
    response
}

/// Middleware: Spring Security's default headers, which the management port's `actuatorSecurity` chain
/// writes. Its HSTS writer is left at the default, which writes only on HTTPS, so never here.
pub async fn management_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    spring_security_defaults(&mut response);
    response
}

/// The writers `HeadersConfigurer` enables by default, minus HSTS. The cache headers are left out entirely if the
/// handler set any of `Cache-Control`, `Expires` or `Pragma` (login and the receipt link set
/// `Cache-Control: no-store`), and on a 304, as Spring's `CacheControlHeadersWriter` does.
fn spring_security_defaults(response: &mut Response) {
    let not_modified = response.status() == StatusCode::NOT_MODIFIED;
    let headers = response.headers_mut();
    let sets_caching = [header::CACHE_CONTROL, header::EXPIRES, header::PRAGMA]
        .iter()
        .any(|name| headers.contains_key(name));
    set_if_absent(headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set_if_absent(headers, header::X_XSS_PROTECTION, "0");
    if !sets_caching && !not_modified {
        set_if_absent(
            headers,
            header::CACHE_CONTROL,
            "no-cache, no-store, max-age=0, must-revalidate",
        );
        set_if_absent(headers, header::PRAGMA, "no-cache");
        set_if_absent(headers, header::EXPIRES, "0");
    }
    set_if_absent(headers, header::X_FRAME_OPTIONS, "DENY");
}

fn set_if_absent(headers: &mut HeaderMap, name: HeaderName, value: &'static str) {
    if !headers.contains_key(&name) {
        headers.insert(name, HeaderValue::from_static(value));
    }
}

/// `SecurityConfig.corsSource`: the methods and headers a browser may use, and the response headers it may read.
const ALLOWED_METHODS: [&str; 5] = ["GET", "POST", "PATCH", "DELETE", "OPTIONS"];
const ALLOWED_HEADERS: [&str; 5] = [
    "Authorization",
    "Content-Type",
    "If-Match",
    "Idempotency-Key",
    "X-Requested-With",
];
const EXPOSED_HEADERS: &str = "ETag, Location, Retry-After, X-Total-Count, X-Truncated";
const MAX_AGE_SECONDS: &str = "3600";

/// The CORS layer's state: the exact origins of `ledgerly.security.allowed-origins`.
#[derive(Debug, Clone)]
pub struct Cors {
    allowed_origins: Arc<[String]>,
    forward_headers: ForwardHeaders,
}

impl Cors {
    /// `allowed_origins` have been checked at startup ([`crate::config::is_valid_origin`]); `forward_headers`
    /// decides which origin the request itself was sent to.
    #[must_use]
    pub fn new(allowed_origins: &[String], forward_headers: ForwardHeaders) -> Self {
        Self {
            allowed_origins: allowed_origins.into(),
            forward_headers,
        }
    }

    /// `CorsConfiguration.checkOrigin`: the origin as sent, if it equals an allowed one ignoring case and a
    /// trailing slash.
    fn check_origin<'a>(&self, origin: &'a str) -> Option<&'a str> {
        let trimmed = origin.strip_suffix('/').unwrap_or(origin);
        (!origin.trim().is_empty()
            && self
                .allowed_origins
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(trimmed)))
        .then_some(origin)
    }
}

/// What `DefaultCorsProcessor` decides for a request.
enum Decision {
    /// Not a cross-origin request: nothing to add but `Vary`.
    NotCors,
    /// An allowed cross-origin request: the handler answers, with these headers added.
    Actual(HeaderMap),
    /// An allowed preflight, answered here with these headers.
    Preflight(HeaderMap),
    /// 403 `Invalid CORS request`.
    Rejected,
}

/// Middleware: Spring's `CorsFilter`. A cross-origin request from an origin that is not allowed, or with a
/// method or (in a preflight) headers that are not, is 403 with a plain-text body and never reaches
/// authentication. A preflight that is allowed is answered here with 200. Every response gets `Vary` for the
/// three CORS request headers.
pub async fn cors(State(cors): State<Cors>, request: Request, next: Next) -> Response {
    let mut response = match decide(&cors, &request) {
        Decision::NotCors => next.run(request).await,
        Decision::Actual(added) => {
            let mut response = next.run(request).await;
            response.headers_mut().extend(added);
            response
        }
        Decision::Preflight(added) => {
            let mut response = Response::new(Body::empty());
            response.headers_mut().extend(added);
            response
        }
        Decision::Rejected => {
            let mut response = Response::new(Body::from("Invalid CORS request"));
            *response.status_mut() = StatusCode::FORBIDDEN;
            response
        }
    };
    let headers = response.headers_mut();
    for vary in [
        "Origin",
        "Access-Control-Request-Method",
        "Access-Control-Request-Headers",
    ] {
        let present = headers
            .get_all(header::VARY)
            .iter()
            .any(|value| value.as_bytes().eq_ignore_ascii_case(vary.as_bytes()));
        if !present {
            headers.append(header::VARY, HeaderValue::from_static(vary));
        }
    }
    response
}

fn decide(cors: &Cors, request: &Request) -> Decision {
    let headers = request.headers();
    let Some(origin) = headers
        .get(header::ORIGIN)
        .map(|value| String::from_utf8_lossy(value.as_bytes()))
    else {
        return Decision::NotCors;
    };
    let peer = forwarded::peer(request);
    if same_origin(&origin, &forwarded::request_origin(cors.forward_headers, peer, headers)) {
        return Decision::NotCors;
    }
    let requested_method = headers.get(header::ACCESS_CONTROL_REQUEST_METHOD);
    let preflight = request.method() == Method::OPTIONS && requested_method.is_some();
    let Some(allowed_origin) = cors.check_origin(&origin) else {
        return Decision::Rejected;
    };
    // `HttpMethod.valueOf` keeps the case: `get` is not `GET`.
    let method = match requested_method.filter(|_| preflight) {
        Some(value) => String::from_utf8_lossy(value.as_bytes()).into_owned(),
        None => request.method().as_str().to_owned(),
    };
    if !ALLOWED_METHODS.contains(&method.as_str()) {
        return Decision::Rejected;
    }
    let allowed_headers = if preflight {
        match check_headers(headers) {
            Some(allowed) => allowed,
            None => return Decision::Rejected,
        }
    } else {
        Vec::new()
    };

    let mut added = HeaderMap::new();
    let mut set = |name: HeaderName, value: &str| {
        if let Ok(value) = HeaderValue::from_str(value) {
            added.insert(name, value);
        }
    };
    set(header::ACCESS_CONTROL_ALLOW_ORIGIN, allowed_origin);
    if preflight {
        set(header::ACCESS_CONTROL_ALLOW_METHODS, &ALLOWED_METHODS.join(","));
        if !allowed_headers.is_empty() {
            set(header::ACCESS_CONTROL_ALLOW_HEADERS, &allowed_headers.join(", "));
        }
    }
    set(header::ACCESS_CONTROL_EXPOSE_HEADERS, EXPOSED_HEADERS);
    set(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true");
    if preflight {
        set(header::ACCESS_CONTROL_MAX_AGE, MAX_AGE_SECONDS);
        Decision::Preflight(added)
    } else {
        Decision::Actual(added)
    }
}

/// `CorsConfiguration.checkHeaders` for a preflight: the requested headers that are allowed, in the request's
/// spelling. No requested header is fine (an empty list); requested headers of which none is allowed are not
/// (`None`).
fn check_headers(headers: &HeaderMap) -> Option<Vec<String>> {
    let requested: Vec<String> = headers
        .get_all(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    if requested.is_empty() {
        return Some(Vec::new());
    }
    let allowed: Vec<String> = requested
        .into_iter()
        .filter(|name| ALLOWED_HEADERS.iter().any(|allowed| allowed.eq_ignore_ascii_case(name)))
        .collect();
    (!allowed.is_empty()).then_some(allowed)
}

/// `CorsUtils.isCorsRequest`, negated: the `Origin` names the scheme, host and port the request was sent to.
fn same_origin(origin: &str, (scheme, host, port): &(String, String, u16)) -> bool {
    let Some((origin_scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let (origin_host, origin_port) = forwarded::split_host(authority);
    let default_port = match origin_scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    };
    origin_scheme == scheme && origin_host == host && origin_port.or(default_port) == Some(*port)
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::routing::any;
    use tower::ServiceExt;

    use super::*;

    const APP: &str = "https://app.example.com";

    fn router(local_docs: bool) -> Router {
        Router::new()
            .route("/api/v1/me", any(|| async { "me" }))
            .route(
                "/api/v1/auth/login",
                any(|| async { ([(header::CACHE_CONTROL, "no-store")], "token") }),
            )
            .route("/swagger-ui/index.html", any(|| async { "ui" }))
            .layer(axum::middleware::from_fn_with_state(
                Cors::new(&[APP.to_owned()], ForwardHeaders::None),
                cors,
            ))
            .layer(axum::middleware::from_fn_with_state(local_docs, secure_headers))
    }

    async fn call(method: Method, path: &str, headers: &[(&'static str, &'static str)]) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost:8080");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        router(true)
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
        response.headers().get(name).map(|value| value.to_str().unwrap())
    }

    #[tokio::test]
    async fn every_response_carries_spring_securitys_headers() {
        let response = call(Method::GET, "/api/v1/me", &[]).await;

        assert_eq!(header(&response, "x-content-type-options"), Some("nosniff"));
        assert_eq!(header(&response, "x-xss-protection"), Some("0"));
        assert_eq!(
            header(&response, "cache-control"),
            Some("no-cache, no-store, max-age=0, must-revalidate")
        );
        assert_eq!(header(&response, "pragma"), Some("no-cache"));
        assert_eq!(header(&response, "expires"), Some("0"));
        assert_eq!(
            header(&response, "strict-transport-security"),
            Some("max-age=31536000 ; includeSubDomains")
        );
        assert_eq!(header(&response, "x-frame-options"), Some("DENY"));
        assert_eq!(header(&response, "referrer-policy"), Some("no-referrer"));
        assert_eq!(header(&response, "content-security-policy"), Some(API_CSP));
        let vary: Vec<_> = response.headers().get_all("vary").iter().collect();
        assert_eq!(
            vary,
            [
                "Origin",
                "Access-Control-Request-Method",
                "Access-Control-Request-Headers"
            ]
        );
    }

    #[tokio::test]
    async fn a_handler_that_sets_cache_control_keeps_it_alone() {
        let response = call(Method::POST, "/api/v1/auth/login", &[]).await;

        assert_eq!(header(&response, "cache-control"), Some("no-store"));
        assert_eq!(header(&response, "pragma"), None);
        assert_eq!(header(&response, "expires"), None);
    }

    #[tokio::test]
    async fn swagger_ui_gets_its_own_csp_in_local_only() {
        let local = call(Method::GET, "/swagger-ui/index.html", &[]).await;
        let elsewhere = router(false)
            .oneshot(Request::get("/swagger-ui/index.html").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(header(&local, "content-security-policy"), Some(SWAGGER_CSP));
        assert_eq!(header(&elsewhere, "content-security-policy"), Some(API_CSP));
    }

    #[tokio::test]
    async fn an_allowed_preflight_is_answered_here() {
        let response = call(
            Method::OPTIONS,
            "/api/v1/anything",
            &[
                ("origin", APP),
                ("access-control-request-method", "PATCH"),
                ("access-control-request-headers", "authorization, if-match, x-unknown"),
            ],
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, "access-control-allow-origin"), Some(APP));
        assert_eq!(
            header(&response, "access-control-allow-methods"),
            Some("GET,POST,PATCH,DELETE,OPTIONS")
        );
        assert_eq!(
            header(&response, "access-control-allow-headers"),
            Some("authorization, if-match")
        );
        assert_eq!(header(&response, "access-control-allow-credentials"), Some("true"));
        assert_eq!(header(&response, "access-control-max-age"), Some("3600"));
        assert_eq!(
            header(&response, "access-control-expose-headers"),
            Some(EXPOSED_HEADERS)
        );
        assert_eq!(header(&response, "x-frame-options"), Some("DENY"));
    }

    #[tokio::test]
    async fn an_allowed_actual_request_reaches_the_handler_with_the_cors_headers() {
        let response = call(Method::GET, "/api/v1/me", &[("origin", "HTTPS://APP.EXAMPLE.COM/")]).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            header(&response, "access-control-allow-origin"),
            Some("HTTPS://APP.EXAMPLE.COM/")
        );
        assert_eq!(header(&response, "access-control-allow-methods"), None);
        assert_eq!(header(&response, "access-control-max-age"), None);
    }

    #[tokio::test]
    async fn other_origins_methods_and_headers_are_refused() {
        let refused = [
            call(Method::GET, "/api/v1/me", &[("origin", "https://evil.example.com")]).await,
            call(Method::GET, "/api/v1/me", &[("origin", "null")]).await,
            call(Method::GET, "/api/v1/me", &[("origin", "http://app.example.com")]).await,
            call(Method::PUT, "/api/v1/me", &[("origin", APP)]).await,
            call(
                Method::OPTIONS,
                "/api/v1/me",
                &[("origin", APP), ("access-control-request-method", "get")],
            )
            .await,
            call(
                Method::OPTIONS,
                "/api/v1/me",
                &[
                    ("origin", APP),
                    ("access-control-request-method", "GET"),
                    ("access-control-request-headers", "x-unknown"),
                ],
            )
            .await,
        ];

        for response in refused {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(header(&response, "access-control-allow-origin"), None);
            assert_eq!(header(&response, "content-type"), None);
            assert_eq!(header(&response, "x-content-type-options"), Some("nosniff"));
            let body = axum::body::to_bytes(response.into_body(), 100).await.unwrap();
            assert_eq!(&body[..], b"Invalid CORS request");
        }
    }

    #[tokio::test]
    async fn the_same_origin_is_not_a_cors_request() {
        let response = call(Method::POST, "/api/v1/me", &[("origin", "http://localhost:8080")]).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, "access-control-allow-origin"), None);
    }

    #[tokio::test]
    async fn options_without_a_requested_method_is_not_a_preflight() {
        let response = call(Method::OPTIONS, "/api/v1/me", &[("origin", APP)]).await;

        // It reaches the handler like any request from an allowed origin.
        let body = axum::body::to_bytes(response.into_body(), 100).await.unwrap();
        assert_eq!(&body[..], b"me");
    }
}
