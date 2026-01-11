//! Unified JSON logging with custom format.
//!
//! Log format:
//! ```json
//! {"ts":"2024-12-28T15:04:05.123Z","level":"info","type":"app","msg":"Server started","ctx":{},"data":{}}
//! ```

use serde::Serialize;
use std::collections::HashMap;
use std::io::{self, Write};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// Log entry with unified structure.
#[derive(Serialize)]
pub struct LogEntry<'a> {
    /// ISO 8601 timestamp with milliseconds, UTC
    pub ts: &'a str,
    /// Log level: debug, info, warn, error
    pub level: &'a str,
    /// Log type: app, access, error
    #[serde(rename = "type")]
    pub log_type: &'a str,
    /// Short human-readable message
    pub msg: &'a str,
    /// Context: request_id, service, etc.
    pub ctx: LogContext<'a>,
    /// Type-specific data
    pub data: HashMap<&'a str, serde_json::Value>,
}

/// Log context.
#[derive(Serialize, Default)]
pub struct LogContext<'a> {
    /// Service name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<&'a str>,
    /// Request ID for correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<&'a str>,
    /// Worker ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<u32>,
}

/// Custom JSON formatter for tracing.
pub struct JsonFormatter {
    service_name: String,
}

impl JsonFormatter {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }
}

impl<S, N> FormatEvent<S, N> for JsonFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let level = match *meta.level() {
            Level::TRACE => "debug",
            Level::DEBUG => "debug",
            Level::INFO => "info",
            Level::WARN => "warn",
            Level::ERROR => "error",
        };

        // Determine log type from target
        let log_type = if meta.target() == "access" {
            "access"
        } else if *meta.level() == Level::ERROR {
            "error"
        } else {
            "app"
        };

        // Collect fields
        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);

        // Generate timestamp
        let ts = crate::server::connection::chrono_lite_iso8601();

        // Build message
        let msg = if log_type == "access" {
            // For access logs, build "METHOD /path STATUS"
            let method = visitor
                .fields
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let path = visitor
                .fields
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let status = visitor
                .fields
                .get("status")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!("{} {} {}", method, path, status)
        } else {
            visitor.message.clone().unwrap_or_default()
        };

        // Build context
        let ctx = serde_json::json!({
            "service": &self.service_name
        });

        // Build data (remove message from fields for app logs)
        let mut data = visitor.fields;
        if log_type != "access" {
            data.remove("message");
        }

        // Build final JSON
        let entry = serde_json::json!({
            "ts": ts,
            "level": level,
            "type": log_type,
            "msg": msg,
            "ctx": ctx,
            "data": data,
        });

        writeln!(
            writer,
            "{}",
            serde_json::to_string(&entry).unwrap_or_default()
        )
    }
}

/// Field visitor for collecting tracing fields.
struct FieldVisitor {
    message: Option<String>,
    fields: HashMap<String, serde_json::Value>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            fields: HashMap::new(),
        }
    }
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value).trim_matches('"').to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(format!("{:?}", value)),
            );
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }
}

/// Log an access request directly (bypassing tracing for simpler output).
#[allow(clippy::too_many_arguments)]
pub fn log_access(
    ts: &str,
    request_id: &str,
    ip: &str,
    method: &str,
    path: &str,
    query: Option<&str>,
    http: &str,
    status: u16,
    bytes: u64,
    duration_ms: f64,
    ua: Option<&str>,
    referer: Option<&str>,
    xff: Option<&str>,
    tls: Option<&str>,
    trace_id: Option<&str>,
    span_id: Option<&str>,
) {
    let msg = format!("{} {} {}", method, path, status);

    let mut data = serde_json::Map::new();
    data.insert("method".into(), serde_json::json!(method));
    data.insert("path".into(), serde_json::json!(path));
    if let Some(q) = query {
        data.insert("query".into(), serde_json::json!(q));
    }
    data.insert("http".into(), serde_json::json!(http));
    data.insert("status".into(), serde_json::json!(status));
    data.insert("bytes".into(), serde_json::json!(bytes));
    data.insert("duration_ms".into(), serde_json::json!(duration_ms));
    data.insert("ip".into(), serde_json::json!(ip));
    if let Some(u) = ua {
        data.insert("ua".into(), serde_json::json!(u));
    }
    if let Some(r) = referer {
        data.insert("referer".into(), serde_json::json!(r));
    }
    if let Some(x) = xff {
        data.insert("xff".into(), serde_json::json!(x));
    }
    if let Some(t) = tls {
        data.insert("tls".into(), serde_json::json!(t));
    }

    // Build context with trace information
    let mut ctx = serde_json::Map::new();
    ctx.insert("service".into(), serde_json::json!("tokio_php"));
    ctx.insert("request_id".into(), serde_json::json!(request_id));
    if let Some(tid) = trace_id {
        ctx.insert("trace_id".into(), serde_json::json!(tid));
    }
    if let Some(sid) = span_id {
        ctx.insert("span_id".into(), serde_json::json!(sid));
    }

    let entry = serde_json::json!({
        "ts": ts,
        "level": "info",
        "type": "access",
        "msg": msg,
        "ctx": ctx,
        "data": data,
    });

    // Write directly to stdout
    let _ = writeln!(io::stdout(), "{}", entry);
}
