//! Timers and spans around calls to other systems: one observation per call, recorded as a Prometheus timer
//! and an OpenTelemetry span with the same tags.
//!
//! Java counterpart: Micrometer's `Observation` with the default meter and tracing handlers, as
//! `S3ObjectStorage` (`ledgerly.s3`, tagged `operation`) and `ClamAvScanner` (`ledgerly.clamav.scan`, tagged
//! `outcome`) use it. A finished observation becomes the timer `<name>` (`<name>_seconds` in Prometheus) with
//! its low-cardinality tags plus `error`, which is `none` or the kind of failure; and a span named by the
//! contextual name, a child of the request's span when there is one.
//!
//! Tags are fixed words: never a key, a file name, an id or an address, so the number of series stays small
//! and nothing personal reaches the metrics.

use std::time::Instant;

use opentelemetry::KeyValue;
use opentelemetry::global::{self, BoxedSpan};
use opentelemetry::trace::{Span, Status, Tracer};

/// One timed call. Stop it with [`Observation::stop`]; dropping it unstopped records nothing.
pub struct Observation {
    name: &'static str,
    started: Instant,
    span: BoxedSpan,
    tags: Vec<(&'static str, &'static str)>,
    error: &'static str,
}

impl std::fmt::Debug for Observation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Observation")
            .field("name", &self.name)
            .field("tags", &self.tags)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl Observation {
    /// Starts the timer and the span `contextual_name` (Micrometer's `contextualName`), under the current
    /// OpenTelemetry context.
    #[must_use]
    pub fn start(name: &'static str, contextual_name: &'static str) -> Self {
        let span = global::tracer("ledgerly").start(contextual_name);
        Self {
            name,
            started: Instant::now(),
            span,
            tags: Vec::new(),
            error: "none",
        }
    }

    /// Adds a low-cardinality tag to both the timer and the span.
    #[must_use]
    pub fn with_tag(mut self, key: &'static str, value: &'static str) -> Self {
        self.tag(key, value);
        self
    }

    /// Adds a low-cardinality tag to both the timer and the span.
    pub fn tag(&mut self, key: &'static str, value: &'static str) {
        self.span.set_attribute(KeyValue::new(key, value));
        self.tags.push((key, value));
    }

    /// Marks the call as failed with `kind` (Java: the exception's simple class name).
    pub fn error(&mut self, kind: &'static str) {
        self.error = kind;
        self.span.set_status(Status::error(kind));
    }

    /// Records the timer and ends the span.
    pub fn stop(mut self) {
        let mut labels: Vec<(&'static str, &'static str)> = self.tags.clone();
        labels.push(("error", self.error));
        let labels: Vec<metrics::Label> = labels
            .into_iter()
            .map(|(key, value)| metrics::Label::new(key, value))
            .collect();
        metrics::histogram!(format!("{}.seconds", self.name), labels).record(self.started.elapsed().as_secs_f64());
        self.span.end();
    }
}
