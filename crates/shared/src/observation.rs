//! Timers and spans around calls to other systems: one observation per call, recorded as a timer in Micrometer's
//! shape and an OpenTelemetry span with the same tags.
//!
//! Java counterpart: Micrometer's `Observation` with the default meter and tracing handlers, as
//! `S3ObjectStorage` (`ledgerly.s3`, tagged `operation`) and `ClamAvScanner` (`ledgerly.clamav.scan`, tagged
//! `outcome`) use it. `DefaultMeterObservationHandler` makes two meters of an observation:
//!
//! - a long-task timer `<name>.active` with the tags known when the observation starts, which counts the calls
//!   running now;
//! - a timer `<name>` with the tags known when it stops plus `error` (`none`, or the kind of failure), which
//!   counts finished calls, their total time and the longest one of the last two minutes.
//!
//! [`render`] writes both as Micrometer's Prometheus registry does (`_count`, `_sum`, `_max`, and
//! `_active_count`, `_duration_sum`, `_max` for the long-task timer), so that dashboards built on the Java
//! service read the Rust one too.
//!
//! Tags are fixed words: never a key, a file name, an id or an address, so the number of series stays small
//! and nothing personal reaches the metrics.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use opentelemetry::KeyValue;
use opentelemetry::global::{self, BoxedSpan};
use opentelemetry::trace::{Span, Status, Tracer};

