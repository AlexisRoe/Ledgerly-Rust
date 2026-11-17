//! The cap on JSON request bodies, before anything else looks at the request.
//!
//! Java counterpart: `shared/web/JsonBodyLimitFilter.java` (`@Order(HIGHEST_PRECEDENCE)`, before Spring
//! Security), with `ledgerly.limits.max-json-body-size`. A JSON request that declares a `Content-Length` over
//! the limit is answered 413 with `Connection: close` without parsing the body, whoever sends it and wherever
//! it goes, so not even an unauthenticated caller can make the server parse a huge document. A body without a
//! length (chunked) is counted while it is read: the layer hands the limit to [`super::read_json`].
//!
//! Before the 413 goes out, up to 16 MiB of the refused body are read and thrown away, as Tomcat does
//! (`server.tomcat.max-swallow-size: 16MB` in `application.yaml`). A client still sending would otherwise have
//! its connection reset under it and never read the answer.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;

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
        swallow(request.into_body()).await;
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

/// Tomcat's `max-swallow-size`: the most of a refused body that is read so the client gets the answer.
const MAX_SWALLOW_SIZE: usize = 16 * 1024 * 1024;

/// Reads and drops the body until it ends, fails or passes [`MAX_SWALLOW_SIZE`]; whatever is left is not read,
/// and the connection closes after the answer.
async fn swallow(mut body: Body) {
    let mut swallowed = 0_usize;
    while let Some(Ok(frame)) = body.frame().await {
        swallowed = swallowed.saturating_add(frame.data_ref().map_or(0, |data| data.len()));
        if swallowed > MAX_SWALLOW_SIZE {
            break;
        }
    }
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

    /// The refused body is read to its end (so the client can finish sending and read the 413), but never
    /// handed on.
    #[tokio::test]
    async fn a_declared_oversized_body_is_swallowed_and_refused() {
        use axum::Router;
        use axum::routing::post;
        use tower::ServiceExt;

        let router = Router::new()
            .route("/x", post(|| async { "parsed" }))
            .layer(axum::middleware::from_fn_with_state(10_u64, limit_json_bodies));
        let request = Request::post("/x")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, "20")
            .body(Body::from("01234567890123456789"))
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response.headers()[header::CONNECTION], "close");
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!body.starts_with(b"parsed"));
    }

    #[tokio::test]
    async fn swallowing_stops_after_sixteen_mebibytes() {
        let body = Body::from(vec![b'x'; MAX_SWALLOW_SIZE + 1024 * 1024]);
        tokio::time::timeout(std::time::Duration::from_secs(5), swallow(body))
            .await
            .unwrap();
    }

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
