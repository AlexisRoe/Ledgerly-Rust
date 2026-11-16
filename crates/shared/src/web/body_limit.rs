//! The cap on JSON request bodies, before anything else looks at the request.
//!
//! Java counterpart: `shared/web/JsonBodyLimitFilter.java` (`@Order(HIGHEST_PRECEDENCE)`, before Spring
//! Security), with `ledgerly.limits.max-json-body-size`. A JSON request that declares a `Content-Length` over
//! the limit is answered 413 with `Connection: close` without reading the body, whoever sends it and wherever
//! it goes, so not even an unauthenticated caller can make the server read a huge document. A body without a
//! length (chunked) is counted while it is read: the layer hands the limit to [`super::read_json`].

use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::problem::Problem;

/// The configured limit, in bytes, as a request extension for [`super::read_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonBodyLimit(pub u64);

/// The layer: `limit` is `ledgerly.limits.max-json-body-size`.
pub async fn limit_json_bodies(State(limit): State<u64>, mut request: Request, next: Next) -> Response {
    if !is_json(request.headers().get(header::CONTENT_TYPE)) {
        return next.run(request).await;
    }
    let declared = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok());
    if declared.is_some_and(|length| length > limit) {
        let mut response = too_large(limit).into_response();
        // The body was not read: the connection is not kept for it.
        response
            .headers_mut()
            .insert(header::CONNECTION, HeaderValue::from_static("close"));
        return response;
    }
    request.extensions_mut().insert(JsonBodyLimit(limit));
    next.run(request).await
}

/// Java's `PayloadTooLarge("The request body is larger than " + limit + " bytes")`.
pub(crate) fn too_large(limit: u64) -> Problem {
    Problem::payload_too_large(format!("The request body is larger than {limit} bytes"))
}

/// Spring's test: `isCompatibleWith(application/json)` (which also takes `*/*` and `application/*`), or the
/// subtype `json`, or a subtype ending in `+json`. A type that does not parse is not JSON.
fn is_json(content_type: Option<&HeaderValue>) -> bool {
    let Some(text) = content_type.and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let essence = text.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    let Some((kind, subtype)) = essence.split_once('/') else {
        return false;
    };
    if kind.is_empty() || subtype.is_empty() {
        return false;
    }
    let compatible = matches!((kind, subtype), ("*", "*") | ("application", "*" | "json"));
    compatible || subtype == "json" || subtype.ends_with("+json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_what_spring_counts_as_json() {
        for json in [
            "application/json",
            "application/json; charset=UTF-8",
            "Application/JSON",
            "application/problem+json",
            "text/json",
            "*/*",
            "application/*",
        ] {
            assert!(is_json(Some(&HeaderValue::from_static(json))), "{json}");
        }
        for other in [
            "text/plain",
            "multipart/form-data; boundary=x",
            "json",
            "",
            "application/xml",
        ] {
            assert!(!is_json(Some(&HeaderValue::from_static(other))), "{other}");
        }
        assert!(!is_json(None));
    }
}
