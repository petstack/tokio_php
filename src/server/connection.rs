//! TCP/TLS connection handling.

use std::borrow::Cow;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use http::header::{self, HeaderName, HeaderValue};

// ============================================================================
// Header constants for O(1) lookup (avoid string comparison)
// ============================================================================

mod header_names {
    use super::*;

    // Standard headers (from http crate)
    pub static CONTENT_TYPE: HeaderName = header::CONTENT_TYPE;
    pub static USER_AGENT: HeaderName = header::USER_AGENT;
    pub static REFERER: HeaderName = header::REFERER;
    pub static ACCEPT: HeaderName = header::ACCEPT;
    pub static ACCEPT_ENCODING: HeaderName = header::ACCEPT_ENCODING;
    pub static ACCEPT_LANGUAGE: HeaderName = header::ACCEPT_LANGUAGE;
    pub static COOKIE: HeaderName = header::COOKIE;
    pub static HOST: HeaderName = header::HOST;
    pub static IF_NONE_MATCH: HeaderName = header::IF_NONE_MATCH;
    pub static IF_MODIFIED_SINCE: HeaderName = header::IF_MODIFIED_SINCE;
    pub static CONTENT_LENGTH: HeaderName = header::CONTENT_LENGTH;
    pub static RETRY_AFTER: HeaderName = header::RETRY_AFTER;
}

// Custom headers (lazily initialized)
static X_REQUEST_ID: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("x-request-id"));
static X_FORWARDED_FOR: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("x-forwarded-for"));
static X_RATELIMIT_LIMIT: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("x-ratelimit-limit"));
static X_RATELIMIT_REMAINING: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("x-ratelimit-remaining"));
static X_RATELIMIT_RESET: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("x-ratelimit-reset"));
static TRACEPARENT: std::sync::LazyLock<HeaderName> =
    std::sync::LazyLock::new(|| HeaderName::from_static("traceparent"));

// Static header values (zero allocation)
mod header_values {
    use super::*;

    pub static TEXT_PLAIN: HeaderValue = HeaderValue::from_static("text/plain");
    pub static TEXT_PLAIN_UTF8: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
    pub static TEXT_HTML_UTF8: HeaderValue = HeaderValue::from_static("text/html; charset=utf-8");
    pub static ZERO: HeaderValue = HeaderValue::from_static("0");
    pub static ONE: HeaderValue = HeaderValue::from_static("1");
}

// ============================================================================
// HTTP version constants (avoid String allocation)
// ============================================================================

mod http_versions {
    pub const HTTP_10: &str = "HTTP/1.0";
    pub const HTTP_11: &str = "HTTP/1.1";
    pub const HTTP_20: &str = "HTTP/2.0";
    pub const HTTP_30: &str = "HTTP/3.0";

    /// Convert hyper::Version to static string.
    #[inline]
    pub fn from_hyper(version: hyper::Version) -> &'static str {
        match version {
            hyper::Version::HTTP_10 => HTTP_10,
            hyper::Version::HTTP_11 => HTTP_11,
            hyper::Version::HTTP_2 => HTTP_20,
            hyper::Version::HTTP_3 => HTTP_30,
            _ => HTTP_11,
        }
    }
}

// ============================================================================
// ISO 8601 timestamp formatting (zero heap allocation)
// ============================================================================

/// ISO 8601 timestamp buffer - exactly 24 bytes: "2024-01-15T10:30:00.123Z"
/// Stack-allocated, no heap allocation.
#[derive(Clone, Copy)]
pub struct Iso8601Timestamp {
    buf: [u8; 24],
}

impl Iso8601Timestamp {
    /// Create a new timestamp for the current time.
    #[inline]
    pub fn now() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self::from_duration(now)
    }

    /// Create from a Duration since UNIX_EPOCH.
    #[inline]
    pub fn from_duration(duration: Duration) -> Self {
        let secs = duration.as_secs();
        let millis = duration.subsec_millis();

        // Time of day
        let day_secs = secs % 86400;
        let hours = (day_secs / 3600) as u8;
        let minutes = ((day_secs % 3600) / 60) as u8;
        let seconds = (day_secs % 60) as u8;

        // Days since epoch
        let days = secs / 86400;

        // Year calculation (valid for 1970-2099)
        let mut year = 1970u16;
        let mut remaining = days as i64;

        loop {
            let year_days = if is_leap_year(year) { 366 } else { 365 };
            if remaining < year_days {
                break;
            }
            remaining -= year_days;
            year += 1;
        }

        // Month/day calculation
        let leap = is_leap_year(year);
        let month_days: [u8; 12] = if leap {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut month = 1u8;
        for &days_in_month in &month_days {
            if remaining < days_in_month as i64 {
                break;
            }
            remaining -= days_in_month as i64;
            month += 1;
        }
        let day = (remaining + 1) as u8;

        // Build buffer directly (no format! macro)
        let mut buf = [0u8; 24];
        write_u16_padded(&mut buf[0..4], year);
        buf[4] = b'-';
        write_u8_padded(&mut buf[5..7], month);
        buf[7] = b'-';
        write_u8_padded(&mut buf[8..10], day);
        buf[10] = b'T';
        write_u8_padded(&mut buf[11..13], hours);
        buf[13] = b':';
        write_u8_padded(&mut buf[14..16], minutes);
        buf[16] = b':';
        write_u8_padded(&mut buf[17..19], seconds);
        buf[19] = b'.';
        write_u16_padded_3(&mut buf[20..23], millis as u16);
        buf[23] = b'Z';

        Self { buf }
    }

    /// Get the timestamp as a string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: We only write ASCII digits and punctuation
        unsafe { std::str::from_utf8_unchecked(&self.buf) }
    }
}

impl AsRef<str> for Iso8601Timestamp {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for Iso8601Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for Iso8601Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Check if a year is a leap year.
#[inline]
const fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Write a 4-digit year to buffer (0000-9999).
#[inline]
fn write_u16_padded(buf: &mut [u8], val: u16) {
    buf[0] = b'0' + ((val / 1000) % 10) as u8;
    buf[1] = b'0' + ((val / 100) % 10) as u8;
    buf[2] = b'0' + ((val / 10) % 10) as u8;
    buf[3] = b'0' + (val % 10) as u8;
}

/// Write a 2-digit value to buffer (00-99).
#[inline]
fn write_u8_padded(buf: &mut [u8], val: u8) {
    buf[0] = b'0' + (val / 10);
    buf[1] = b'0' + (val % 10);
}

/// Write a 3-digit milliseconds value to buffer (000-999).
#[inline]
fn write_u16_padded_3(buf: &mut [u8], val: u16) {
    buf[0] = b'0' + ((val / 100) % 10) as u8;
    buf[1] = b'0' + ((val / 10) % 10) as u8;
    buf[2] = b'0' + (val % 10) as u8;
}

/// Legacy function for compatibility - returns owned String.
/// Prefer `Iso8601Timestamp::now()` for zero-allocation usage.
#[inline]
pub fn chrono_lite_iso8601() -> String {
    Iso8601Timestamp::now().as_str().to_string()
}

// ============================================================================
// Server variable key constants (zero allocation)
// ============================================================================

mod server_var_keys {
    use std::borrow::Cow;

    // Request timing
    pub const REQUEST_TIME: Cow<'static, str> = Cow::Borrowed("REQUEST_TIME");
    pub const REQUEST_TIME_FLOAT: Cow<'static, str> = Cow::Borrowed("REQUEST_TIME_FLOAT");

