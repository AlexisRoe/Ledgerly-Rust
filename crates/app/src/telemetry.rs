//! Request tracing: one span per request with W3C trace ids, exported over OTLP when an endpoint is configured.
//!
//! Java counterpart: Micrometer Tracing with the OpenTelemetry bridge (`spring-boot-starter-opentelemetry`,
//! `management.tracing.sampling.probability: 1.0`), which puts `traceId` and `spanId` into the MDC.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use ledgerly_shared::config::TracingConfig;
use opentelemetry::KeyValue;
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::{Span, SpanKind, Tracer, TracerProvider};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use tracing::Instrument;

/// The ids of the request being handled, as lower-case hex: 32 characters and 16 characters.
///
/// Stored in the request's extensions, so that whatever writes the response (a problem body, from M3) reports
/// the same `traceId` the log lines carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    trace_id: String,
    span_id: String,
}

impl TraceContext {
    #[must_use]
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    #[must_use]
    pub fn span_id(&self) -> &str {
        &self.span_id
    }
}

/// Creates the spans and, if configured, exports them.
#[derive(Debug, Clone)]
pub struct Telemetry {
    provider: SdkTracerProvider,
}

/// The trace exporter could not be set up. The message names no endpoint.
#[derive(Debug, thiserror::Error)]
#[error("the OTLP trace exporter could not be created; check MANAGEMENT_OPENTELEMETRY_TRACING_EXPORT_OTLP_ENDPOINT")]
pub struct TelemetryError(#[source] opentelemetry_otlp::ExporterBuildError);

impl Telemetry {
    /// Sets tracing up. With an endpoint, finished spans are sent there in batches (OTLP over HTTP, protobuf)
    /// from a background thread. Without one, spans still get their ids but go nowhere.
    ///
    /// Call it outside the async runtime: the exporter's HTTP client is a blocking one.
    ///
    /// # Errors
    ///
    /// Returns a [`TelemetryError`] if the exporter cannot be built.
    pub fn new(config: &TracingConfig) -> Result<Self, TelemetryError> {
        let mut provider =
            SdkTracerProvider::builder().with_resource(Resource::builder().with_service_name("ledgerly").build());
        if let Some(endpoint) = &config.otlp_endpoint {
            let exporter = SpanExporter::builder()
                .with_http()
                .with_endpoint(endpoint.clone())
                .build()
                .map_err(TelemetryError)?;
            provider = provider.with_batch_exporter(exporter);
        }
        Ok(Self {
            provider: provider.build(),
        })
    }

    pub(crate) fn tracer(&self) -> SdkTracer {
        self.provider.tracer("ledgerly")
    }

    /// Sends what is still buffered and stops the exporter. Call it once, after the listeners have stopped and
    /// outside the async runtime.
    pub fn shutdown(&self) {
        // Losing the last spans is not worth failing a clean exit for, and there is no log to report it to.
        let _ = self.provider.shutdown();
    }
}

struct Headers<'a>(&'a HeaderMap);

impl Extractor for Headers<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}

