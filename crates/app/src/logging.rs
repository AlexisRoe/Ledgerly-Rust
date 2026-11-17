//! Log output: ECS JSON on stdout, or human-readable lines in the `local` profile.
//!
//! Java counterpart: `logging.structured.format.console: ecs` in `application.yaml` (emptied by
//! `application-local.yaml`). The JSON has the members Spring Boot's
//! `ElasticCommonSchemaStructuredLogFormatter` writes, in its order and nested as it nests them, because log
//! pipelines built for the Java service parse them (ADR 0013).

use std::io::Write;

use chrono::{SecondsFormat, Utc};
use ledgerly_shared::config::Profiles;
use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::{SubscriberInitExt, TryInitError};

/// `spring.application.name`, written as `service.name`.
const SERVICE_NAME: &str = "ledgerly";
/// The ECS version Spring Boot 4.2 declares.
const ECS_VERSION: &str = "8.11";

/// Installs the process-wide log output: readable lines with `local`, ECS JSON otherwise. Like Spring's root
/// logger, it shows `INFO` and above.
///
/// # Errors
///
/// Returns an error if log output was installed before.
pub fn init(profiles: Profiles) -> Result<(), TryInitError> {
    let registry = tracing_subscriber::registry().with(LevelFilter::INFO);
    if profiles.local {
        registry.with(tracing_subscriber::fmt::layer()).try_init()
    } else {
        registry.with(EcsLayer::new(std::io::stdout)).try_init()
    }
}

/// Writes every event as one line of ECS JSON.
///
/// The fields of the spans an event happens in play the part of Java's MDC: `traceId` and `spanId` of the
/// request, later `userId` and `teamId`. They and the event's own fields follow `message`. A name with dots
/// becomes nested objects, as in Spring.
#[derive(Debug)]
pub struct EcsLayer<W> {
    writer: W,
    pid: u32,
    version: &'static str,
}

