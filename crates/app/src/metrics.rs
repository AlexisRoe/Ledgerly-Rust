//! Metrics in the Prometheus text format, for `/actuator/prometheus` on the management port.
//!
//! Java counterpart: Micrometer with the Prometheus registry (`spring-boot-starter-actuator`,
//! `micrometer-registry-prometheus`). The modules count with the `metrics` macros under Java's meter names
//! (`ledgerly.expenses.transitions`, `ledgerly.receipts.quarantined`, `ledgerly.imports.rows.skipped`, and the
//! `ledgerly.s3` and `ledgerly.clamav.scan` timers, which `shared::observation` keeps and renders itself in
//! Micrometer's shape); the recorder installed here gives the counters Micrometer's Prometheus names: dots
//! become underscores and counters end in `_total`.
//!
//! The JVM and Spring MVC series have Rust counterparts instead (decided after M1):
//! `process_resident_memory_bytes`, and the request histogram `http_server_request_duration_seconds` under
//! OpenTelemetry's name and labels.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use ledgerly_shared::web::MatchedRoute;
use metrics::{Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

/// OpenTelemetry's `http.server.request.duration`, in seconds.
const HTTP_SERVER_DURATION: &str = "http.server.request.duration.seconds";
/// OpenTelemetry's advised buckets for `http.server.request.duration`.
const HTTP_BUCKETS: [f64; 14] = [
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];

static HANDLE: OnceLock<Option<PrometheusHandle>> = OnceLock::new();

/// The process's recorder, installed on first use. A process has one global recorder, so tests that start the
/// app twice share it. `None` if another recorder was installed first, which only a test could do.
pub(crate) fn prometheus() -> Option<&'static PrometheusHandle> {
    HANDLE
        .get_or_init(|| {
            let recorder = builder().build_recorder();
            let handle = recorder.handle();
            metrics::set_global_recorder(MicrometerNames(recorder))
                .ok()
                .map(|()| handle)
        })
        .as_ref()
}

fn builder() -> PrometheusBuilder {
    let name = prometheus_name(HTTP_SERVER_DURATION);
    match PrometheusBuilder::new().set_buckets_for_metric(Matcher::Full(name), &HTTP_BUCKETS) {
        Ok(builder) => builder,
        // Only empty buckets are refused, and these are not; without buckets the histogram is still exported,
        // as a summary.
        Err(_) => PrometheusBuilder::new(),
    }
}

/// The exporter's rendering of a metric name: every character Prometheus does not allow becomes `_`.
fn prometheus_name(name: &str) -> String {
    name.replace('.', "_")
}

/// Micrometer's Prometheus convention the exporter does not follow: a counter's name ends in `_total`. The
/// modules keep Java's meter names; this recorder adds the suffix.
#[derive(Debug)]
struct MicrometerNames<R>(R);

impl<R: Recorder> Recorder for MicrometerNames<R> {
    fn describe_counter(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.0.describe_counter(total(key.as_str()).into(), unit, description);
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.0.describe_gauge(key, unit, description);
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.0.describe_histogram(key, unit, description);
    }

    fn register_counter(&self, key: &Key, metadata: &Metadata<'_>) -> Counter {
        let renamed = Key::from_parts(total(key.name()), key.labels().cloned().collect::<Vec<_>>());
        self.0.register_counter(&renamed, metadata)
    }

    fn register_gauge(&self, key: &Key, metadata: &Metadata<'_>) -> Gauge {
        self.0.register_gauge(key, metadata)
    }

    fn register_histogram(&self, key: &Key, metadata: &Metadata<'_>) -> Histogram {
        self.0.register_histogram(key, metadata)
    }
}

fn total(name: &str) -> String {
    format!("{name}.total")
}

/// Drains the histograms into their summaries now and then, as the exporter asks its user to.
pub(crate) fn keep_up() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async {
        let mut every = tokio::time::interval(Duration::from_secs(5));
        loop {
            every.tick().await;
            if let Some(handle) = prometheus() {
                handle.run_upkeep();
            }
        }
    })
}

/// The scrape: everything recorded, plus the process's resident memory where the system reports it.
pub(crate) fn render() -> String {
    let mut text = prometheus().map(PrometheusHandle::render).unwrap_or_default();
    text.push_str(&ledgerly_shared::observation::render());
    if let Some(bytes) = resident_memory_bytes() {
        text.push_str(&format!(
            "# HELP process_resident_memory_bytes Resident memory size in bytes.\n\
             # TYPE process_resident_memory_bytes gauge\n\
             process_resident_memory_bytes {bytes}\n"
        ));
    }
    text
}

/// `VmRSS` of `/proc/self/status`, on Linux (the container). Elsewhere `None`.
fn resident_memory_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes * 1024)
}

/// Middleware: every request's duration in `http_server_request_duration_seconds`, labelled with the method,
/// the status code and the route template, never the path.
pub(crate) async fn record_request(request: Request, next: Next) -> Response {
    let method = method_label(request.method());
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    let mut labels = vec![
        ("http.request.method", method.to_owned()),
        ("http.response.status_code", status),
    ];
    if let Some(MatchedRoute(template)) = response.extensions().get::<MatchedRoute>() {
        labels.push(("http.route", (*template).to_owned()));
    }
    metrics::histogram!(HTTP_SERVER_DURATION, &labels).record(started.elapsed().as_secs_f64());
    response
}

/// OpenTelemetry's rule: a method outside the standard ones is `_OTHER`, so a client cannot create series.
fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        Method::CONNECT => "CONNECT",
        Method::TRACE => "TRACE",
        _ => "_OTHER",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names Java's Prometheus registry gives Ledgerly's meters.
    #[test]
    fn the_meters_get_micrometers_prometheus_names() {
        let prometheus = builder().build_recorder();
        let handle = prometheus.handle();
        let recorder = MicrometerNames(prometheus);

        metrics::with_local_recorder(&recorder, || {
            metrics::counter!("ledgerly.expenses.transitions", "action" => "submit").increment(1);
            metrics::counter!("ledgerly.receipts.quarantined").increment(1);
            metrics::counter!("ledgerly.imports.rows.skipped").increment(2);
            metrics::histogram!(HTTP_SERVER_DURATION, "http.request.method" => "GET").record(0.002);
        });
        let text = handle.render();

        for expected in [
            "ledgerly_expenses_transitions_total{action=\"submit\"} 1",
            "ledgerly_receipts_quarantined_total 1",
            "ledgerly_imports_rows_skipped_total 2",
            "http_server_request_duration_seconds_bucket{http_request_method=\"GET\",le=\"0.005\"} 1",
        ] {
            assert!(text.contains(expected), "{expected} is not in\n{text}");
        }
    }

    #[test]
    fn unknown_methods_share_one_label() {
        assert_eq!(method_label(&Method::GET), "GET");
        assert_eq!(method_label(&Method::from_bytes(b"BREW").unwrap()), "_OTHER");
    }
}
