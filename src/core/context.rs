//! Request context for middleware pipeline.

use std::any::Any;
use std::cell::Cell;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;

/// HTTP version as static string (no allocation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpVersion(&'static str);

impl HttpVersion {
    pub const HTTP_10: Self = Self("HTTP/1.0");
    pub const HTTP_11: Self = Self("HTTP/1.1");
    pub const HTTP_20: Self = Self("HTTP/2.0");

    /// Get the version string.
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }

    /// Create from http::Version.
    #[inline]
    pub fn from_http(version: http::Version) -> Self {
        match version {
            http::Version::HTTP_10 => Self::HTTP_10,
            http::Version::HTTP_11 => Self::HTTP_11,
            http::Version::HTTP_2 => Self::HTTP_20,
            _ => Self::HTTP_11, // fallback
        }
    }
}

impl std::fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Default for HttpVersion {
    fn default() -> Self {
        Self::HTTP_11
    }
}

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

    /// HTTP version (no allocation, Copy).
    pub http_version: HttpVersion,

    /// Whether this request is being profiled.
    pub profiling: bool,

    /// Whether client accepts HTML responses.
    pub accepts_html: bool,

    /// Whether client accepts Brotli compression.
    pub accepts_brotli: bool,

    /// Response headers to add (pre-sized for typical usage).
    response_headers: HashMap<String, String>,

    /// Custom key-value storage for middleware.
    values: HashMap<String, Box<dyn Any + Send + Sync>>,
}

impl Context {
    /// Create a new context with minimal information.
    #[inline]
    pub fn new(client_ip: IpAddr, trace_id: String, span_id: String) -> Self {
        let request_id = make_request_id(&trace_id, &span_id);

        Self {
            client_ip,
            trace_id,
            span_id,
            parent_span_id: None,
            request_id,
            started_at: Instant::now(),
            http_version: HttpVersion::HTTP_11,
            profiling: false,
            accepts_html: false,
            accepts_brotli: false,
            response_headers: HashMap::with_capacity(4),
            values: HashMap::new(),
        }
    }

    /// Create a context builder for more control.
    #[inline]
    pub fn builder(client_ip: IpAddr) -> ContextBuilder {
        ContextBuilder::new(client_ip)
    }

    /// Set a custom value.
    #[inline]
    pub fn set<T: Send + Sync + 'static>(&mut self, key: &str, value: T) {
        self.values.insert(key.to_string(), Box::new(value));
    }

    /// Get a custom value.
    #[inline]
    pub fn get<T: 'static>(&self, key: &str) -> Option<&T> {
        self.values.get(key).and_then(|v| v.downcast_ref())
    }

    /// Get a mutable reference to a custom value.
    #[inline]
    pub fn get_mut<T: 'static>(&mut self, key: &str) -> Option<&mut T> {
        self.values.get_mut(key).and_then(|v| v.downcast_mut())
    }

    /// Remove a custom value.
    #[inline]
    pub fn remove<T: 'static>(&mut self, key: &str) -> Option<T> {
        self.values
            .remove(key)
            .and_then(|v| v.downcast().ok())
            .map(|b| *b)
    }

    /// Add a response header.
    #[inline]
    pub fn set_response_header(&mut self, name: impl Into<String>, value: impl ToString) {
        self.response_headers.insert(name.into(), value.to_string());
    }

    /// Get all response headers to add.
    #[inline]
    pub fn response_headers(&self) -> &HashMap<String, String> {
        &self.response_headers
    }

    /// Get elapsed time since request started.
    #[inline]
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Get elapsed time in milliseconds.
    #[inline]
    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed().as_secs_f64() * 1000.0
    }

    /// Get elapsed time in microseconds.
    #[inline]
    pub fn elapsed_us(&self) -> u64 {
        self.elapsed().as_micros() as u64
    }
}

/// Build request ID from trace_id and span_id.
#[inline]
fn make_request_id(trace_id: &str, span_id: &str) -> String {
    let trace_part = &trace_id[..12.min(trace_id.len())];
    let span_part = &span_id[..4.min(span_id.len())];

    let mut id = String::with_capacity(trace_part.len() + 1 + span_part.len());
    id.push_str(trace_part);
    id.push('-');
    id.push_str(span_part);
    id
}

/// Builder for creating Context with more control.
pub struct ContextBuilder {
    client_ip: IpAddr,
    trace_id: Option<String>,
    span_id: Option<String>,
    parent_span_id: Option<String>,
    http_version: HttpVersion,
    profiling: bool,
    accepts_html: bool,
    accepts_brotli: bool,
}

impl ContextBuilder {
    /// Create a new context builder.
    #[inline]
    pub fn new(client_ip: IpAddr) -> Self {
        Self {
            client_ip,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            http_version: HttpVersion::HTTP_11,
            profiling: false,
            accepts_html: false,
            accepts_brotli: false,
        }
    }

