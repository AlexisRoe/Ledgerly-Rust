//! The refresh-token cookie, written as Spring's `ResponseCookie.toString()` writes it.
//!
//! Java counterpart: `AuthController.cookie`: `refresh_token`, `HttpOnly` (no script access), `Secure`,
//! `SameSite=Strict`, and sent only to `/api/v1/auth`.

use axum::http::{HeaderMap, HeaderValue};
use chrono::{TimeDelta, Utc};
use secrecy::{ExposeSecret, SecretString};

/// Java: `AuthController.COOKIE`.
pub(crate) const NAME: &str = "refresh_token";
/// Java: `AuthController.COOKIE_PATH`.
const PATH: &str = "/api/v1/auth";

/// `Set-Cookie` with `value`, living `max_age`.
///
/// `Expires` is the operating system's time plus `max_age` (Spring uses `System.currentTimeMillis()`, not the
/// application clock), or the epoch for a `max_age` of zero, in the format of Spring's `HttpHeaders.formatDate`.
pub(crate) fn set_cookie(value: &SecretString, max_age: TimeDelta) -> Option<HeaderValue> {
    let seconds = max_age.num_seconds();
    let expires = if seconds > 0 {
        Utc::now().checked_add_signed(max_age)?
    } else {
        chrono::DateTime::UNIX_EPOCH
    };
    let text = format!(
        "{NAME}={}; Path={PATH}; Max-Age={seconds}; Expires={}; Secure; HttpOnly; SameSite=Strict",
        value.expose_secret(),
        expires.format("%a, %d %b %Y %H:%M:%S GMT"),
    );
    let mut header = HeaderValue::from_str(&text).ok()?;
    // A header that carries a secret is kept out of HTTP/2 header compression tables.
    header.set_sensitive(true);
    Some(header)
}

/// The value of the first `refresh_token` cookie, as Spring's `@CookieValue` reads it with Tomcat's RFC 6265
/// parser: cookies are split at `;`, names compared exactly, values taken as they are.
pub(crate) fn read(headers: &HeaderMap) -> Option<SecretString> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == NAME)
        .map(|(_, value)| SecretString::from(strip_quotes(value.trim())))
}

/// RFC 6265 allows a value in double quotes; Tomcat hands it over without them.
fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use axum::http::header::COOKIE;

    use super::*;

    #[test]
    fn the_cookie_has_springs_attributes_in_springs_order() {
        let header = set_cookie(&SecretString::from("abc"), TimeDelta::days(14)).unwrap();
        let text = header.to_str().unwrap();

        let (start, rest) = text.split_once("; Expires=").unwrap();
        assert_eq!(start, "refresh_token=abc; Path=/api/v1/auth; Max-Age=1209600");
        let (date, end) = rest.split_once("; ").unwrap();
        assert_eq!(end, "Secure; HttpOnly; SameSite=Strict");
        assert!(
            chrono::DateTime::parse_from_rfc2822(&date.replace("GMT", "+0000")).is_ok(),
            "{date}"
        );
        assert!(header.is_sensitive());
    }

    #[test]
    fn clearing_the_cookie_expires_it_at_the_epoch() {
        let header = set_cookie(&SecretString::from(""), TimeDelta::zero()).unwrap();

        assert_eq!(
            header.to_str().unwrap(),
            "refresh_token=; Path=/api/v1/auth; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Secure; HttpOnly; \
             SameSite=Strict"
        );
    }

    fn cookies(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(COOKIE, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    fn read_text(values: &[&str]) -> Option<String> {
        read(&cookies(values)).map(|value| value.expose_secret().to_owned())
    }

    #[test]
    fn the_refresh_cookie_is_found_among_others() {
        assert_eq!(read_text(&["a=1; refresh_token=tok; b=2"]), Some("tok".to_owned()));
        assert_eq!(read_text(&["a=1", "refresh_token=tok"]), Some("tok".to_owned()));
        assert_eq!(read_text(&["refresh_token=\"tok\""]), Some("tok".to_owned()));
        assert_eq!(read_text(&["refresh_token="]), Some(String::new()));
    }

    #[test]
    fn the_first_one_wins_and_names_are_exact() {
        assert_eq!(
            read_text(&["refresh_token=one; refresh_token=two"]),
            Some("one".to_owned())
        );
        assert_eq!(read_text(&["Refresh_Token=x; xrefresh_token=y"]), None);
        assert_eq!(read_text(&[]), None);
    }
}
