//! The API router's layers working together: every rejection is a problem body with the request's path and
//! trace id, and the trace id in the body is the one in the log line.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use ledgerly_shared::config::TracingConfig;
use ledgerly_shared::id::UserId;
use ledgerly_shared::web::{Body as Consumes, Routes};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::logging::testing::Captured;

/// Accepts the token `test`.
#[derive(Debug)]
struct TestTokens;

impl TokenVerifier for TestTokens {
    fn verify(&self, token: &str) -> Option<UserId> {
        (token == "test").then(UserId::new_v7)
    }
}

/// The real layers around a table with a JSON endpoint and a handler that panics.
fn app() -> Router {
    let telemetry = Telemetry::new(&TracingConfig::default()).unwrap();
    let routes = Routes::new()
        .route(
            Method::POST,
            "/api/v1/teams/{teamId}/expenses",
            Consumes::Json,
            || async { "created" },
            (),
        )
        .route(
            Method::GET,
            "/api/v1/boom",
            Consumes::Ignored,
            || async {
                let parts: Vec<&str> = Vec::new();
                // Indexing an empty list stands in for any bug that panics in a handler.
                #[allow(clippy::indexing_slicing)]
                parts[0].to_owned()
            },
            (),
        );
    layered(
        routes,
        Authentication::new(Some(Arc::new(TestTokens)), false),
        &telemetry,
    )
}

/// Sends `request` as a signed-in caller.
async fn send(mut request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    request
        .headers_mut()
        .insert("authorization", axum::http::HeaderValue::from_static("Bearer test"));
    send_anonymously(request).await
}

async fn send_anonymously(request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = app().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    (status, headers, serde_json::from_slice(&body).unwrap())
}

fn is_trace_id(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Java: `AuthApiTest.unauthenticatedResponsesAreProblemDetails`: the 401 of the authentication layer is a
/// catalogued problem with the challenge, the path and the trace id.
#[tokio::test]
async fn without_a_token_the_answer_is_a_401_problem_with_instance_and_trace_id() {
    let (status, headers, body) =
        send_anonymously(Request::get("/api/v1/teams/x/expenses").body(Body::empty()).unwrap()).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers["www-authenticate"], "Bearer");
    assert_eq!(headers["content-type"], "application/problem+json");
    assert_eq!(body["code"], "UNAUTHENTICATED");
    assert_eq!(body["detail"], "Authentication required");
    assert_eq!(body["instance"], "/api/v1/teams/x/expenses");
    assert!(is_trace_id(&body["traceId"]), "{body}");
}

/// Inventory §4.2 "Authentication before routing": an unknown path or a wrong method is 401 without a token,
/// and only a signed-in caller learns that it is 404 or 405.
#[tokio::test]
async fn authentication_comes_before_routing() {
    for request in [
        || Request::get("/api/v1/nope").body(Body::empty()).unwrap(),
        || Request::put("/api/v1/boom").body(Body::empty()).unwrap(),
    ] {
        assert_eq!(send_anonymously(request()).await.0, StatusCode::UNAUTHORIZED);
        assert_ne!(send(request()).await.0, StatusCode::UNAUTHORIZED);
    }
}

/// Java: `ExpensesApiTest.unknownPathIsAProblemToo`, and the body Spring's `NoResourceFoundException` gets
/// from `ProblemFactory.enrich`.
#[tokio::test]
async fn an_unknown_route_is_springs_404_with_request_rejected() {
    let (status, headers, body) = send(Request::get("/api/v1/nope").body(Body::empty()).unwrap()).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers["content-type"], "application/problem+json");
    assert!(is_trace_id(&body["traceId"]), "{body}");
    assert_eq!(
        body,
        json!({
            "type": "about:blank",
            "title": "Not Found",
            "status": 404,
            "detail": "No static resource api/v1/nope.",
            "instance": "/api/v1/nope",
            "code": "REQUEST_REJECTED",
            "traceId": body["traceId"],
        })
    );
}

#[tokio::test]
async fn a_wrong_method_is_405_with_allow() {
    let (status, headers, body) = send(
        Request::delete("/api/v1/teams/t1/expenses")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(headers["allow"], "POST");
    assert_eq!(body["code"], "REQUEST_REJECTED");
    assert_eq!(body["title"], "Method Not Allowed");
    assert_eq!(body["detail"], "Method 'DELETE' is not supported.");
    assert_eq!(body["instance"], "/api/v1/teams/t1/expenses");
}

#[tokio::test]
async fn a_body_that_is_not_json_is_415_with_accept() {
    let (status, headers, body) = send(
        Request::post("/api/v1/teams/t1/expenses")
            .header("content-type", "text/plain")
            .body(Body::from("hello"))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(headers["accept"], "application/json");
    assert_eq!(body["code"], "REQUEST_REJECTED");
    assert_eq!(body["title"], "Unsupported Media Type");
    assert_eq!(body["detail"], "Content-Type 'text/plain' is not supported.");
}

/// Java: `ProblemExceptionHandler.unexpected` logs "Unhandled exception" and answers a 500 that names nothing
/// but the trace (inventory §4.1: the keys are a subset of type, title, status, code, traceId, instance, detail).
#[tokio::test(flavor = "current_thread")]
async fn a_panic_is_a_500_whose_trace_id_is_the_one_in_the_log_line() {
    let captured = Captured::default();
    let _logging = tracing::subscriber::set_default(captured.subscriber());

    let (status, _, body) = send(Request::get("/api/v1/boom").body(Body::empty()).unwrap()).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({
            "type": "https://ledgerly.dev/problems/internal-error",
            "title": "Unexpected error",
            "status": 500,
            "instance": "/api/v1/boom",
            "code": "INTERNAL_ERROR",
            "traceId": body["traceId"],
        })
    );
    let lines = captured.lines();
    let logged = lines
        .iter()
        .find(|line| line["message"] == "Unhandled exception")
        .unwrap_or_else(|| panic!("no log line for the panic in {lines:?}"));
    assert_eq!(logged["log"]["level"], "ERROR");
    assert!(is_trace_id(&body["traceId"]));
    assert_eq!(logged["traceId"], body["traceId"]);
    assert!(
        !captured.text().contains("index out of bounds"),
        "the panic message stays out of the log"
    );
}