    /// Set the trace ID.
    #[inline]
    pub fn trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Set the span ID.
    #[inline]
    pub fn span_id(mut self, span_id: impl Into<String>) -> Self {
        self.span_id = Some(span_id.into());
        self
    }

    /// Set the parent span ID.
    #[inline]
    pub fn parent_span_id(mut self, parent_span_id: impl Into<String>) -> Self {
        self.parent_span_id = Some(parent_span_id.into());
        self
    }

    /// Set the HTTP version.
    #[inline]
    pub fn http_version(mut self, version: HttpVersion) -> Self {
        self.http_version = version;
        self
    }

    /// Enable profiling.
    #[inline]
    pub fn profiling(mut self, enabled: bool) -> Self {
        self.profiling = enabled;
        self
    }

    /// Set whether client accepts HTML.
    #[inline]
    pub fn accepts_html(mut self, accepts: bool) -> Self {
        self.accepts_html = accepts;
        self
    }

    /// Set whether client accepts Brotli.
    #[inline]
    pub fn accepts_brotli(mut self, accepts: bool) -> Self {
        self.accepts_brotli = accepts;
        self
    }

    /// Build the context.
    #[inline]
    pub fn build(self) -> Context {
        let trace_id = self.trace_id.unwrap_or_else(generate_trace_id);
        let span_id = self.span_id.unwrap_or_else(generate_span_id);
        let request_id = make_request_id(&trace_id, &span_id);

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
            response_headers: HashMap::with_capacity(4),
            values: HashMap::new(),
        }
    }
}

// ============================================================================
// Fast random ID generation with thread-local state
// ============================================================================

thread_local! {
    static RNG_STATE: Cell<u64> = Cell::new(init_rng_seed());
}

/// Initialize RNG seed from system entropy.
fn init_rng_seed() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    hasher.finish()
}

/// Fast random u64 using thread-local xorshift64.
#[inline]
fn rand_u64() -> u64 {
    RNG_STATE.with(|state| {
        let mut x = state.get();
        // xorshift64 algorithm
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        state.set(x);
        x
    })
}

/// Generate a random trace ID (32 hex chars).
pub fn generate_trace_id() -> String {
    use std::fmt::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let random = rand_u64();

    let mut id = String::with_capacity(32);
    let _ = write!(id, "{:016x}{:016x}", timestamp, random);
    id
}

/// Generate a random span ID (16 hex chars).
#[inline]
pub fn generate_span_id() -> String {
    use std::fmt::Write;

    let mut id = String::with_capacity(16);
    let _ = write!(id, "{:016x}", rand_u64());
    id
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
        assert_eq!(ctx.http_version, HttpVersion::HTTP_11);
    }

    #[test]
    fn test_context_builder() {
        let ctx = Context::builder(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
            .trace_id("abc123def456")
            .span_id("span1234")
            .http_version(HttpVersion::HTTP_20)
            .profiling(true)
            .accepts_html(true)
            .accepts_brotli(true)
            .build();

        assert_eq!(ctx.client_ip.to_string(), "10.0.0.1");
        assert_eq!(ctx.trace_id, "abc123def456");
        assert_eq!(ctx.span_id, "span1234");
        assert_eq!(ctx.http_version, HttpVersion::HTTP_20);
        assert_eq!(ctx.http_version.as_str(), "HTTP/2.0");
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

    #[test]
    fn test_http_version() {
        assert_eq!(HttpVersion::HTTP_10.as_str(), "HTTP/1.0");
        assert_eq!(HttpVersion::HTTP_11.as_str(), "HTTP/1.1");
        assert_eq!(HttpVersion::HTTP_20.as_str(), "HTTP/2.0");

        assert_eq!(HttpVersion::from_http(http::Version::HTTP_10), HttpVersion::HTTP_10);
        assert_eq!(HttpVersion::from_http(http::Version::HTTP_11), HttpVersion::HTTP_11);
        assert_eq!(HttpVersion::from_http(http::Version::HTTP_2), HttpVersion::HTTP_20);

        // Display
        assert_eq!(format!("{}", HttpVersion::HTTP_20), "HTTP/2.0");
    }

    #[test]
    fn test_http_version_is_copy() {
        let v1 = HttpVersion::HTTP_20;
        let v2 = v1; // Copy
        assert_eq!(v1, v2);
        assert_eq!(v1.as_str(), v2.as_str());
    }

    #[test]
    fn test_make_request_id() {
        let id = make_request_id("0af7651916cd43dd8448eb211c80319c", "b7ad6b7169203331");
        assert_eq!(id, "0af7651916cd-b7ad");

        // Short inputs
        let id = make_request_id("short", "ab");
        assert_eq!(id, "short-ab");
    }
}