impl<W> EcsLayer<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// The fields recorded on a span, kept with the span for the events inside it.
#[derive(Debug, Default)]
struct Fields(Vec<(&'static str, Value)>);

impl Fields {
    fn set(&mut self, name: &'static str, value: Value) {
        match self.0.iter_mut().find(|(known, _)| *known == name) {
            Some((_, slot)) => *slot = value,
            None => self.0.push((name, value)),
        }
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.set(field.name(), Value::from(format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.set(field.name(), Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.set(field.name(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.set(field.name(), Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.set(field.name(), Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.set(field.name(), Value::from(value));
    }
}

/// Puts `value` at `name`, turning `a.b.c` into nested objects. A name that collides with a value already
/// there replaces it.
fn insert_nested(target: &mut Map<String, Value>, name: &str, value: Value) {
    match name.split_once('.') {
        None => {
            target.insert(name.to_owned(), value);
        }
        Some((head, rest)) => {
            let slot = target
                .entry(head.to_owned())
                .or_insert_with(|| Value::Object(Map::new()));
            if !slot.is_object() {
                *slot = Value::Object(Map::new());
            }
            if let Value::Object(inner) = slot {
                insert_nested(inner, rest, value);
            }
        }
    }
}

impl<S, W> Layer<S> for EcsLayer<W>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        if let Some(span) = context.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, context: Context<'_, S>) {
        if let Some(span) = context.span(id)
            && let Some(fields) = span.extensions_mut().get_mut::<Fields>()
        {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        let mut own = Fields::default();
        event.record(&mut own);
        let mut message = Value::from("");
        let mut pairs = Map::new();
        // Outermost span first, so that an inner span and then the event itself win on equal names.
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                if let Some(fields) = span.extensions().get::<Fields>() {
                    for (name, value) in &fields.0 {
                        insert_nested(&mut pairs, name, value.clone());
                    }
                }
            }
        }
        for (name, value) in own.0 {
            if name == "message" {
                message = value;
            } else {
                insert_nested(&mut pairs, name, value);
            }
        }

        let metadata = event.metadata();
        let thread = std::thread::current();
        let mut line = format!(
            "{{\"@timestamp\":{},\"log\":{{\"level\":{},\"logger\":{}}},\
             \"process\":{{\"pid\":{},\"thread\":{{\"name\":{}}}}},\
             \"service\":{{\"name\":{},\"version\":{}}},\"message\":{}",
            Value::from(Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true)),
            Value::from(metadata.level().as_str()),
            Value::from(metadata.target()),
            self.pid,
            Value::from(thread.name().unwrap_or("unnamed")),
            Value::from(SERVICE_NAME),
            Value::from(self.version),
            message,
        );
        for (name, value) in &pairs {
            line.push_str(&format!(",{}:{value}", Value::from(name.as_str())));
        }
        line.push_str(&format!(",\"ecs\":{{\"version\":\"{ECS_VERSION}\"}}}}\n"));
        // A log line that cannot be written has nowhere to be reported.
        let _ = self.writer.make_writer().write_all(line.as_bytes());
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Collects log output in memory.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        /// Every line written so far, parsed.
        pub(crate) fn lines(&self) -> Vec<Value> {
            let bytes = self.0.lock().unwrap().clone();
            String::from_utf8(bytes)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }

        pub(crate) fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }

        /// A subscriber that writes ECS JSON into this buffer.
        pub(crate) fn subscriber(&self) -> impl Subscriber + Send + Sync + use<> {
            tracing_subscriber::registry()
                .with(LevelFilter::INFO)
                .with(EcsLayer::new(self.clone()))
        }
    }

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for Captured {
        type Writer = Self;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Captured;
    use super::*;

    fn keys(value: &Value) -> Vec<&str> {
        value.as_object().unwrap().keys().map(String::as_str).collect()
    }

    fn capture(log: impl FnOnce()) -> Captured {
        let captured = Captured::default();
        tracing::subscriber::with_default(captured.subscriber(), log);
        captured
    }

    /// The member list of Spring Boot 4.2's `ElasticCommonSchemaStructuredLogFormatter.jsonMembers` for an
    /// event without MDC, exception or markers.
    #[test]
    fn a_log_line_has_exactly_spring_boots_ecs_members() {
        let captured = capture(|| tracing::info!("Started {} in {} ms", "Ledgerly", 12));

        let lines = captured.lines();
        let [line] = lines.as_slice() else {
            panic!("expected one line, got {lines:?}")
        };
        assert_eq!(
            keys(line),
            ["@timestamp", "ecs", "log", "message", "process", "service"]
        );
        assert_eq!(keys(&line["log"]), ["level", "logger"]);
        assert_eq!(keys(&line["process"]), ["pid", "thread"]);
        assert_eq!(keys(&line["process"]["thread"]), ["name"]);
        assert_eq!(keys(&line["service"]), ["name", "version"]);
        assert_eq!(keys(&line["ecs"]), ["version"]);

        assert_eq!(line["message"], "Started Ledgerly in 12 ms");
        assert_eq!(line["log"]["level"], "INFO");
        assert_eq!(line["log"]["logger"], "ledgerly_app::logging::tests");
        assert_eq!(line["process"]["pid"], std::process::id());
        assert!(line["process"]["thread"]["name"].is_string());
        assert_eq!(line["service"]["name"], "ledgerly");
        assert_eq!(line["service"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(line["ecs"]["version"], "8.11");
    }

    #[test]
    fn the_members_come_in_spring_boots_order() {
        let captured = capture(|| tracing::info_span!("request", traceId = "abc").in_scope(|| tracing::info!("hello")));

        let text = captured.text();
        let position = |member: &str| text.find(member).unwrap_or_else(|| panic!("{member} is not in {text}"));
        let order = [
            "\"@timestamp\"",
            "\"log\"",
            "\"process\"",
            "\"service\"",
            "\"message\"",
            "\"traceId\"",
            "\"ecs\"",
        ]
        .map(position);
        assert!(order.is_sorted(), "{text}");
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn the_timestamp_is_an_instant_in_utc_as_java_prints_it() {
        let captured = capture(|| tracing::info!("now"));

        let lines = captured.lines();
        let timestamp = lines[0]["@timestamp"].as_str().unwrap();
        assert!(timestamp.ends_with('Z'), "{timestamp}");
        let parsed = chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
        assert!((Utc::now() - parsed.to_utc()).num_seconds().abs() < 5);
    }

    #[test]
    fn levels_are_written_as_java_writes_them_and_debug_is_off() {
        let captured = capture(|| {
            tracing::trace!("t");
            tracing::debug!("d");
            tracing::info!("i");
            tracing::warn!("w");
            tracing::error!("e");
        });

        let levels: Vec<Value> = captured
            .lines()
            .iter()
            .map(|line| line["log"]["level"].clone())
            .collect();
        assert_eq!(levels, ["INFO", "WARN", "ERROR"]);
    }

    #[test]
    fn span_fields_appear_like_javas_mdc_and_inner_spans_win() {
        let captured = capture(|| {
            let request = tracing::info_span!(
                "request",
                traceId = "0af7651916cd43dd8448eb211c80319c",
                spanId = "b7ad6b7169203331",
                userId = tracing::field::Empty,
            );
            let _in_request = request.enter();
            tracing::info!("before sign-in");
            request.record("userId", "0196f3a2-0000-7000-8000-000000000001");
            tracing::info_span!("inner", spanId = "00f067aa0ba902b7").in_scope(|| tracing::info!("after sign-in"));
        });

        let lines = captured.lines();
        assert_eq!(
            keys(&lines[0]),
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
        assert_eq!(lines[0]["traceId"], "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(lines[0]["spanId"], "b7ad6b7169203331");
        assert_eq!(lines[1]["userId"], "0196f3a2-0000-7000-8000-000000000001");
        assert_eq!(lines[1]["traceId"], "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(lines[1]["spanId"], "00f067aa0ba902b7");
    }

    #[test]
    fn event_fields_follow_the_message_and_dotted_names_nest() {
        let captured = capture(|| {
            tracing::warn!(
                receiptId = "r-1",
                attempts = 3,
                error.r#type = "ScanFailed",
                error.message = "timeout",
                "Scan failed"
            );
        });

        let lines = captured.lines();
        assert_eq!(lines[0]["message"], "Scan failed");
        assert_eq!(lines[0]["receiptId"], "r-1");
        assert_eq!(lines[0]["attempts"], 3);
        assert_eq!(
            lines[0]["error"],
            serde_json::json!({"type": "ScanFailed", "message": "timeout"})
        );
    }

    #[test]
    fn text_is_escaped_so_that_one_event_is_one_line() {
        let captured = capture(|| tracing::info!("line one\nline \"two\"\t{}", '\u{1}'));

        assert_eq!(captured.text().lines().count(), 1);
        assert_eq!(captured.lines()[0]["message"], "line one\nline \"two\"\t\u{1}");
    }
}
