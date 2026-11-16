//! Request budgets (API4): per signed-in user on every request, and per client address on the endpoints that
//! need no token.
//!
//! Java counterpart: `shared/security/ApiRateLimitFilter.java`, `AuthRateLimitFilter.java` and
//! `ProblemSecurityHandler.rateLimited`, placed in `SecurityConfig`'s filter chain: the address limit before the
//! token is checked, the user limit right after. The client address is what Tomcat reports, which depends on
//! `server.forward-headers-strategy` ([`ForwardHeaders`]).

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::current_user::CurrentUser;
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
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        let address = client_address(limits.forward_headers, peer, request.headers());
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

/// The client address as Java's `request.getRemoteAddr()` reports it. Without a peer (a request that did not
/// come over a socket, in tests) it is empty, so all such requests share one budget.
fn client_address(strategy: ForwardHeaders, peer: Option<IpAddr>, headers: &HeaderMap) -> String {
    let peer_text = peer.map(|ip| ip.to_string()).unwrap_or_default();
    match strategy {
        ForwardHeaders::None => peer_text,
        ForwardHeaders::Native => {
            if peer.is_some_and(is_internal_proxy) {
                remote_ip_valve(&forwarded_for(headers)).unwrap_or(peer_text)
            } else {
                peer_text
            }
        }
        ForwardHeaders::Framework => forwarded_header_filter(headers).unwrap_or(peer_text),
    }
}

/// Every `X-Forwarded-For` entry, left to right, over all such headers, as Tomcat joins them.
fn forwarded_for(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Tomcat's `RemoteIpValve`: from the right, skip internal proxies; the first other entry is the client. If
/// every entry is an internal proxy, the leftmost one is.
fn remote_ip_valve(entries: &[String]) -> Option<String> {
    let mut client = None;
    for entry in entries.iter().rev() {
        client = Some(entry.clone());
        if !entry.parse().is_ok_and(is_internal_proxy) {
            break;
        }
    }
    client
}

/// Tomcat's default `internalProxies`: 10/8, 192.168/16, 169.254/16, 127/8, 100.64/10, 172.16/12, `::1`,
/// fe80::/10 and fc00::/7.
fn is_internal_proxy(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 10
                || a == 127
                || (a == 192 && b == 168)
                || (a == 169 && b == 254)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 172 && (16..=31).contains(&b))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_internal_proxy(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xffc0) == 0xfe80 || (first & 0xfe00) == 0xfc00
        }
    }
}

/// Spring's `ForwardedHeaderFilter`: the `for` of the first element of `Forwarded` (RFC 7239), else the first
/// entry of `X-Forwarded-For`, without its port.
fn forwarded_header_filter(headers: &HeaderMap) -> Option<String> {
    let forwarded = headers
        .get(header::FORWARDED)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .and_then(|element| {
            element.split(';').find_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                name.eq_ignore_ascii_case("for")
                    .then(|| value.trim().trim_matches('"').to_owned())
            })
        });
    let address = forwarded.or_else(|| forwarded_for(headers).into_iter().next())?;
    Some(without_port(&address).to_owned()).filter(|host| !host.is_empty())
}

/// `[::1]:8080` → `[::1]`, `1.2.3.4:5` → `1.2.3.4`; a bare IPv6 address stays as it is.
fn without_port(address: &str) -> &str {
    if let Some(end) = address.find(']') {
        return address.get(..=end).unwrap_or(address);
    }
    match address.split_once(':') {
        Some((host, port)) if !port.contains(':') => host,
        _ => address,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::any;
    use chrono::{TimeZone, Utc};
    use tower::ServiceExt;

    use super::*;
    use crate::clock::FixedClock;
    use crate::id::UserId;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(*name, HeaderValue::from_static(value));
        }
        headers
    }

    fn ip(text: &str) -> Option<IpAddr> {
        Some(text.parse().unwrap())
    }

    #[test]
    fn without_a_strategy_the_peer_is_the_client() {
        let forwarded = headers(&[("x-forwarded-for", "20.1.2.3")]);

        assert_eq!(
            client_address(ForwardHeaders::None, ip("10.0.0.5"), &forwarded),
            "10.0.0.5"
        );
    }

    #[test]
    fn native_trusts_the_header_only_from_an_internal_proxy() {
        let forwarded = headers(&[("x-forwarded-for", "20.1.2.3")]);

        assert_eq!(
            client_address(ForwardHeaders::Native, ip("172.18.0.1"), &forwarded),
            "20.1.2.3"
        );
        assert_eq!(
            client_address(ForwardHeaders::Native, ip("::1"), &forwarded),
            "20.1.2.3"
        );
        assert_eq!(
            client_address(ForwardHeaders::Native, ip("8.8.8.8"), &forwarded),
            "8.8.8.8"
        );
        assert_eq!(
            client_address(ForwardHeaders::Native, ip("10.0.0.5"), &HeaderMap::new()),
            "10.0.0.5"
        );
    }

    #[test]
    fn native_reads_from_the_right_and_skips_internal_proxies() {
        // A client cannot choose its address by putting one in front: the rightmost outside entry wins.
        let spoofed = headers(&[
            ("x-forwarded-for", "1.1.1.1, 20.1.2.3, 10.0.0.7"),
            ("x-forwarded-for", "192.168.1.1"),
        ]);
        let all_internal = headers(&[("x-forwarded-for", "10.0.0.1, 10.0.0.2")]);

        assert_eq!(
            client_address(ForwardHeaders::Native, ip("127.0.0.1"), &spoofed),
            "20.1.2.3"
        );
        assert_eq!(
            client_address(ForwardHeaders::Native, ip("127.0.0.1"), &all_internal),
            "10.0.0.1"
        );
    }

    #[test]
    fn tomcats_internal_proxies() {
        for internal in [
            "10.1.2.3",
            "127.0.0.1",
            "192.168.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "172.16.0.1",
            "172.31.255.255",
            "::1",
            "fe80::1",
            "fd00::1",
            "fc12::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_internal_proxy(internal.parse().unwrap()), "{internal}");
        }
        for outside in [
            "8.8.8.8",
            "100.128.0.1",
            "172.32.0.1",
            "192.169.0.1",
            "2001:db8::1",
            "fec0::1",
        ] {
            assert!(!is_internal_proxy(outside.parse().unwrap()), "{outside}");
        }
    }

    #[test]
    fn framework_takes_the_first_forwarded_entry_from_anyone() {
        let x_forwarded = headers(&[("x-forwarded-for", "20.1.2.3, 10.0.0.7")]);
        let forwarded = headers(&[
            ("forwarded", "for=\"[2001:db8::1]:4711\";proto=https, for=10.0.0.7"),
            ("x-forwarded-for", "20.1.2.3"),
        ]);
        let with_port = headers(&[("forwarded", "For=30.1.2.3:80")]);

        assert_eq!(
            client_address(ForwardHeaders::Framework, ip("8.8.8.8"), &x_forwarded),
            "20.1.2.3"
        );
        assert_eq!(
            client_address(ForwardHeaders::Framework, ip("8.8.8.8"), &forwarded),
            "[2001:db8::1]"
        );
        assert_eq!(
            client_address(ForwardHeaders::Framework, ip("8.8.8.8"), &with_port),
            "30.1.2.3"
        );
        assert_eq!(
            client_address(ForwardHeaders::Framework, ip("8.8.8.8"), &HeaderMap::new()),
            "8.8.8.8"
        );
    }

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