    // Request info
    pub const REQUEST_METHOD: Cow<'static, str> = Cow::Borrowed("REQUEST_METHOD");
    pub const REQUEST_URI: Cow<'static, str> = Cow::Borrowed("REQUEST_URI");
    pub const QUERY_STRING: Cow<'static, str> = Cow::Borrowed("QUERY_STRING");

    // Client info
    pub const REMOTE_ADDR: Cow<'static, str> = Cow::Borrowed("REMOTE_ADDR");
    pub const REMOTE_PORT: Cow<'static, str> = Cow::Borrowed("REMOTE_PORT");

    // Server info
    pub const SERVER_NAME: Cow<'static, str> = Cow::Borrowed("SERVER_NAME");
    pub const SERVER_PORT: Cow<'static, str> = Cow::Borrowed("SERVER_PORT");
    pub const SERVER_ADDR: Cow<'static, str> = Cow::Borrowed("SERVER_ADDR");
    pub const SERVER_SOFTWARE: Cow<'static, str> = Cow::Borrowed("SERVER_SOFTWARE");
    pub const SERVER_PROTOCOL: Cow<'static, str> = Cow::Borrowed("SERVER_PROTOCOL");
    pub const DOCUMENT_ROOT: Cow<'static, str> = Cow::Borrowed("DOCUMENT_ROOT");
    pub const GATEWAY_INTERFACE: Cow<'static, str> = Cow::Borrowed("GATEWAY_INTERFACE");

    // Script paths
    pub const SCRIPT_NAME: Cow<'static, str> = Cow::Borrowed("SCRIPT_NAME");
    pub const SCRIPT_FILENAME: Cow<'static, str> = Cow::Borrowed("SCRIPT_FILENAME");
    pub const PHP_SELF: Cow<'static, str> = Cow::Borrowed("PHP_SELF");

    // Content info
    pub const CONTENT_TYPE: Cow<'static, str> = Cow::Borrowed("CONTENT_TYPE");
    pub const CONTENT_LENGTH: Cow<'static, str> = Cow::Borrowed("CONTENT_LENGTH");

    // HTTP headers
    pub const HTTP_HOST: Cow<'static, str> = Cow::Borrowed("HTTP_HOST");
    pub const HTTP_COOKIE: Cow<'static, str> = Cow::Borrowed("HTTP_COOKIE");
    pub const HTTP_USER_AGENT: Cow<'static, str> = Cow::Borrowed("HTTP_USER_AGENT");
    pub const HTTP_REFERER: Cow<'static, str> = Cow::Borrowed("HTTP_REFERER");
    pub const HTTP_ACCEPT_LANGUAGE: Cow<'static, str> = Cow::Borrowed("HTTP_ACCEPT_LANGUAGE");
    pub const HTTP_ACCEPT: Cow<'static, str> = Cow::Borrowed("HTTP_ACCEPT");
    pub const HTTP_TRACEPARENT: Cow<'static, str> = Cow::Borrowed("HTTP_TRACEPARENT");

    // TLS info
    pub const HTTPS: Cow<'static, str> = Cow::Borrowed("HTTPS");
    pub const SSL_PROTOCOL: Cow<'static, str> = Cow::Borrowed("SSL_PROTOCOL");

    // Trace context
    pub const TRACE_ID: Cow<'static, str> = Cow::Borrowed("TRACE_ID");
    pub const SPAN_ID: Cow<'static, str> = Cow::Borrowed("SPAN_ID");
    pub const PARENT_SPAN_ID: Cow<'static, str> = Cow::Borrowed("PARENT_SPAN_ID");
}

// Static server variable values (zero allocation)
mod server_var_values {
    use std::borrow::Cow;

    pub const ADDR_0000: Cow<'static, str> = Cow::Borrowed("0.0.0.0");
    pub const SERVER_SOFTWARE: Cow<'static, str> = Cow::Borrowed("tokio_php/0.1.0");
    pub const GATEWAY_INTERFACE: Cow<'static, str> = Cow::Borrowed("CGI/1.1");
    pub const HTTPS_ON: Cow<'static, str> = Cow::Borrowed("on");
    pub const PORT_80: Cow<'static, str> = Cow::Borrowed("80");
    pub const PORT_443: Cow<'static, str> = Cow::Borrowed("443");
    pub const LOCALHOST: Cow<'static, str> = Cow::Borrowed("localhost");

    // HTTP methods (zero allocation for common methods)
    pub const METHOD_GET: Cow<'static, str> = Cow::Borrowed("GET");
    pub const METHOD_POST: Cow<'static, str> = Cow::Borrowed("POST");
    pub const METHOD_PUT: Cow<'static, str> = Cow::Borrowed("PUT");
    pub const METHOD_DELETE: Cow<'static, str> = Cow::Borrowed("DELETE");
    pub const METHOD_PATCH: Cow<'static, str> = Cow::Borrowed("PATCH");
    pub const METHOD_HEAD: Cow<'static, str> = Cow::Borrowed("HEAD");
    pub const METHOD_OPTIONS: Cow<'static, str> = Cow::Borrowed("OPTIONS");
    pub const METHOD_QUERY: Cow<'static, str> = Cow::Borrowed("QUERY");

    // HTTP protocol versions (zero allocation)
    pub const PROTOCOL_HTTP_10: Cow<'static, str> = Cow::Borrowed("HTTP/1.0");
    pub const PROTOCOL_HTTP_11: Cow<'static, str> = Cow::Borrowed("HTTP/1.1");
    pub const PROTOCOL_HTTP_20: Cow<'static, str> = Cow::Borrowed("HTTP/2.0");
}

// ============================================================================
// Static value helpers (zero allocation for common cases)
// ============================================================================

/// Get static Cow for HTTP method (zero allocation for common methods).
#[inline]
fn method_to_cow(method: &hyper::Method) -> std::borrow::Cow<'static, str> {
    use std::borrow::Cow;
    match method {
        &hyper::Method::GET => server_var_values::METHOD_GET,
        &hyper::Method::POST => server_var_values::METHOD_POST,
        &hyper::Method::PUT => server_var_values::METHOD_PUT,
        &hyper::Method::DELETE => server_var_values::METHOD_DELETE,
        &hyper::Method::PATCH => server_var_values::METHOD_PATCH,
        &hyper::Method::HEAD => server_var_values::METHOD_HEAD,
        &hyper::Method::OPTIONS => server_var_values::METHOD_OPTIONS,
        m if m.as_str() == "QUERY" => server_var_values::METHOD_QUERY,
        m => Cow::Owned(m.as_str().to_string()),
    }
}

/// Get static Cow for HTTP protocol version (zero allocation).
#[inline]
fn protocol_to_cow(version: &str) -> std::borrow::Cow<'static, str> {
    use std::borrow::Cow;
    match version {
        "HTTP/1.0" => server_var_values::PROTOCOL_HTTP_10,
        "HTTP/1.1" => server_var_values::PROTOCOL_HTTP_11,
        "HTTP/2.0" => server_var_values::PROTOCOL_HTTP_20,
        v => Cow::Owned(v.to_string()),
    }
}

// ============================================================================
// IP address formatting (zero heap allocation)
// ============================================================================

/// Format an IP address to a stack buffer, returning the string slice.
/// Buffer must be at least 45 bytes for IPv6 (max: "xxxx:xxxx:xxxx:xxxx:xxxx:xxxx:xxxx:xxxx").
#[inline]
fn format_ip_to_buf(ip: std::net::IpAddr, buf: &mut [u8; 48]) -> &str {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    let _ = write!(cursor, "{}", ip);
    let len = cursor.position() as usize;
    // SAFETY: IP address formatting only produces ASCII
    unsafe { std::str::from_utf8_unchecked(&buf[..len]) }
}

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Incoming as IncomingBody};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, warn};