/// A meter: its name and its tags, sorted by key as Micrometer sorts them.
type Series = (&'static str, Vec<(&'static str, &'static str)>);

/// Micrometer's `DistributionStatisticConfig` defaults for the rolling maximum: two minutes, in three buffers.
const MAX_EXPIRY: Duration = Duration::from_secs(120);
const MAX_BUFFERS: usize = 3;

/// Every timer and long-task timer of the process.
#[derive(Debug, Default)]
struct Registry {
    timers: BTreeMap<Series, Timer>,
    /// The start of every call still running, per long-task timer. A long-task timer stays once registered.
    active: BTreeMap<Series, BTreeMap<u64, Instant>>,
    next_task: u64,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Micrometer's `Timer`: count, total time and `TimeWindowMax`.
#[derive(Debug)]
struct Timer {
    count: u64,
    total: Duration,
    max: WindowMax,
}

/// Micrometer's `TimeWindowMax`: every value goes into all buffers; every `expiry / buffers` the current buffer
/// is cleared and the next one becomes current. The maximum reported is the current buffer's, so a value counts
/// for between 80 and 120 seconds.
#[derive(Debug)]
struct WindowMax {
    buffers: [Duration; MAX_BUFFERS],
    current: usize,
    last_rotation: Instant,
}

impl WindowMax {
    fn new(now: Instant) -> Self {
        Self {
            buffers: [Duration::ZERO; MAX_BUFFERS],
            current: 0,
            last_rotation: now,
        }
    }

    fn rotate(&mut self, now: Instant) {
        let step = MAX_EXPIRY / u32::try_from(MAX_BUFFERS).unwrap_or(1);
        let mut rotations = 0;
        while now.duration_since(self.last_rotation) >= step {
            self.last_rotation += step;
            if rotations < MAX_BUFFERS {
                if let Some(buffer) = self.buffers.get_mut(self.current) {
                    *buffer = Duration::ZERO;
                }
                self.current = (self.current + 1) % MAX_BUFFERS;
            }
            rotations += 1;
        }
    }

    fn record(&mut self, value: Duration, now: Instant) {
        self.rotate(now);
        for buffer in &mut self.buffers {
            *buffer = (*buffer).max(value);
        }
    }

    fn poll(&mut self, now: Instant) -> Duration {
        self.rotate(now);
        self.buffers.get(self.current).copied().unwrap_or_default()
    }
}

/// One timed call. Stop it with [`Observation::stop`]; one dropped unstopped leaves the active calls and
/// records nothing else.
pub struct Observation {
    name: &'static str,
    started: Instant,
    span: BoxedSpan,
    tags: Vec<(&'static str, &'static str)>,
    error: &'static str,
    /// The long-task timer and this call's entry in it, set by the first tag-free use of the observation.
    task: Option<(Series, u64)>,
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
    /// OpenTelemetry context. `tags` are the ones known at the start; they go on the long-task timer too.
    #[must_use]
    pub fn start(name: &'static str, contextual_name: &'static str, tags: &[(&'static str, &'static str)]) -> Self {
        let mut span = global::tracer("ledgerly").start(contextual_name);
        for (key, value) in tags {
            span.set_attribute(KeyValue::new(*key, *value));
        }
        let started = Instant::now();
        let series: Series = (name, sorted(tags.to_vec()));
        let task = {
            let mut registry = registry();
            registry.next_task += 1;
            let id = registry.next_task;
            registry.active.entry(series.clone()).or_default().insert(id, started);
            (series, id)
        };
        Self {
            name,
            started,
            span,
            tags: tags.to_vec(),
            error: "none",
            task: Some(task),
        }
    }

    /// Adds a low-cardinality tag found out during the call, for the timer and the span.
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
        let now = Instant::now();
        let elapsed = now.duration_since(self.started);
        let mut tags = self.tags.clone();
        tags.push(("error", self.error));
        {
            let mut registry = registry();
            let timer = registry
                .timers
                .entry((self.name, sorted(tags)))
                .or_insert_with(|| Timer {
                    count: 0,
                    total: Duration::ZERO,
                    max: WindowMax::new(now),
                });
            timer.count += 1;
            timer.total += elapsed;
            timer.max.record(elapsed, now);
        }
        self.span.end();
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        if let Some((series, id)) = self.task.take()
            && let Some(tasks) = registry().active.get_mut(&series)
        {
            tasks.remove(&id);
        }
    }
}

fn sorted(mut tags: Vec<(&'static str, &'static str)>) -> Vec<(&'static str, &'static str)> {
    tags.sort_by_key(|(key, _)| *key);
    tags
}

/// The Prometheus name of a meter: Micrometer's, with dots as underscores.
fn prometheus(name: &str) -> String {
    name.replace('.', "_")
}

fn labels(tags: &[(&'static str, &'static str)]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = tags
        .iter()
        .map(|(key, value)| format!("{key}=\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("{{{}}}", inner.join(","))
}

/// Every timer and long-task timer in the Prometheus text format, as Micrometer's registry writes them: per
/// timer a summary `<name>_seconds` (`_count`, `_sum`) and a gauge `<name>_seconds_max`; per long-task timer a
/// summary `<name>_active_seconds` (`_active_count`, `_duration_sum`) and a gauge `<name>_active_seconds_max`.
#[must_use]
pub fn render() -> String {
    let now = Instant::now();
    let mut registry = registry();
    let mut text = String::new();
    let mut by_name: BTreeMap<&'static str, Vec<(String, u64, f64, f64)>> = BTreeMap::new();
    for ((name, tags), timer) in &mut registry.timers {
        let max = timer.max.poll(now).as_secs_f64();
        by_name
            .entry(name)
            .or_default()
            .push((labels(tags), timer.count, timer.total.as_secs_f64(), max));
    }
    for (name, series) in by_name {
        let base = format!("{}_seconds", prometheus(name));
        let _ = writeln!(text, "# HELP {base}  \n# TYPE {base} summary");
        for (labels, count, sum, _) in &series {
            let _ = writeln!(text, "{base}_count{labels} {count}\n{base}_sum{labels} {sum}");
        }
        let _ = writeln!(text, "# HELP {base}_max  \n# TYPE {base}_max gauge");
        for (labels, _, _, max) in &series {
            let _ = writeln!(text, "{base}_max{labels} {max}");
        }
    }
    let mut active: BTreeMap<&'static str, Vec<(String, usize, f64, f64)>> = BTreeMap::new();
    for ((name, tags), tasks) in &registry.active {
        let running: Vec<f64> = tasks
            .values()
            .map(|started| now.duration_since(*started).as_secs_f64())
            .collect();
        let longest = running.iter().copied().fold(0.0, f64::max);
        active
            .entry(name)
            .or_default()
            .push((labels(tags), running.len(), running.iter().sum(), longest));
    }
    for (name, series) in active {
        let base = format!("{}_active_seconds", prometheus(name));
        let _ = writeln!(text, "# HELP {base}  \n# TYPE {base} summary");
        for (labels, count, sum, _) in &series {
            let _ = writeln!(
                text,
                "{base}_active_count{labels} {count}\n{base}_duration_sum{labels} {sum}"
            );
        }
        let _ = writeln!(text, "# HELP {base}_max  \n# TYPE {base}_max gauge");
        for (labels, _, _, max) in &series {
            let _ = writeln!(text, "{base}_max{labels} {max}");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line<'a>(text: &'a str, start: &str) -> &'a str {
        text.lines()
            .find(|line| line.starts_with(start))
            .unwrap_or_else(|| panic!("no line starting with {start} in\n{text}"))
    }

    #[test]
    fn a_finished_call_is_a_timer_with_its_tags_and_error() {
        let mut observation = Observation::start("test.finished", "test finished", &[("operation", "put")]);
        observation.tag("outcome", "clean");
        observation.stop();
        let mut failed = Observation::start("test.finished", "test finished", &[("operation", "put")]);
        failed.error("IOException");
        failed.stop();

        let text = render();

        assert_eq!(
            line(&text, "test_finished_seconds_count{error=\"none\""),
            "test_finished_seconds_count{error=\"none\",operation=\"put\",outcome=\"clean\"} 1"
        );
        assert!(text.contains("test_finished_seconds_count{error=\"IOException\",operation=\"put\"} 1"));
        assert!(text.contains("# TYPE test_finished_seconds summary"));
        assert!(text.contains("# TYPE test_finished_seconds_max gauge"));
        assert!(line(&text, "test_finished_seconds_max{error=\"none\"").ends_with(|c: char| c.is_ascii_digit()));
        // The call is over: the long-task timer stays, at zero.
        assert_eq!(
            line(&text, "test_finished_active_seconds_active_count"),
            "test_finished_active_seconds_active_count{operation=\"put\"} 0"
        );
    }

    #[test]
    fn a_running_call_is_counted_as_active() {
        let running = Observation::start("test.running", "test running", &[]);

        let text = render();

        assert_eq!(
            line(&text, "test_running_active_seconds_active_count"),
            "test_running_active_seconds_active_count 1"
        );
        drop(running);
        assert_eq!(
            line(&render(), "test_running_active_seconds_active_count"),
            "test_running_active_seconds_active_count 0"
        );
        // Dropped without `stop`: no timer.
        assert!(!render().contains("test_running_seconds_count"));
    }

    #[test]
    fn the_maximum_is_kept_for_two_minutes_at_most() {
        let start = Instant::now();
        let mut max = WindowMax::new(start);
        max.record(Duration::from_secs(5), start);

        assert_eq!(max.poll(start + Duration::from_secs(79)), Duration::from_secs(5));
        assert_eq!(max.poll(start + Duration::from_secs(121)), Duration::ZERO);
        max.record(Duration::from_secs(1), start + Duration::from_secs(130));
        assert_eq!(max.poll(start + Duration::from_secs(131)), Duration::from_secs(1));
        // Far later, every buffer has been cleared.
        assert_eq!(max.poll(start + Duration::from_secs(3600)), Duration::ZERO);
    }
}
