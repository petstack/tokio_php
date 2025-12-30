//! Request context for middleware pipeline.

use std::any::Any;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;

/// Request context shared across middleware and handlers.
///
/// Context carries request-scoped data through the middleware pipeline:
/// - Client information (IP, trace IDs)
/// - Timing information
/// - Response headers to add
/// - Custom key-value storage for middleware communication
pub struct Context {
    /// Client IP address.
    pub client_ip: IpAddr,

    /// W3C Trace ID (32 hex chars).
    pub trace_id: String,

    /// Span ID (16 hex chars).
    pub span_id: String,

    /// Parent span ID (if propagated from upstream).
    pub parent_span_id: Option<String>,

    /// Short request ID for logging.
    pub request_id: String,

    /// Request start time.
    pub started_at: Instant,

    /// HTTP version string (for logging).
    pub http_version: String,

    /// Whether this request is being profiled.
    pub profiling: bool,

    /// Whether client accepts HTML responses.
    pub accepts_html: bool,

    /// Whether client accepts Brotli compression.
    pub accepts_brotli: bool,

    /// Response headers to add.
    response_headers: HashMap<String, String>,

    /// Custom key-value storage for middleware.
    values: HashMap<String, Box<dyn Any + Send + Sync>>,
}

impl Context {
    /// Create a new context with minimal information.
    pub fn new(client_ip: IpAddr, trace_id: String, span_id: String) -> Self {
        let request_id = format!(
            "{}-{}",
            &trace_id[..12.min(trace_id.len())],
            &span_id[..4.min(span_id.len())]
        );

        Self {
            client_ip,
            trace_id,
            span_id,
            parent_span_id: None,
            request_id,
            started_at: Instant::now(),
            http_version: "HTTP/1.1".to_string(),
            profiling: false,
            accepts_html: false,
            accepts_brotli: false,
            response_headers: HashMap::new(),
            values: HashMap::new(),
        }
    }

    /// Create a context builder for more control.
    pub fn builder(client_ip: IpAddr) -> ContextBuilder {
        ContextBuilder::new(client_ip)
    }

    /// Set a custom value.
    pub fn set<T: Send + Sync + 'static>(&mut self, key: &str, value: T) {
        self.values.insert(key.to_string(), Box::new(value));
    }

    /// Get a custom value.
    pub fn get<T: 'static>(&self, key: &str) -> Option<&T> {
        self.values.get(key).and_then(|v| v.downcast_ref())
    }

    /// Get a mutable reference to a custom value.
    pub fn get_mut<T: 'static>(&mut self, key: &str) -> Option<&mut T> {
        self.values.get_mut(key).and_then(|v| v.downcast_mut())
    }

    /// Remove a custom value.
    pub fn remove<T: 'static>(&mut self, key: &str) -> Option<T> {
        self.values
            .remove(key)
            .and_then(|v| v.downcast().ok())
            .map(|b| *b)
    }

    /// Add a response header.
    pub fn set_response_header(&mut self, name: impl ToString, value: impl ToString) {
        self.response_headers
            .insert(name.to_string(), value.to_string());
    }

    /// Get all response headers to add.
    pub fn response_headers(&self) -> &HashMap<String, String> {
        &self.response_headers
    }

    /// Get elapsed time since request started.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Get elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed().as_secs_f64() * 1000.0
    }

    /// Get elapsed time in microseconds.
    pub fn elapsed_us(&self) -> u64 {
        self.elapsed().as_micros() as u64
    }
}

/// Builder for creating Context with more control.
pub struct ContextBuilder {
    client_ip: IpAddr,
    trace_id: Option<String>,
    span_id: Option<String>,
    parent_span_id: Option<String>,
    http_version: String,
    profiling: bool,
    accepts_html: bool,
    accepts_brotli: bool,
}

impl ContextBuilder {
    /// Create a new context builder.
    pub fn new(client_ip: IpAddr) -> Self {
        Self {
            client_ip,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            http_version: "HTTP/1.1".to_string(),
            profiling: false,
            accepts_html: false,
            accepts_brotli: false,
        }
    }

    /// Set the trace ID.
    pub fn trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Set the span ID.
    pub fn span_id(mut self, span_id: impl Into<String>) -> Self {
        self.span_id = Some(span_id.into());
        self
    }

    /// Set the parent span ID.
    pub fn parent_span_id(mut self, parent_span_id: impl Into<String>) -> Self {
        self.parent_span_id = Some(parent_span_id.into());
        self
    }

    /// Set the HTTP version.
    pub fn http_version(mut self, version: impl Into<String>) -> Self {
        self.http_version = version.into();
        self
    }

