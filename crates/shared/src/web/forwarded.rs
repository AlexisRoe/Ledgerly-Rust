//! What a request says about its client when a proxy stands in between: the client address and the origin
//! the client used, as Tomcat and Spring report them for each `server.forward-headers-strategy`.
//!
//! Java counterpart: none in Ledgerly-Java's code. Tomcat's `RemoteIpValve` (Spring Boot configures it for
//! `native`) and Spring's `ForwardedHeaderFilter` (`framework`) change what `getRemoteAddr()`, `getScheme()`,
//! `getServerName()` and `getServerPort()` return; `AuthRateLimitFilter` and the CORS same-origin check read
//! them.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, Request};
use axum::http::{HeaderMap, header};

use crate::config::ForwardHeaders;

/// The peer of the connection, if the request came over a socket.
pub(crate) fn peer(request: &Request) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip())
}

/// Whether `strategy` lets this request's headers speak for the client: always for `framework`, only from an
/// internal proxy for `native`.
fn trusts_headers(strategy: ForwardHeaders, peer: Option<IpAddr>) -> bool {
    match strategy {
        ForwardHeaders::None => false,
        ForwardHeaders::Native => peer.is_some_and(is_internal_proxy),
        ForwardHeaders::Framework => true,
    }
}

/// The scheme, host and port the client addressed, as Spring's `CorsUtils.isSameOrigin` takes them from the
/// request: `http` (the server speaks no TLS) and the `Host` header, unless a trusted proxy says otherwise in
/// `X-Forwarded-Proto`, `X-Forwarded-Host` and `X-Forwarded-Port`. The port is never absent: the scheme's
/// default stands in.
pub(crate) fn request_origin(
    strategy: ForwardHeaders,
    peer: Option<IpAddr>,
    headers: &HeaderMap,
) -> (String, String, u16) {
    let text = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let (host, port) = split_host(text(header::HOST.as_str()).unwrap_or_default());
    let mut scheme = "http".to_owned();
    let mut host = host.to_owned();
    let mut port = port;
    if trusts_headers(strategy, peer) {
        if let Some(proto) = text("x-forwarded-proto") {
            let secure = proto.split(',').all(|entry| entry.trim().eq_ignore_ascii_case("https"));
            scheme = if secure { "https" } else { "http" }.to_owned();
            port = None;
        }
        if let Some(forwarded_host) = text("x-forwarded-host") {
            let first = forwarded_host.split(',').next().unwrap_or_default().trim();
            host = split_host(first).0.to_owned();
        }
        if let Some(forwarded_port) = text("x-forwarded-port").and_then(|value| value.parse().ok()) {
            port = Some(forwarded_port);
        }
    }
    let port = port.unwrap_or(if scheme == "https" { 443 } else { 80 });
    (scheme, host, port)
}

/// `example.com:8080` → (`example.com`, 8080); `[::1]:80` → (`[::1]`, 80); no port → `None`.
pub(crate) fn split_host(value: &str) -> (&str, Option<u16>) {
    let colon = match value.find(']') {
        Some(end) => value
            .get(end + 1..)
            .filter(|rest| rest.starts_with(':'))
            .map(|_| end + 1),
        None => value.rfind(':'),
    };
    match colon {
        Some(index) => (
            value.get(..index).unwrap_or(value),
            value.get(index + 1..).and_then(|port| port.parse().ok()),
        ),
        None => (value, None),
    }
}

/// The client address as Java's `request.getRemoteAddr()` reports it. Without a peer (a request that did not
/// come over a socket, in tests) it is empty, so all such requests share one budget.
pub(crate) fn client_address(strategy: ForwardHeaders, peer: Option<IpAddr>, headers: &HeaderMap) -> String {
    let peer_text = peer.map(|ip| ip.to_string()).unwrap_or_default();
    if !trusts_headers(strategy, peer) {
        return peer_text;
    }
    match strategy {
        ForwardHeaders::Native => remote_ip_valve(&forwarded_for(headers)).unwrap_or(peer_text),
        ForwardHeaders::Framework => forwarded_header_filter(headers).unwrap_or(peer_text),
        ForwardHeaders::None => peer_text,
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
    use axum::http::HeaderValue;

    use super::*;

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

    #[test]
    fn the_origin_is_the_host_header_unless_a_trusted_proxy_says_otherwise() {
        let plain = headers(&[("host", "api.example.com:8080")]);
        let proxied = headers(&[
            ("host", "ledgerly:8080"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "api.example.com"),
        ]);
        let origin = |strategy, peer, headers| request_origin(strategy, peer, headers);

        assert_eq!(
            origin(ForwardHeaders::None, ip("10.0.0.1"), &plain),
            ("http".to_owned(), "api.example.com".to_owned(), 8080)
        );
        assert_eq!(
            origin(ForwardHeaders::None, ip("10.0.0.1"), &proxied),
            ("http".to_owned(), "ledgerly".to_owned(), 8080)
        );
        assert_eq!(
            origin(ForwardHeaders::Native, ip("10.0.0.1"), &proxied),
            ("https".to_owned(), "api.example.com".to_owned(), 443)
        );
        assert_eq!(
            origin(ForwardHeaders::Native, ip("8.8.8.8"), &proxied),
            ("http".to_owned(), "ledgerly".to_owned(), 8080)
        );
        assert_eq!(
            origin(ForwardHeaders::None, None, &headers(&[("host", "[::1]")])),
            ("http".to_owned(), "[::1]".to_owned(), 80)
        );
    }
}