#[cfg(feature = "otel")]
use crate::observability::tracing_middleware::TracedRequest;

use super::access_log;
use super::config::TlsInfo;
use super::error_pages::{accepts_html, status_reason_phrase, ErrorPages};
use super::request::{parse_cookies, parse_multipart, parse_query_string};
use super::response::{
    accepts_brotli, empty_stub_response, from_script_response, full_to_flexible, is_sse_accept,
    metered_streaming_response, metered_streaming_to_flexible, not_found_response,
    serve_static_file, stub_response_with_profile, FlexibleResponse, BAD_REQUEST_BODY, EMPTY_BODY,
    METHOD_NOT_ALLOWED_BODY,
};
use super::routing::is_php_uri;
use crate::executor::{ExecuteResult, ScriptExecutor, DEFAULT_STREAM_BUFFER_SIZE};
use crate::middleware::rate_limit::RateLimiter;
use crate::types::{ScriptRequest, UploadedFile};

/// Check if an error is a common connection reset or timeout.
#[inline]
fn is_connection_error(err_str: &str) -> bool {
    err_str.contains("connection reset")
        || err_str.contains("broken pipe")
        || err_str.contains("Connection reset")
        || err_str.contains("os error 104")
        || err_str.contains("os error 32")
        || err_str.contains("timed out")
        || err_str.contains("deadline has elapsed")
        || err_str.contains("HeaderTimeout") // Slowloris protection timeout
}

use super::internal::RequestMetrics;
use super::routing::{resolve_request, RouteResult};
use crate::trace_context::TraceContext;

/// Connection handler context.
pub struct ConnectionContext<E: ScriptExecutor> {
    pub executor: Arc<E>,
    pub document_root: Arc<str>,
    /// Cached document root as Cow::Borrowed with 'static lifetime (zero allocation per request).
    /// Created by leaking the document_root string at server startup.
    pub document_root_static: std::borrow::Cow<'static, str>,
    pub is_stub_mode: bool,
    /// Route configuration (INDEX_FILE handling)
    pub route_config: Arc<super::routing::RouteConfig>,
    pub active_connections: Arc<AtomicUsize>,
    pub request_metrics: Arc<RequestMetrics>,
    pub error_pages: ErrorPages,
    pub rate_limiter: Option<Arc<RateLimiter>>,
    pub static_cache_ttl: super::config::StaticCacheTtl,
    pub request_timeout: super::config::RequestTimeout,
    /// SSE timeout (SSE_TIMEOUT env var, default: 30m).
    pub sse_timeout: super::config::RequestTimeout,
    /// Header read timeout (HEADER_TIMEOUT_SECS, default: 5s).
    pub header_timeout: std::time::Duration,
    /// Idle connection timeout (IDLE_TIMEOUT_SECS, default: 60s).
    pub idle_timeout: std::time::Duration,
    /// Profiling enabled (compile-time with debug-profile feature).
    #[allow(dead_code)]
    pub profile_enabled: bool,
    /// Access logging enabled (ACCESS_LOG=1).
    pub access_log_enabled: bool,
    /// File cache (LRU, max 200 entries).
    pub file_cache: Arc<super::file_cache::FileCache>,
}