    /// Enable profiling.
    pub fn profiling(mut self, enabled: bool) -> Self {
        self.profiling = enabled;
        self
    }

    /// Set whether client accepts HTML.
    pub fn accepts_html(mut self, accepts: bool) -> Self {
        self.accepts_html = accepts;
        self
    }

    /// Set whether client accepts Brotli.
    pub fn accepts_brotli(mut self, accepts: bool) -> Self {
        self.accepts_brotli = accepts;
        self
    }

    /// Build the context.
    pub fn build(self) -> Context {
        let trace_id = self.trace_id.unwrap_or_else(generate_trace_id);
        let span_id = self.span_id.unwrap_or_else(generate_span_id);
        let request_id = format!(
            "{}-{}",
            &trace_id[..12.min(trace_id.len())],
            &span_id[..4.min(span_id.len())]
        );

        Context {
            client_ip: self.client_ip,
            trace_id,
            span_id,
            parent_span_id: self.parent_span_id,
            request_id,
            started_at: Instant::now(),
            http_version: self.http_version,
            profiling: self.profiling,
            accepts_html: self.accepts_html,
            accepts_brotli: self.accepts_brotli,
            response_headers: HashMap::new(),
            values: HashMap::new(),
        }
    }
}

/// Generate a random trace ID (32 hex chars).
fn generate_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let random: u64 = rand_u64();

    format!("{:016x}{:016x}", timestamp, random)
}

/// Generate a random span ID (16 hex chars).
fn generate_span_id() -> String {
    format!("{:016x}", rand_u64())
}

/// Simple random u64 without external dependency.
fn rand_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_context_new() {
        let ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "0af7651916cd43dd8448eb211c80319c".to_string(),
            "b7ad6b7169203331".to_string(),
        );

        assert_eq!(ctx.client_ip.to_string(), "127.0.0.1");
        assert_eq!(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(ctx.span_id, "b7ad6b7169203331");
        assert_eq!(ctx.request_id, "0af7651916cd-b7ad");
        assert!(!ctx.profiling);
    }

    #[test]
    fn test_context_builder() {
        let ctx = Context::builder(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
            .trace_id("abc123def456")
            .span_id("span1234")
            .http_version("HTTP/2.0")
            .profiling(true)
            .accepts_html(true)
            .accepts_brotli(true)
            .build();

        assert_eq!(ctx.client_ip.to_string(), "10.0.0.1");
        assert_eq!(ctx.trace_id, "abc123def456");
        assert_eq!(ctx.span_id, "span1234");
        assert_eq!(ctx.http_version, "HTTP/2.0");
        assert!(ctx.profiling);
        assert!(ctx.accepts_html);
        assert!(ctx.accepts_brotli);
    }

    #[test]
    fn test_context_custom_values() {
        let mut ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        );

        ctx.set("counter", 42u32);
        ctx.set("name", "test".to_string());

        assert_eq!(ctx.get::<u32>("counter"), Some(&42));
        assert_eq!(ctx.get::<String>("name"), Some(&"test".to_string()));
        assert_eq!(ctx.get::<u32>("missing"), None);

        // Mutate
        if let Some(counter) = ctx.get_mut::<u32>("counter") {
            *counter += 1;
        }
        assert_eq!(ctx.get::<u32>("counter"), Some(&43));

        // Remove
        let removed = ctx.remove::<u32>("counter");
        assert_eq!(removed, Some(43));
        assert_eq!(ctx.get::<u32>("counter"), None);
    }

    #[test]
    fn test_context_response_headers() {
        let mut ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        );

        ctx.set_response_header("X-Custom", "value1");
        ctx.set_response_header("X-Another", "value2");

        let headers = ctx.response_headers();
        assert_eq!(headers.get("X-Custom"), Some(&"value1".to_string()));
        assert_eq!(headers.get("X-Another"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_context_elapsed() {
        let ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        );

        std::thread::sleep(std::time::Duration::from_millis(10));

        assert!(ctx.elapsed_ms() >= 10.0);
        assert!(ctx.elapsed_us() >= 10000);
    }

    #[test]
    fn test_generate_trace_id() {
        let id1 = generate_trace_id();
        let id2 = generate_trace_id();

        assert_eq!(id1.len(), 32);
        assert_eq!(id2.len(), 32);
        // IDs should be different (with very high probability)
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_generate_span_id() {
        let id1 = generate_span_id();
        let id2 = generate_span_id();

        assert_eq!(id1.len(), 16);
        assert_eq!(id2.len(), 16);
    }
}
