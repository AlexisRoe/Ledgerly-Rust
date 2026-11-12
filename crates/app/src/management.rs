//! The management listener: Spring Boot Actuator's endpoints, as Ledgerly-Java exposes them on port 8081.
//!
//! Java counterpart: `management.*` in `application.yaml` (`exposure.include: health,info,prometheus`,
//! `endpoint.health.probes.enabled: true`). The bodies, status codes and content type are Spring Boot's, because
//! orchestrators and dashboards written for the Java service read them (ADR 0013).
//! `/actuator/prometheus` arrives with the metrics in M10.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};

use crate::server::Shutdown;

/// The media type Actuator answers with when the client does not ask for another one.
const ACTUATOR_V3: &str = "application/vnd.spring-boot.actuator.v3+json";

/// What `/actuator/info` reports about the build, in the shape of Spring Boot's `build-info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: &'static str,
    /// When the binary was built, as an ISO 8601 instant, if the build recorded it.
    pub time: Option<&'static str>,
}

impl BuildInfo {
    /// The build of this binary. The image build passes the time in `LEDGERLY_BUILD_TIME`.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            time: option_env!("LEDGERLY_BUILD_TIME"),
        }
    }
}

#[derive(Debug, Clone)]
struct Management {
    shutdown: Shutdown,
    build: BuildInfo,
}

/// The router of the management listener. Anything it does not name is a 404.
pub(crate) fn router(shutdown: Shutdown, build: BuildInfo) -> Router {
    Router::new()
        .route("/actuator/health", get(health))
        .route("/actuator/health/liveness", get(liveness))
        .route("/actuator/health/readiness", get(readiness))
        .route("/actuator/info", get(info))
        .fallback(not_found)
        .with_state(Management { shutdown, build })
}

/// Spring's availability states as health statuses. Liveness is `UP` for as long as the process can answer.
/// Readiness turns `OUT_OF_SERVICE` when shutdown starts, so a load balancer stops sending traffic while
/// requests in flight finish.
fn readiness_status(state: &Management) -> &'static str {
    if state.shutdown.has_started() {
        "OUT_OF_SERVICE"
    } else {
        "UP"
    }
}

/// Spring maps `DOWN` and `OUT_OF_SERVICE` to 503 and everything else to 200.
fn status_code(status: &str) -> StatusCode {
    match status {
        "DOWN" | "OUT_OF_SERVICE" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::OK,
    }
}

fn actuator(status: StatusCode, body: &Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, HeaderValue::from_static(ACTUATOR_V3))],
        body.to_string(),
    )
        .into_response()
}

/// The overall health: the worst of its parts, with the names of the probe groups and no details.
async fn health(State(state): State<Management>) -> Response {
    let status = readiness_status(&state);
    actuator(
        status_code(status),
        &json!({"status": status, "groups": ["liveness", "readiness"]}),
    )
}

async fn liveness() -> Response {
    actuator(StatusCode::OK, &json!({"status": "UP"}))
}

async fn readiness(State(state): State<Management>) -> Response {
    let status = readiness_status(&state);
    actuator(status_code(status), &json!({"status": status}))
}

async fn info(State(state): State<Management>) -> Response {
    let mut build = json!({
        "artifact": "ledgerly",
        "name": "ledgerly",
        "version": state.build.version,
        "group": "com.ledgerly",
    });
    if let (Some(time), Some(fields)) = (state.build.time, build.as_object_mut()) {
        fields.insert("time".to_owned(), Value::from(time));
    }
    actuator(StatusCode::OK, &json!({ "build": build }))
}

/// Spring Boot's error body for the management context.
async fn not_found(uri: Uri) -> Response {
    let body = json!({
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, false),
        "status": 404,
        "error": "Not Found",
        "path": uri.path(),
    });
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        body.to_string(),
    )
        .into_response()
}
