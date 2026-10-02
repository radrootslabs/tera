//! Closed diagnostics at both native logging sinks. Raw event messages,
//! span fields, targets, identifiers and paths are never formatted.

use std::fmt;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};

pub(super) struct DiagnosticFormat;

#[derive(Default)]
struct DiagnosticFields {
    code: Option<&'static str>,
    counts: Vec<(&'static str, u64)>,
}

impl Visit for DiagnosticFields {
    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "code" {
            self.code = match value {
                "message_redacted" => Some("message_redacted"),
                "runtime_started" => Some("runtime_started"),
                "runtime_stopped" => Some("runtime_stopped"),
                "operation_failed" => Some("operation_failed"),
                _ => None,
            };
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let key = match field.name() {
            "count" => "count",
            "attempts" => "attempts",
            "schema" => "schema",
            "latency_ms" => "latency_ms",
            _ => return,
        };
        if value <= 1_000_000 && self.counts.len() < 4 {
            self.counts.push((key, value));
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if let Ok(value) = u64::try_from(value) {
            self.record_u64(field, value);
        }
    }
}

impl<S, N> FormatEvent<S, N> for DiagnosticFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut fields = DiagnosticFields::default();
        event.record(&mut fields);
        write!(
            writer,
            "level={} code={}",
            event.metadata().level(),
            fields.code.unwrap_or("message_redacted")
        )?;
        for (key, value) in fields.counts {
            write!(writer, " {key}={value}")?;
        }
        writeln!(writer)
    }
}