/// Middleware: runs the request inside a span. The trace continues the caller's `traceparent` header if there
/// is a valid one and starts fresh otherwise.
pub(crate) async fn trace_request(State(tracer): State<SdkTracer>, mut request: Request, next: Next) -> Response {
    let parent = TraceContextPropagator::new().extract(&Headers(request.headers()));
    let mut span = tracer
        .span_builder(format!("http {}", request.method().as_str().to_ascii_lowercase()))
        .with_kind(SpanKind::Server)
        .with_attributes([KeyValue::new(
            "http.request.method",
            request.method().as_str().to_owned(),
        )])
        .start_with_context(&tracer, &parent);
    let context = TraceContext {
        trace_id: span.span_context().trace_id().to_string(),
        span_id: span.span_context().span_id().to_string(),
    };
    // Java's MDC keys. `userId` and `teamId` are filled in by the layers that learn them (M5, M6).
    let log_span = tracing::info_span!(
        "request",
        traceId = context.trace_id,
        spanId = context.span_id,
        userId = tracing::field::Empty,
        teamId = tracing::field::Empty,
    );
    request.extensions_mut().insert(context);

    let response = next.run(request).instrument(log_span).await;

    span.set_attribute(KeyValue::new(
        "http.response.status_code",
        i64::from(response.status().as_u16()),
    ));
    span.end();
    response
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::*;
    use crate::logging::testing::Captured;

    /// A router with the tracing layer and one route that logs a line and reports the ids it was given.
    fn traced() -> Router {
        let telemetry = Telemetry::new(&TracingConfig::default()).unwrap();
        Router::new()
            .route(
                "/ids",
                get(|request: Request| async move {
                    tracing::info!("handling");
                    let context = request.extensions().get::<TraceContext>().unwrap();
                    format!("{} {}", context.trace_id(), context.span_id())
                }),
            )
            .layer(axum::middleware::from_fn_with_state(telemetry.tracer(), trace_request))
    }

    async fn ids(router: Router, traceparent: Option<&str>) -> (String, String) {
        let mut request = axum::http::Request::get("/ids");
        if let Some(traceparent) = traceparent {
            request = request.header("traceparent", traceparent);
        }
        let response = router.oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        let (trace_id, span_id) = text.split_once(' ').unwrap();
        (trace_id.to_owned(), span_id.to_owned())
    }

    fn is_lower_hex(text: &str, length: usize) -> bool {
        text.len() == length && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[tokio::test]
    async fn every_request_gets_w3c_ids_of_32_and_16_hex_characters() {
        let (trace_id, span_id) = ids(traced(), None).await;

        assert!(is_lower_hex(&trace_id, 32), "{trace_id}");
        assert!(is_lower_hex(&span_id, 16), "{span_id}");
        assert_ne!(trace_id, "0".repeat(32));
        assert_ne!(span_id, "0".repeat(16));
    }

    #[tokio::test]
    async fn two_requests_get_different_traces() {
        let router = traced();

        assert_ne!(ids(router.clone(), None).await.0, ids(router, None).await.0);
    }

    #[tokio::test]
    async fn a_valid_traceparent_is_continued_with_a_new_span() {
        let (trace_id, span_id) = ids(
            traced(),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        )
        .await;

        assert_eq!(trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert!(is_lower_hex(&span_id, 16));
        assert_ne!(span_id, "b7ad6b7169203331");
    }

    #[tokio::test]
    async fn an_invalid_traceparent_starts_a_fresh_trace() {
        for header in [
            "garbage",
            "00-00000000000000000000000000000000-b7ad6b7169203331-01",
            "00-0af7651916cd43dd8448eb211c80319c-0000000000000000-01",
        ] {
            let (trace_id, _) = ids(traced(), Some(header)).await;
            assert!(is_lower_hex(&trace_id, 32), "{header}");
            assert_ne!(trace_id, "0af7651916cd43dd8448eb211c80319c", "{header}");
            assert_ne!(trace_id, "0".repeat(32), "{header}");
        }
    }

    /// The log line and the response body must name the same trace: M3 puts this id into problem bodies.
    #[tokio::test(flavor = "current_thread")]
    async fn a_log_line_inside_a_request_carries_the_requests_ids() {
        let captured = Captured::default();
        let _logging = tracing::subscriber::set_default(captured.subscriber());

        let (trace_id, span_id) = ids(traced(), None).await;

        let lines = captured.lines();
        let [line] = lines.as_slice() else {
            panic!("expected one line, got {lines:?}")
        };
        assert_eq!(line["message"], "handling");
        assert_eq!(line["traceId"], trace_id.as_str());
        assert_eq!(line["spanId"], span_id.as_str());
        // `userId` and `teamId` are absent until something records them, like an MDC key that was never put.
        let keys: Vec<&str> = line.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "@timestamp",
                "ecs",
                "log",
                "message",
                "process",
                "service",
                "spanId",
                "traceId"
            ]
        );
    }

    #[test]
    fn an_exporter_for_a_configured_endpoint_can_be_built_and_shut_down() {
        let telemetry = Telemetry::new(&TracingConfig {
            otlp_endpoint: Some("http://127.0.0.1:9/v1/traces".to_owned()),
        })
        .unwrap();

        telemetry.shutdown();
    }
}
