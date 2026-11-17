//! Request budgets (API4): per signed-in user on every request, and per client address on the endpoints that
//! need no token.
//!
//! Java counterpart: `shared/security/ApiRateLimitFilter.java`, `AuthRateLimitFilter.java` and
//! `ProblemSecurityHandler.rateLimited`, placed in `SecurityConfig`'s filter chain: the address limit before the
//! token is checked, the user limit right after. The client address is what Tomcat reports, which depends on
//! `server.forward-headers-strategy` ([`ForwardHeaders`]).

use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::current_user::CurrentUser;
use super::forwarded;
use super::problem::{ErrorCode, Problem};
use crate::config::ForwardHeaders;
use crate::ratelimit::{InvalidRateLimit, RateLimit, RateLimiter};

/// Java's `ApiRateLimitFilter.SCOPE`.
const USER_SCOPE: &str = "api";
/// Java's `AuthRateLimitFilter.SCOPE`.
const ADDRESS_SCOPE: &str = "auth-ip";
/// Java's `AuthRateLimitFilter.PREFIX`, compared with the raw request URI.
const TOKEN_FREE_PREFIX: &str = "/api/v1/auth/";

/// The state of both rate-limit layers.
#[derive(Debug, Clone)]
pub struct RequestLimits {
    limiter: RateLimiter,
    per_user: RateLimit,
    per_address: RateLimit,
    forward_headers: ForwardHeaders,
}

impl RequestLimits {
    /// `requests_per_minute` for each signed-in user, `auth_requests_per_minute` for each client address on
    /// `/api/v1/auth/**`, both refilling smoothly, kept in `limiter`.
    ///
    /// # Errors
    ///
    /// [`InvalidRateLimit`] for a zero allowance.
    pub fn new(
        limiter: RateLimiter,
        requests_per_minute: u32,
        auth_requests_per_minute: u32,
        forward_headers: ForwardHeaders,
    ) -> Result<Self, InvalidRateLimit> {
        Ok(Self {
            limiter,
            per_user: RateLimit::per_minute(requests_per_minute)?,
            per_address: RateLimit::per_minute(auth_requests_per_minute)?,
            forward_headers,
        })
    }
}

/// Middleware: one token per request to `/api/v1/auth/**` from the client's address, or 429. Java:
/// `AuthRateLimitFilter`, which runs before the bearer token is checked, so a request with a bad token is
/// counted too.
pub async fn limit_by_client_address(State(limits): State<RequestLimits>, request: Request, next: Next) -> Response {
    if request.uri().path().starts_with(TOKEN_FREE_PREFIX) {
        let address = forwarded::client_address(limits.forward_headers, forwarded::peer(&request), request.headers());
        let decision = limits.limiter.try_consume(ADDRESS_SCOPE, &address, limits.per_address);
        if !decision.allowed {
            return rate_limited(decision.retry_after);
        }
    }
    next.run(request).await
}

/// Middleware: one token per request of a signed-in user, or 429. Requests without a user pass. Java:
/// `ApiRateLimitFilter`, right after the token check and before the authorization rules, so it counts every
/// request with a valid token, also one to a path that does not exist.
///
/// It must run inside [`super::authenticate`], which puts the [`CurrentUser`] in the request.
pub async fn limit_by_user(State(limits): State<RequestLimits>, request: Request, next: Next) -> Response {
    if let Some(user) = request.extensions().get::<CurrentUser>() {
        let decision = limits
            .limiter
            .try_consume(USER_SCOPE, &user.id().to_string(), limits.per_user);
        if !decision.allowed {
            return rate_limited(decision.retry_after);
        }
    }
    next.run(request).await
}