impl<E: ScriptExecutor + 'static> ConnectionContext<E> {
    /// Handle an incoming TCP connection (with optional TLS).
    pub async fn handle_connection(
        self: Arc<Self>,
        stream: TcpStream,
        remote_addr: SocketAddr,
        tls_acceptor: Option<TlsAcceptor>,
    ) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);

        if let Some(acceptor) = tls_acceptor {
            self.clone()
                .handle_tls_connection(stream, remote_addr, acceptor)
                .await;
        } else {
            self.clone()
                .handle_plain_connection(stream, remote_addr)
                .await;
        }

        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Handle an incoming TCP connection with graceful shutdown support.
    /// When shutdown is triggered, in-flight requests complete naturally before connection closes.
    pub async fn handle_connection_graceful(
        self: Arc<Self>,
        stream: TcpStream,
        remote_addr: SocketAddr,
        tls_acceptor: Option<TlsAcceptor>,
        _shutdown_rx: watch::Receiver<bool>,
    ) {
        // The graceful shutdown is handled at the server level:
        // 1. Accept loops stop when shutdown is triggered
        // 2. Existing connections complete naturally
        // 3. wait_for_drain() waits for active_connections to reach 0
        //
        // Note: HTTP/2 GOAWAY frames would require hyper's graceful_shutdown(),
        // but auto::Builder's API design prevents storing the connection for later use.
        // This is acceptable for most deployments - connections complete in-flight work.
        self.handle_connection(stream, remote_addr, tls_acceptor)
            .await;
    }

    async fn handle_tls_connection(
        self: Arc<Self>,
        stream: TcpStream,
        remote_addr: SocketAddr,
        acceptor: TlsAcceptor,
    ) {
        let tls_start = Instant::now();

        // TLS handshake with timeout
        let tls_stream =
            match tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    debug!("TLS handshake failed: {:?}", e);
                    return;
                }
                Err(_) => {
                    debug!("TLS handshake timeout: {:?}", remote_addr);
                    return;
                }
            };

        let handshake_us = tls_start.elapsed().as_micros() as u64;

        // Extract TLS info from the connection
        let (_, server_conn) = tls_stream.get_ref();
        let tls_info = TlsInfo {
            handshake_us,
            protocol: server_conn
                .protocol_version()
                .map(|v| format!("{:?}", v))
                .unwrap_or_default(),
            alpn: server_conn
                .alpn_protocol()
                .map(|p| String::from_utf8_lossy(p).to_string())
                .unwrap_or_default(),
        };

        let ctx = Arc::clone(&self);
        let service = service_fn(move |req| {
            let ctx = Arc::clone(&ctx);
            let tls = tls_info.clone();
            async move { ctx.handle_request(req, remote_addr, Some(tls)).await }
        });

        let io = TokioIo::new(tls_stream);
        if let Err(err) = auto::Builder::new(TokioExecutor::new())
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(Some(self.header_timeout))
            .keep_alive(true)
            .http2()
            .max_concurrent_streams(250)
            .serve_connection(io, service)
            .await
        {
            let err_str = format!("{:?}", err);
            if !is_connection_error(&err_str) {
                debug!("TLS connection error: {:?}", err);
            }
        }
    }

    async fn handle_plain_connection(self: Arc<Self>, stream: TcpStream, remote_addr: SocketAddr) {
        // Wait for first byte with timeout to detect idle connections (skip for stub mode)
        if !self.is_stub_mode {
            let mut peek_buf = [0u8; 1];
            match tokio::time::timeout(self.idle_timeout, stream.peek(&mut peek_buf)).await {
                Ok(Ok(0)) | Err(_) => {
                    // Connection closed or timeout - client connected but sent nothing
                    debug!("Connection idle timeout or closed: {:?}", remote_addr);
                    return;
                }
                Ok(Err(e)) => {
                    debug!("Peek error: {:?}", e);
                    return;
                }
                Ok(Ok(_)) => {
                    // Data available, proceed
                }
            }
        }

        let ctx = Arc::clone(&self);
        let service = service_fn(move |req| {
            let ctx = Arc::clone(&ctx);
            async move { ctx.handle_request(req, remote_addr, None).await }
        });

        let io = TokioIo::new(stream);
        if let Err(err) = auto::Builder::new(TokioExecutor::new())
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(Some(self.header_timeout))
            .keep_alive(true)
            .http2()
            .max_concurrent_streams(250)
            .serve_connection(io, service)
            .await
        {
            let err_str = format!("{:?}", err);
            if !is_connection_error(&err_str) {
                debug!("Connection error: {:?}", err);
            }
        }
    }

    async fn handle_request(
        &self,
        req: Request<IncomingBody>,
        remote_addr: SocketAddr,
        tls_info: Option<TlsInfo>,
    ) -> Result<FlexibleResponse, Infallible> {
        // Network I/O timing: capture entry time
        let handler_entry_time = Instant::now();

        // Check for SSE request (Accept: text/event-stream)
        let accept_header = req
            .headers()
            .get(&header_names::ACCEPT)
            .and_then(|v| v.to_str().ok());
        let is_sse = is_sse_accept(accept_header);

        // Handle SSE requests separately (streaming response path)
        if is_sse {
            return self.handle_sse_request(req, remote_addr, tls_info).await;
        }

        // Normal (non-streaming) request path
        let request_start = Instant::now();

        // Start OpenTelemetry span for tracing (if otel feature enabled)
        #[cfg(feature = "otel")]
        let traced_request = TracedRequest::new(&req);

        // Extract or generate W3C Trace Context
        let trace_ctx = TraceContext::from_headers(req.headers());

        // Use trace_id as request_id for correlation, or fall back to X-Request-ID
        // Zero-allocation when no X-Request-ID header (common case)
        let request_id_from_header: Option<String> = req
            .headers()
            .get(&*X_REQUEST_ID)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());
        let request_id: &str = request_id_from_header
            .as_deref()
            .unwrap_or_else(|| trace_ctx.short_id());

        // Check rate limit (per-IP) with timing
        let rate_limit_start = Instant::now();
        if let Some(ref limiter) = self.rate_limiter {
            let (allowed, _remaining, reset_after) = limiter.check(remote_addr.ip());
            if !allowed {
                let mut response = Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .header(
                        header_names::CONTENT_TYPE.clone(),
                        header_values::TEXT_PLAIN.clone(),
                    )
                    .header(header_names::RETRY_AFTER.clone(), reset_after.to_string())
                    .header(X_RATELIMIT_LIMIT.clone(), limiter.limit().to_string())
                    .header(X_RATELIMIT_REMAINING.clone(), header_values::ZERO.clone())
                    .header(X_RATELIMIT_RESET.clone(), reset_after.to_string())
                    .body(Full::new(Bytes::from_static(b"429 Too Many Requests")))
                    .unwrap();
                response
                    .headers_mut()
                    .insert(X_REQUEST_ID.clone(), request_id.parse().unwrap());
                return Ok(full_to_flexible(response));
            }
        }
        let rate_limit_us = rate_limit_start.elapsed().as_micros() as u64;

        // Increment request method metrics
        self.request_metrics.increment_method(req.method());

        let is_head = *req.method() == Method::HEAD;

        // Capture data for access logging (before consuming request)
        let access_log_enabled = self.access_log_enabled;
        let method_str = req.method().to_string();
        let uri_str = req.uri().path().to_string();
        let query_str = req.uri().query().map(|s| s.to_string());
        let http_version = http_versions::from_hyper(req.version());

        // Extract headers for access log
        let (user_agent_log, referer_log, xff_log) = if access_log_enabled {
            (
                req.headers()
                    .get(&header_names::USER_AGENT)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string()),
                req.headers()
                    .get(&header_names::REFERER)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string()),
                req.headers()
                    .get(&*X_FORWARDED_FOR)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string()),
            )
        } else {
            (None, None, None)
        };

        // Extract TLS protocol for access log (before tls_info is moved)
        let tls_protocol_log = tls_info.as_ref().map(|t| t.protocol.clone());

        // Check if client accepts HTML (for custom error pages)
        let client_accepts_html = req
            .headers()
            .get(&header_names::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(accepts_html)
            .unwrap_or(false);

        let mut response = match req.method().as_str() {
            "GET" | "POST" | "HEAD" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "QUERY" => {
                let mut resp = self
                    .process_request(
                        req,
                        remote_addr,
                        tls_info,
                        &trace_ctx,
                        rate_limit_us,
                        handler_entry_time,
                    )
                    .await;

                // HEAD: return headers only, no body
                if is_head {
                    let (parts, _) = resp.into_parts();
                    resp = full_to_flexible(Response::from_parts(
                        parts,
                        Full::new(EMPTY_BODY.clone()),
                    ));
                }
                resp
            }
            _ => full_to_flexible(
                Response::builder()
                    .status(StatusCode::METHOD_NOT_ALLOWED)
                    .header(
                        header_names::CONTENT_TYPE.clone(),
                        header_values::TEXT_PLAIN.clone(),
                    )
                    .body(Full::new(METHOD_NOT_ALLOWED_BODY.clone()))
                    .unwrap(),
            ),
        };

        // Apply custom error page or default reason phrase for 4xx/5xx responses
        let status = response.status().as_u16();
        if (400..600).contains(&status) {
            let body_is_empty = response.body().size_hint().exact() == Some(0);
            if body_is_empty {
                // Try custom error page first (if client accepts HTML)
                if client_accepts_html {
                    if let Some(error_html) = self.error_pages.get(status) {
                        let (mut parts, _) = response.into_parts();
                        parts.headers.insert(
                            header_names::CONTENT_TYPE.clone(),
                            header_values::TEXT_HTML_UTF8.clone(),
                        );
                        parts.headers.insert(
                            header_names::CONTENT_LENGTH.clone(),
                            error_html.len().to_string().parse().unwrap(),
                        );
                        response = full_to_flexible(Response::from_parts(
                            parts,
                            Full::new(error_html.clone()),
                        ));
                    } else {
                        // No custom page, use default reason phrase
                        let reason = status_reason_phrase(status);
                        let (mut parts, _) = response.into_parts();
                        parts.headers.insert(
                            header_names::CONTENT_TYPE.clone(),
                            header_values::TEXT_PLAIN_UTF8.clone(),
                        );
                        parts.headers.insert(
                            header_names::CONTENT_LENGTH.clone(),
                            reason.len().to_string().parse().unwrap(),
                        );
                        response = full_to_flexible(Response::from_parts(
                            parts,
                            Full::new(Bytes::from(reason)),
                        ));
                    }
                } else {
                    // Non-HTML client, use default reason phrase
                    let reason = status_reason_phrase(status);
                    let (mut parts, _) = response.into_parts();
                    parts.headers.insert(
                        header_names::CONTENT_TYPE.clone(),
                        header_values::TEXT_PLAIN_UTF8.clone(),
                    );
                    parts.headers.insert(
                        header_names::CONTENT_LENGTH.clone(),
                        reason.len().to_string().parse().unwrap(),
                    );
                    response = full_to_flexible(Response::from_parts(
                        parts,
                        Full::new(Bytes::from(reason)),
                    ));
                }
            }
        }

        // Record response time and status metrics
        let response_time_us = request_start.elapsed().as_micros() as u64;
        self.request_metrics.record_response_time(response_time_us);
        self.request_metrics
            .increment_status(response.status().as_u16());

        // Add X-Request-ID header to response
        response
            .headers_mut()
            .insert(X_REQUEST_ID.clone(), request_id.parse().unwrap());

        // Add W3C Trace Context header to response (zero-allocation)
        response.headers_mut().insert(
            TRACEPARENT.clone(),
            trace_ctx.traceparent().parse().unwrap(),
        );

        // Access logging (optimized: stack-allocated timestamp, no heap alloc for IP)
        if access_log_enabled {
            let duration = request_start.elapsed();
            let body_size = response.body().size_hint().exact().unwrap_or(0);
            let ts = Iso8601Timestamp::now();

            // Format IP to stack buffer (max IPv6 is 45 chars, use 48 for safety)
            let mut ip_buf = [0u8; 48];
            let ip_str = format_ip_to_buf(remote_addr.ip(), &mut ip_buf);

            access_log::log_request(
                ts.as_str(),
                request_id,
                ip_str,
                &method_str,
                &uri_str,
                query_str.as_deref(),
                http_version,
                response.status().as_u16(),
                body_size,
                duration.as_secs_f64() * 1000.0,
                user_agent_log.as_deref(),
                referer_log.as_deref(),
                xff_log.as_deref(),
                tls_protocol_log.as_deref(),
                Some(trace_ctx.trace_id()),
                Some(trace_ctx.span_id()),
            );
        }

        // End OpenTelemetry span (if otel feature enabled)
        #[cfg(feature = "otel")]
        traced_request.end(response.status().as_u16());

        Ok(response)
    }

    #[allow(unused_variables, unused_mut, unused_assignments)]
    async fn process_request(
        &self,
        req: Request<IncomingBody>,
        remote_addr: SocketAddr,
        tls_info: Option<TlsInfo>,
        trace_ctx: &TraceContext,
        rate_limit_us: u64,
        handler_entry_time: Instant,
    ) -> FlexibleResponse {
        // Calculate handler entry delay (time from handler start to processing start)
        let net_handler_entry_us = handler_entry_time.elapsed().as_micros() as u64;

        // Capture request timestamp at the very start
        let request_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let request_time_secs = request_time.as_secs();
        let request_time_float = request_time.as_secs_f64();

        let parse_start = Instant::now();

        // Profile timing variables (used when profiling_enabled is true)
        let mut headers_extract_us = 0u64;
        let mut query_parse_us = 0u64;
        let mut cookies_parse_us = 0u64;
        let mut body_read_us = 0u64;
        let mut body_parse_us = 0u64;
        let mut server_vars_us = 0u64;
        let mut path_resolve_us = 0u64;
        let mut file_check_us = 0u64;

        let method = req.method().clone();
        let http_version = http_versions::from_hyper(req.version());
        let uri = req.uri().clone();
        let uri_path = uri.path();
        let query_string = uri.query().unwrap_or("");

        // Profiling is controlled by compile-time feature, not runtime header
        #[cfg(feature = "debug-profile")]
        let profiling_enabled = true;
        #[cfg(not(feature = "debug-profile"))]
        let profiling_enabled = false;

        // Check if client accepts Brotli compression
        let use_brotli = req
            .headers()
            .get(&header_names::ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .map(accepts_brotli)
            .unwrap_or(false);

        // Extract conditional caching headers for static file serving
        let if_none_match = req
            .headers()
            .get(&header_names::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let if_modified_since = req
            .headers()
            .get(&header_names::IF_MODIFIED_SINCE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // Fast path for stub mode only
        if self.is_stub_mode && is_php_uri(uri_path) {
            if profiling_enabled {
                let total_us = parse_start.elapsed().as_micros() as u64;
                let (tls_handshake_us, tls_protocol, tls_alpn) = match &tls_info {
                    Some(tls) => (tls.handshake_us, tls.protocol.as_str(), tls.alpn.as_str()),
                    None => (0, "", ""),
                };
                return full_to_flexible(stub_response_with_profile(
                    total_us,
                    http_version,
                    tls_handshake_us,
                    tls_protocol,
                    tls_alpn,
                ));
            }
            return full_to_flexible(empty_stub_response());
        }

        // Full processing path - extract headers before consuming body
        let headers_start = Instant::now();
        let headers = req.headers();

        let content_type_str = headers
            .get(&header_names::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let cookie_header_str = headers
            .get(&header_names::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // For HTTP/2, the :authority pseudo-header is in uri.authority()
        let host_header = headers
            .get(&header_names::HOST)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .or_else(|| uri.authority().map(|a| a.to_string()))
            .unwrap_or_default();

        let user_agent = headers
            .get(&header_names::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let referer = headers
            .get(&header_names::REFERER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let accept_language = headers
            .get(&header_names::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let accept = headers
            .get(&header_names::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if profiling_enabled {
            headers_extract_us = headers_start.elapsed().as_micros() as u64;
        }

        // Parse cookies
        let cookies_start = Instant::now();
        let has_cookies = !cookie_header_str.is_empty();
        let cookies = if has_cookies {
            parse_cookies(&cookie_header_str)
        } else {
            Vec::new()
        };
        if profiling_enabled {
            cookies_parse_us = cookies_start.elapsed().as_micros() as u64;
        }

        // Parse query string
        let query_start = Instant::now();
        let get_params = if query_string.is_empty() {
            Vec::new()
        } else {
            parse_query_string(query_string)
        };
        if profiling_enabled {
            query_parse_us = query_start.elapsed().as_micros() as u64;
        }

        // Handle request body (POST, PUT, PATCH, DELETE, OPTIONS, QUERY - not GET/HEAD)
        let method_str = method.as_str();
        let has_body = matches!(
            method_str,
            "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "QUERY"
        );
        let (post_params, files, raw_body) = if has_body {
            let body_read_start = Instant::now();
            let body_bytes = match req.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    return full_to_flexible(
                        Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .header(
                                header_names::CONTENT_TYPE.clone(),
                                header_values::TEXT_PLAIN.clone(),
                            )
                            .body(Full::new(BAD_REQUEST_BODY.clone()))
                            .unwrap(),
                    );
                }
            };
            if profiling_enabled {
                body_read_us = body_read_start.elapsed().as_micros() as u64;
            }

            // Store raw body for php://input (QUERY method especially needs this)
            let raw_body_bytes = body_bytes.clone();

            let body_parse_start = Instant::now();
            let content_type_lower = content_type_str.to_lowercase();
            let result = if content_type_lower.starts_with("application/x-www-form-urlencoded") {
                let body_str = String::from_utf8_lossy(&body_bytes);
                (parse_query_string(&body_str), Vec::new())
            } else if content_type_lower.starts_with("multipart/form-data") {
                match parse_multipart(&content_type_str, body_bytes).await {
                    Ok((params, uploaded_files)) => (params, uploaded_files),
                    Err(e) => {
                        return full_to_flexible(
                            Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .header(
                                    header_names::CONTENT_TYPE.clone(),
                                    header_values::TEXT_PLAIN.clone(),
                                )
                                .body(Full::new(Bytes::from(format!(
                                    "Failed to parse multipart form: {}",
                                    e
                                ))))
                                .unwrap(),
                        );
                    }
                }
            } else {
                // For JSON, XML, etc. - body available via raw_body
                (Vec::new(), Vec::new())
            };
            if profiling_enabled {
                body_parse_us = body_parse_start.elapsed().as_micros() as u64;
            }
            (result.0, result.1, Some(raw_body_bytes))
        } else {
            (Vec::new(), Vec::new(), None)
        };

        // Resolve route (routing + file existence check combined)
        let path_start = Instant::now();
        let route_result = if self.is_stub_mode {
            // Stub mode: route to PHP without file checks
            RouteResult::Execute(format!("{}/index.php", self.document_root))
        } else {
            resolve_request(uri_path, &self.route_config, &self.file_cache)
        };

        // Handle routing result
        let file_path_string = match &route_result {
            RouteResult::Execute(path) | RouteResult::Serve(path) => path.clone(),
            RouteResult::NotFound => {
                return full_to_flexible(not_found_response());
            }
        };
        let file_path = Path::new(&file_path_string);
        let is_php = matches!(route_result, RouteResult::Execute(_));

        // For profiling compatibility
        let file_cache_hit = false; // Cache hit info is now internal to resolve_request
        if profiling_enabled {
            path_resolve_us = path_start.elapsed().as_micros() as u64;
            file_check_us = 0; // Combined with path resolution
        }

        // Build server variables
        let server_vars_start = Instant::now();

        // Parse Host header for SERVER_NAME and SERVER_PORT
        // Parse server name and port from Host header (using Cow for static ports)
        let (server_name, server_port): (Cow<'static, str>, Cow<'static, str>) =
            if !host_header.is_empty() {
                if let Some(colon_pos) = host_header.rfind(':') {
                    if host_header.starts_with('[') && !host_header.contains("]:") {
                        // IPv6 without port
                        (
                            Cow::Owned(host_header.clone()),
                            if tls_info.is_some() {
                                server_var_values::PORT_443
                            } else {
                                server_var_values::PORT_80
                            },
                        )
                    } else {
                        // Host:port format
                        (
                            Cow::Owned(host_header[..colon_pos].to_string()),
                            Cow::Owned(host_header[colon_pos + 1..].to_string()),
                        )
                    }
                } else {
                    // No port in header
                    (
                        Cow::Owned(host_header.clone()),
                        if tls_info.is_some() {
                            server_var_values::PORT_443
                        } else {
                            server_var_values::PORT_80
                        },
                    )
                }
            } else {
                // No Host header
                (
                    server_var_values::LOCALHOST,
                    if tls_info.is_some() {
                        server_var_values::PORT_443
                    } else {
                        server_var_values::PORT_80
                    },
                )
            };

        // Calculate SCRIPT_NAME and PHP_SELF
        let script_name = file_path_string
            .strip_prefix(self.document_root.as_ref())
            .unwrap_or(&file_path_string);
        let script_name: Cow<'static, str> = if script_name.starts_with('/') {
            Cow::Owned(script_name.to_string())
        } else {
            Cow::Owned(format!("/{}", script_name))
        };

        let mut server_vars = Vec::with_capacity(32);

        // Request timing (keys static, values dynamic)
        server_vars.push((
            server_var_keys::REQUEST_TIME,
            Cow::Owned(request_time_secs.to_string()),
        ));
        server_vars.push((
            server_var_keys::REQUEST_TIME_FLOAT,
            Cow::Owned(format!("{:.6}", request_time_float)),
        ));

        // Request method and URI (zero allocation for common methods)
        server_vars.push((server_var_keys::REQUEST_METHOD, method_to_cow(&method)));
        server_vars.push((server_var_keys::REQUEST_URI, Cow::Owned(uri.to_string())));
        server_vars.push((
            server_var_keys::QUERY_STRING,
            Cow::Owned(query_string.to_string()),
        ));

        // Client info
        server_vars.push((
            server_var_keys::REMOTE_ADDR,
            Cow::Owned(remote_addr.ip().to_string()),
        ));
        server_vars.push((
            server_var_keys::REMOTE_PORT,
            Cow::Owned(remote_addr.port().to_string()),
        ));

        // Server info (mix of static and dynamic values)
        server_vars.push((server_var_keys::SERVER_NAME, server_name));
        server_vars.push((server_var_keys::SERVER_PORT, server_port));
        server_vars.push((server_var_keys::SERVER_ADDR, server_var_values::ADDR_0000));
        server_vars.push((
            server_var_keys::SERVER_SOFTWARE,
            server_var_values::SERVER_SOFTWARE,
        ));
        // Protocol version (zero allocation for HTTP/1.0, HTTP/1.1, HTTP/2.0)
        server_vars.push((
            server_var_keys::SERVER_PROTOCOL,
            protocol_to_cow(http_version),
        ));
        // Document root (cached at server startup, zero allocation per request)
        server_vars.push((
            server_var_keys::DOCUMENT_ROOT,
            self.document_root_static.clone(),
        ));
        server_vars.push((
            server_var_keys::GATEWAY_INTERFACE,
            server_var_values::GATEWAY_INTERFACE,
        ));

        // Script paths
        server_vars.push((server_var_keys::SCRIPT_NAME, script_name.clone()));
        server_vars.push((
            server_var_keys::SCRIPT_FILENAME,
            Cow::Owned(file_path_string.clone()),
        ));
        server_vars.push((server_var_keys::PHP_SELF, script_name));

        // Content info
        server_vars.push((server_var_keys::CONTENT_TYPE, Cow::Owned(content_type_str)));

        // HTTP headers (all dynamic values)
        if !host_header.is_empty() {
            server_vars.push((server_var_keys::HTTP_HOST, Cow::Owned(host_header)));
        }
        if !cookie_header_str.is_empty() {
            server_vars.push((server_var_keys::HTTP_COOKIE, Cow::Owned(cookie_header_str)));
        }
        if !user_agent.is_empty() {
            server_vars.push((server_var_keys::HTTP_USER_AGENT, Cow::Owned(user_agent)));
        }
        if !referer.is_empty() {
            server_vars.push((server_var_keys::HTTP_REFERER, Cow::Owned(referer)));
        }
        if !accept_language.is_empty() {
            server_vars.push((
                server_var_keys::HTTP_ACCEPT_LANGUAGE,
                Cow::Owned(accept_language),
            ));
        }
        if !accept.is_empty() {
            server_vars.push((server_var_keys::HTTP_ACCEPT, Cow::Owned(accept)));
        }

        // HTTPS/TLS info (static value "on")
        if let Some(ref tls) = tls_info {
            server_vars.push((server_var_keys::HTTPS, server_var_values::HTTPS_ON));
            if !tls.protocol.is_empty() {
                server_vars.push((
                    server_var_keys::SSL_PROTOCOL,
                    Cow::Owned(tls.protocol.clone()),
                ));
            }
        }

        // W3C Trace Context for distributed tracing
        // Note: still need to_owned() for PHP $_SERVER vars (different lifetime)
        server_vars.push((
            server_var_keys::HTTP_TRACEPARENT,
            Cow::Owned(trace_ctx.traceparent().to_owned()),
        ));
        server_vars.push((
            server_var_keys::TRACE_ID,
            Cow::Owned(trace_ctx.trace_id().to_owned()),
        ));
        server_vars.push((
            server_var_keys::SPAN_ID,
            Cow::Owned(trace_ctx.span_id().to_owned()),
        ));
        if let Some(parent) = trace_ctx.parent_span_id() {
            server_vars.push((
                server_var_keys::PARENT_SPAN_ID,
                Cow::Owned(parent.to_owned()),
            ));
        }

        // Set CONTENT_LENGTH for requests with body
        if let Some(ref body) = raw_body {
            let len: usize = body.len();
            server_vars.push((server_var_keys::CONTENT_LENGTH, Cow::Owned(len.to_string())));
        }

        if profiling_enabled {
            server_vars_us = server_vars_start.elapsed().as_micros() as u64;
        }

        if is_php {
            let temp_files: Vec<String> = files
                .iter()
                .flat_map(|(_, file_vec): &(String, Vec<UploadedFile>)| {
                    file_vec.iter().map(|f: &UploadedFile| f.tmp_name.clone())
                })
                .filter(|path: &String| !path.is_empty())
                .collect();

            let parse_request_us = if profiling_enabled {
                parse_start.elapsed().as_micros() as u64
            } else {
                0
            };

            let script_request = ScriptRequest {
                script_path: file_path.to_string_lossy().into_owned(),
                get_params,
                post_params,
                cookies,
                server_vars,
                files,
                raw_body: raw_body.map(|b: Bytes| b.to_vec()),
                profile: profiling_enabled,
                timeout: self.request_timeout.as_duration(),
                received_at: request_time_float,
                request_id: trace_ctx.short_id().to_string(),
                trace_id: trace_ctx.trace_id().to_string(),
                span_id: trace_ctx.span_id().to_string(),
            };

            // Track pending requests for metrics (guard ensures cleanup on cancel)
            let _pending_guard = RequestMetrics::pending_guard(&self.request_metrics);

            // Use execute_with_auto_sse for automatic SSE detection based on Content-Type header
            let execute_result = self.executor.execute_with_auto_sse(script_request).await;

            let response = match execute_result {
                Ok(ExecuteResult::Normal(resp)) => {
                    let mut resp = *resp; // Unbox
                                          // Add parse breakdown to profile data if profiling
                    #[cfg(feature = "debug-profile")]
                    {
                        use crate::profiler::RouteType;

                        if let Some(ref mut profile) = resp.profile {
                            profile.http_version = http_version.to_string();
                            if let Some(ref tls) = tls_info {
                                profile.tls_handshake_us = tls.handshake_us;
                                profile.tls_protocol = tls.protocol.clone();
                                profile.tls_alpn = tls.alpn.clone();
                            } else {
                                profile.skip("TLS handshake", "Plain HTTP connection");
                            }

                            // Network I/O timing
                            profile.net_handler_entry_us = net_handler_entry_us;
                            profile.net_response_size_bytes = resp.body.len() as u64;

                            // Routing info
                            profile.route_type = RouteType::Php;
                            profile.resolved_path = file_path_string.clone();
                            profile.index_file_mode = self.route_config.index_file.is_some();
                            profile.file_cache_hit = file_cache_hit;

                            profile.request_method = method.to_string();
                            profile.request_url = uri.to_string();
                            profile.rate_limit_us = rate_limit_us;
                            profile.parse_request_us = parse_request_us;
                            profile.headers_extract_us = headers_extract_us;
                            profile.query_parse_us = query_parse_us;
                            profile.cookies_parse_us = cookies_parse_us;
                            profile.body_read_us = body_read_us;
                            profile.body_parse_us = body_parse_us;
                            profile.server_vars_us = server_vars_us;
                            profile.path_resolve_us = path_resolve_us;
                            profile.file_check_us = file_check_us;

                            // Add skipped actions based on request
                            if self.rate_limiter.is_none() {
                                profile.skip(
                                    "Rate limit check",
                                    "Rate limiting disabled (RATE_LIMIT=0)",
                                );
                            }
                            if query_string.is_empty() {
                                profile.skip("Query string parsing", "No query string in URL");
                            }
                            if !has_cookies {
                                profile.skip("Cookie parsing", "No Cookie header present");
                            }
                            if !has_body {
                                profile.skip(
                                    "Body parsing",
                                    format!("{} request has no body", method),
                                );
                            }
                            if !use_brotli {
                                profile.skip(
                                    "Brotli compression",
                                    "Client doesn't accept br encoding",
                                );
                            }
                        }
                    }

                    // Write profile report to file (debug-profile feature only)
                    #[cfg(feature = "debug-profile")]
                    if let Some(ref profile) = resp.profile {
                        profile.write_report(trace_ctx.short_id());
                    }

                    full_to_flexible(from_script_response(resp, profiling_enabled, use_brotli))
                }
                Ok(ExecuteResult::Streaming {
                    headers,
                    status_code,
                    receiver,
                }) => {
                    // PHP enabled SSE via Content-Type: text/event-stream header
                    // Track SSE connection (ended tracked via metered stream Drop)
                    self.request_metrics.sse_connection_started();

                    // Build streaming response with metrics tracking
                    let response = metered_streaming_response(
                        status_code,
                        headers,
                        receiver,
                        Arc::clone(&self.request_metrics),
                    );
                    metered_streaming_to_flexible(response)
                }
                Err(e) => {
                    if e.is_timeout() {
                        // Request timed out
                        warn!("Request timeout: {}", uri_path);
                        full_to_flexible(
                            Response::builder()
                                .status(StatusCode::GATEWAY_TIMEOUT)
                                .header(
                                    header_names::CONTENT_TYPE.clone(),
                                    header_values::TEXT_PLAIN.clone(),
                                )
                                .body(Full::new(Bytes::from_static(b"504 Gateway Timeout")))
                                .unwrap(),
                        )
                    } else if e.is_queue_full() {
                        // Queue is full - server overloaded
                        self.request_metrics.inc_dropped();
                        full_to_flexible(
                            Response::builder()
                                .status(StatusCode::SERVICE_UNAVAILABLE)
                                .header(
                                    header_names::CONTENT_TYPE.clone(),
                                    header_values::TEXT_PLAIN.clone(),
                                )
                                .header(
                                    header_names::RETRY_AFTER.clone(),
                                    header_values::ONE.clone(),
                                )
                                .body(Full::new(Bytes::from_static(
                                    b"503 Service Unavailable - Server overloaded",
                                )))
                                .unwrap(),
                        )
                    } else {
                        error!("Script execution error: {}", e);
                        full_to_flexible(
                            Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header(
                                    header_names::CONTENT_TYPE.clone(),
                                    header_values::TEXT_HTML_UTF8.clone(),
                                )
                                .body(Full::new(Bytes::from(format!(
                                    "<h1>500 Internal Server Error</h1><pre>{}</pre>",
                                    e
                                ))))
                                .unwrap(),
                        )
                    }
                }
            };

            // Clean up temp files
            for temp_file in temp_files {
                let _ = tokio::fs::remove_file(&temp_file).await;
            }

            response
        } else {
            // serve_static_file returns FlexibleResponse directly
            // (handles both small in-memory files and large streaming files)
            serve_static_file(
                file_path,
                use_brotli,
                &self.static_cache_ttl,
                if_none_match.as_deref(),
                if_modified_since.as_deref(),
            )
            .await
        }
    }

    /// Handle an SSE (Server-Sent Events) streaming request.
    ///
    /// This method is called for requests with `Accept: text/event-stream` header.
    /// It uses the streaming executor path and returns a streaming response.
    async fn handle_sse_request(
        &self,
        req: Request<IncomingBody>,
        remote_addr: SocketAddr,
        tls_info: Option<TlsInfo>,
    ) -> Result<FlexibleResponse, Infallible> {
        let request_start = Instant::now();

        // Start OpenTelemetry span for SSE tracing (if otel feature enabled)
        #[cfg(feature = "otel")]
        let traced_request = TracedRequest::new(&req);

        let trace_ctx = TraceContext::from_headers(req.headers());

        // Get request ID
        let request_id_from_header: Option<String> = req
            .headers()
            .get(&*X_REQUEST_ID)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());
        let request_id: &str = request_id_from_header
            .as_deref()
            .unwrap_or_else(|| trace_ctx.short_id());

        // Increment request method metrics
        self.request_metrics.increment_method(req.method());

        let method = req.method().clone();
        let uri = req.uri().clone();
        let uri_path = uri.path();
        let query_string = uri.query().unwrap_or("");

        // Resolve route
        let route_result = if self.is_stub_mode {
            RouteResult::Execute(format!("{}/index.php", self.document_root))
        } else {
            resolve_request(uri_path, &self.route_config, &self.file_cache)
        };

        // SSE only works for PHP scripts (RouteResult::Execute)
        let file_path_string = match route_result {
            RouteResult::Execute(path) => path,
            RouteResult::Serve(_) => {
                // Return error for non-PHP SSE requests
                #[cfg(feature = "otel")]
                traced_request.end(400);
                let response = Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header(
                        header_names::CONTENT_TYPE.clone(),
                        header_values::TEXT_PLAIN.clone(),
                    )
                    .body(Full::new(Bytes::from_static(
                        b"SSE only supported for PHP scripts",
                    )))
                    .unwrap();
                return Ok(full_to_flexible(response));
            }
            RouteResult::NotFound => {
                #[cfg(feature = "otel")]
                traced_request.end(404);
                return Ok(full_to_flexible(not_found_response()));
            }
        };
        let file_path = Path::new(&file_path_string);

        // Build minimal server vars for SSE (optimized with static values)
        let request_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        let mut server_vars = Vec::with_capacity(16);
        // Method: zero allocation for common methods (GET, POST, etc.)
        server_vars.push((server_var_keys::REQUEST_METHOD, method_to_cow(&method)));
        server_vars.push((server_var_keys::REQUEST_URI, Cow::Owned(uri.to_string())));
        server_vars.push((
            server_var_keys::QUERY_STRING,
            Cow::Owned(query_string.to_string()),
        ));
        server_vars.push((
            server_var_keys::REMOTE_ADDR,
            Cow::Owned(remote_addr.ip().to_string()),
        ));
        server_vars.push((
            server_var_keys::SCRIPT_FILENAME,
            Cow::Owned(file_path_string.clone()),
        ));
        // Document root: cached at server startup, zero allocation per request
        server_vars.push((
            server_var_keys::DOCUMENT_ROOT,
            self.document_root_static.clone(),
        ));
        server_vars.push((
            server_var_keys::SERVER_SOFTWARE,
            server_var_values::SERVER_SOFTWARE,
        ));
        server_vars.push((
            server_var_keys::REQUEST_TIME,
            Cow::Owned(request_time.as_secs().to_string()),
        ));

        if let Some(ref tls) = tls_info {
            server_vars.push((server_var_keys::HTTPS, server_var_values::HTTPS_ON));
            if !tls.protocol.is_empty() {
                server_vars.push((
                    server_var_keys::SSL_PROTOCOL,
                    Cow::Owned(tls.protocol.clone()),
                ));
            }
        }

        // Parse query string and cookies for SSE
        let get_params = if query_string.is_empty() {
            Vec::new()
        } else {
            parse_query_string(query_string)
        };

        let cookie_header_str = req
            .headers()
            .get(&header_names::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let cookies = if cookie_header_str.is_empty() {
            Vec::new()
        } else {
            parse_cookies(cookie_header_str)
        };

        let script_request = ScriptRequest {
            script_path: file_path.to_string_lossy().into_owned(),
            get_params,
            post_params: Vec::new(),
            cookies,
            server_vars,
            files: Vec::new(),
            raw_body: None,
            profile: false,
            timeout: self.sse_timeout.as_duration(), // Use SSE timeout (longer than regular)
            received_at: request_time.as_secs_f64(),
            request_id: request_id.to_string(),
            trace_id: trace_ctx.trace_id().to_string(),
            span_id: trace_ctx.span_id().to_string(),
        };

        // Execute streaming request
        match self
            .executor
            .execute_streaming(script_request, DEFAULT_STREAM_BUFFER_SIZE)
            .await
        {
            Ok(stream_rx) => {
                // Track SSE connection (ended tracked via metered stream Drop)
                self.request_metrics.sse_connection_started();

                // Build SSE headers
                let mut headers = vec![
                    ("Content-Type".to_string(), "text/event-stream".to_string()),
                    ("Cache-Control".to_string(), "no-cache".to_string()),
                    ("Connection".to_string(), "keep-alive".to_string()),
                    ("X-Accel-Buffering".to_string(), "no".to_string()),
                    ("X-Request-ID".to_string(), request_id.to_string()),
                    (
                        "traceparent".to_string(),
                        trace_ctx.traceparent().to_owned(),
                    ),
                ];

                // Add Server header
                headers.push(("Server".to_string(), "tokio_php/0.1.0".to_string()));

                // Build streaming response with metrics tracking
                let response = metered_streaming_response(
                    200,
                    headers,
                    stream_rx,
                    Arc::clone(&self.request_metrics),
                );

                // Record metrics
                let response_time_us = request_start.elapsed().as_micros() as u64;
                self.request_metrics.record_response_time(response_time_us);
                self.request_metrics.increment_status(200);

                // End OpenTelemetry span for SSE (if otel feature enabled)
                #[cfg(feature = "otel")]
                traced_request.end(200);

                Ok(metered_streaming_to_flexible(response))
            }
            Err(e) => {
                // Streaming not supported or error
                let response = Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header(
                        header_names::CONTENT_TYPE.clone(),
                        header_values::TEXT_PLAIN.clone(),
                    )
                    .body(Full::new(Bytes::from(format!("SSE error: {}", e))))
                    .unwrap();

                // End OpenTelemetry span for SSE error (if otel feature enabled)
                #[cfg(feature = "otel")]
                traced_request.end(500);

                Ok(full_to_flexible(response))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso8601_timestamp_format() {
        // Test a known timestamp: 2024-01-15T10:50:45.123Z
        // Unix timestamp: 1705315845.123
        let duration = Duration::new(1705315845, 123_000_000);
        let ts = Iso8601Timestamp::from_duration(duration);

        assert_eq!(ts.as_str(), "2024-01-15T10:50:45.123Z");
        assert_eq!(ts.as_str().len(), 24);
    }

    #[test]
    fn test_iso8601_timestamp_epoch() {
        // Unix epoch
        let ts = Iso8601Timestamp::from_duration(Duration::ZERO);
        assert_eq!(ts.as_str(), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_iso8601_timestamp_leap_year() {
        // Feb 29, 2024 (leap year)
        // 2024-02-29T12:00:00.500Z
        let duration = Duration::new(1709208000, 500_000_000);
        let ts = Iso8601Timestamp::from_duration(duration);

        assert_eq!(ts.as_str(), "2024-02-29T12:00:00.500Z");
    }

    #[test]
    fn test_iso8601_timestamp_now() {
        let ts = Iso8601Timestamp::now();
        let s = ts.as_str();

        // Basic format validation
        assert_eq!(s.len(), 24);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "T");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
        assert_eq!(&s[19..20], ".");
        assert_eq!(&s[23..24], "Z");
    }

    #[test]
    fn test_iso8601_timestamp_display() {
        let duration = Duration::new(1705315845, 123_000_000);
        let ts = Iso8601Timestamp::from_duration(duration);

        assert_eq!(format!("{}", ts), "2024-01-15T10:50:45.123Z");
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000)); // divisible by 400
        assert!(is_leap_year(2024)); // divisible by 4, not by 100
        assert!(!is_leap_year(1900)); // divisible by 100, not by 400
        assert!(!is_leap_year(2023)); // not divisible by 4
    }

    #[test]
    fn test_format_ip_to_buf_v4() {
        use std::net::{IpAddr, Ipv4Addr};

        let mut buf = [0u8; 48];
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let s = format_ip_to_buf(ip, &mut buf);

        assert_eq!(s, "127.0.0.1");
    }

    #[test]
    fn test_format_ip_to_buf_v6() {
        use std::net::{IpAddr, Ipv6Addr};

        let mut buf = [0u8; 48];
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let s = format_ip_to_buf(ip, &mut buf);

        assert_eq!(s, "2001:db8::1");
    }

    #[test]
    fn test_http_versions_from_hyper() {
        assert_eq!(
            http_versions::from_hyper(hyper::Version::HTTP_10),
            "HTTP/1.0"
        );
        assert_eq!(
            http_versions::from_hyper(hyper::Version::HTTP_11),
            "HTTP/1.1"
        );
        assert_eq!(
            http_versions::from_hyper(hyper::Version::HTTP_2),
            "HTTP/2.0"
        );
        assert_eq!(
            http_versions::from_hyper(hyper::Version::HTTP_3),
            "HTTP/3.0"
        );
    }
}