/// Java's `ProblemSecurityHandler.rateLimited`: `Retry-After` in whole seconds, rounded up, at least 1.
fn rate_limited(retry_after: std::time::Duration) -> Response {
    let millis = retry_after.as_millis();
    let seconds = millis.div_ceil(1000).max(1);
    Problem::new(ErrorCode::RateLimited, "Too many requests; slow down")
        .with_header(
            header::RETRY_AFTER,
            HeaderValue::from(u64::try_from(seconds).unwrap_or(u64::MAX)),
        )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::StatusCode;
    use axum::routing::any;
    use chrono::{TimeZone, Utc};
    use tower::ServiceExt;

    use super::*;
    use crate::clock::FixedClock;
    use crate::id::UserId;

    fn limits(per_user: u32, per_address: u32) -> RequestLimits {
        let clock = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 16, 12, 0, 0).unwrap()));
        RequestLimits::new(RateLimiter::new(clock), per_user, per_address, ForwardHeaders::Native).unwrap()
    }

    /// A router with both layers; `x-user` stands in for the authentication layer.
    fn router(limits: RequestLimits) -> Router {
        Router::new()
            .fallback(any(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(limits.clone(), limit_by_user))
            .layer(axum::middleware::from_fn(
                |mut request: Request, next: Next| async move {
                    if let Some(user) = request
                        .headers()
                        .get("x-user")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned)
                    {
                        let id: UserId = user.parse().unwrap();
                        request.extensions_mut().insert(CurrentUser::new(id));
                    }
                    next.run(request).await
                },
            ))
            .layer(axum::middleware::from_fn_with_state(limits, limit_by_client_address))
    }

    async fn call(router: &Router, path: &str, user: Option<&str>, address: &str) -> Response {
        let mut request = Request::builder()
            .uri(path)
            .header("x-forwarded-for", address)
            .body(Body::empty())
            .unwrap();
        if let Some(user) = user {
            request
                .headers_mut()
                .insert("x-user", HeaderValue::from_str(user).unwrap());
        }
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([172, 18, 0, 1], 50_000))));
        router.clone().oneshot(request).await.unwrap()
    }

    #[tokio::test]
    async fn a_user_over_the_budget_gets_429_with_retry_after_and_others_do_not() {
        let router = router(limits(3, 100));
        let (busy, calm) = (UserId::new_v7().to_string(), UserId::new_v7().to_string());

        for _ in 0..3 {
            assert_eq!(
                call(&router, "/api/v1/me", Some(&busy), "20.0.0.1").await.status(),
                StatusCode::OK
            );
        }
        let refused = call(&router, "/api/v1/nothing-here", Some(&busy), "20.0.0.1").await;

        assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
        // Three per minute: the next token is 20 seconds away.
        assert_eq!(refused.headers()[header::RETRY_AFTER], "20");
        assert_eq!(
            call(&router, "/api/v1/me", Some(&calm), "20.0.0.1").await.status(),
            StatusCode::OK
        );
        // Without a user the user limit does not apply.
        assert_eq!(
            call(&router, "/api/v1/me", None, "20.0.0.1").await.status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn token_free_endpoints_are_limited_per_client_address() {
        let router = router(limits(100, 2));

        for _ in 0..2 {
            assert_eq!(
                call(&router, "/api/v1/auth/login", None, "20.0.0.1").await.status(),
                StatusCode::OK
            );
        }
        let refused = call(&router, "/api/v1/auth/refresh", None, "20.0.0.1").await;

        assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(refused.headers()[header::RETRY_AFTER], "30");
        assert_eq!(
            call(&router, "/api/v1/auth/login", None, "20.0.0.2").await.status(),
            StatusCode::OK
        );
        // Only the prefix with its slash is limited, as Java's `startsWith("/api/v1/auth/")`.
        assert_eq!(
            call(&router, "/api/v1/auth", None, "20.0.0.1").await.status(),
            StatusCode::OK
        );
        assert_eq!(
            call(&router, "/api/v1/me", None, "20.0.0.1").await.status(),
            StatusCode::OK
        );
    }

    #[test]
    fn retry_after_is_rounded_up_to_whole_seconds_and_at_least_one() {
        let seconds =
            |millis| rate_limited(std::time::Duration::from_millis(millis)).headers()[header::RETRY_AFTER].clone();

        assert_eq!(seconds(0), "1");
        assert_eq!(seconds(1), "1");
        assert_eq!(seconds(1000), "1");
        assert_eq!(seconds(1001), "2");
    }
}
