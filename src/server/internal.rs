//! Internal HTTP server for health and metrics endpoints.

use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming as IncomingBody;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::net::TcpListener;

// =============================================================================
// Server Configuration Info (for /config endpoint)
// =============================================================================

/// Server configuration info for the /config endpoint.
/// Uses environment variable names as keys with their effective values.
#[derive(Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct ServerConfigInfo {
    pub listen_addr: String,
    pub document_root: String,
    pub php_workers: String,
    pub queue_capacity: String,
    pub index_file: String,
    pub internal_addr: String,
    pub error_pages_dir: String,
    pub drain_timeout_secs: String,
    pub static_cache_ttl: String,
    pub request_timeout: String,
    pub sse_timeout: String,
    pub access_log: String,
    pub rate_limit: String,
    pub rate_window: String,
    pub use_stub: String,
    pub use_ext: String,
    pub profile: String,
    pub tls_cert: String,
    pub tls_key: String,
    pub rust_log: String,
    pub service_name: String,
}

// =============================================================================
// System Metrics (CPU, Memory)
// =============================================================================

/// System metrics snapshot
#[derive(Default)]
pub struct SystemMetrics {
    /// Load average (1 minute)
    pub load_avg_1m: f64,
    /// Load average (5 minutes)
    pub load_avg_5m: f64,
    /// Load average (15 minutes)
    pub load_avg_15m: f64,
    /// Total memory in bytes
    pub memory_total_bytes: u64,
    /// Available memory in bytes
    pub memory_available_bytes: u64,
    /// Used memory in bytes (total - available)
    pub memory_used_bytes: u64,
    /// Memory usage percentage
    pub memory_usage_percent: f64,
}

impl SystemMetrics {
    /// Read current system metrics from /proc (Linux) or return defaults
    pub fn read() -> Self {
        let mut metrics = Self::default();

        // Read load average from /proc/loadavg
        if let Ok(content) = fs::read_to_string("/proc/loadavg") {
            let parts: Vec<&str> = content.split_whitespace().collect();
            if parts.len() >= 3 {
                metrics.load_avg_1m = parts[0].parse().unwrap_or(0.0);
                metrics.load_avg_5m = parts[1].parse().unwrap_or(0.0);
                metrics.load_avg_15m = parts[2].parse().unwrap_or(0.0);
            }
        }

        // Read memory info from /proc/meminfo
        if let Ok(content) = fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    metrics.memory_total_bytes = parse_meminfo_kb(line) * 1024;
                } else if line.starts_with("MemAvailable:") {
                    metrics.memory_available_bytes = parse_meminfo_kb(line) * 1024;
                }
            }
            if metrics.memory_total_bytes > 0 {
                metrics.memory_used_bytes = metrics
                    .memory_total_bytes
                    .saturating_sub(metrics.memory_available_bytes);
                metrics.memory_usage_percent =
                    (metrics.memory_used_bytes as f64 / metrics.memory_total_bytes as f64) * 100.0;
            }
        }

        metrics
    }
}

/// Parse a line like "MemTotal:       16384000 kB" and return the value in KB
fn parse_meminfo_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

// =============================================================================
// Request Metrics
// =============================================================================

/// Request counters by HTTP method and status code.
pub struct RequestMetrics {
    // Server start time for uptime/RPS calculation
    pub start_time: Instant,
    // By HTTP method
    pub get: AtomicUsize,
    pub post: AtomicUsize,
    pub head: AtomicUsize,
    pub put: AtomicUsize,
    pub delete: AtomicUsize,
    pub options: AtomicUsize,
    pub patch: AtomicUsize,
    pub other: AtomicUsize,
    // By status code class
    pub status_2xx: AtomicUsize,
    pub status_3xx: AtomicUsize,
    pub status_4xx: AtomicUsize,
    pub status_5xx: AtomicUsize,
    // Queue metrics
    pub pending_requests: AtomicUsize,
    pub dropped_requests: AtomicUsize,
    // Response time tracking (microseconds)
    pub total_response_time_us: AtomicU64,
    pub response_count: AtomicU64,
    // SSE metrics
    pub sse_active: AtomicUsize,
    pub sse_total: AtomicU64,
    pub sse_chunks: AtomicU64,
    pub sse_bytes: AtomicU64,
}

impl Default for RequestMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestMetrics {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            get: AtomicUsize::new(0),
            post: AtomicUsize::new(0),
            head: AtomicUsize::new(0),
            put: AtomicUsize::new(0),
            delete: AtomicUsize::new(0),
            options: AtomicUsize::new(0),
            patch: AtomicUsize::new(0),
            other: AtomicUsize::new(0),
            status_2xx: AtomicUsize::new(0),
            status_3xx: AtomicUsize::new(0),
            status_4xx: AtomicUsize::new(0),
            status_5xx: AtomicUsize::new(0),
            pending_requests: AtomicUsize::new(0),
            dropped_requests: AtomicUsize::new(0),
            total_response_time_us: AtomicU64::new(0),
            response_count: AtomicU64::new(0),
            sse_active: AtomicUsize::new(0),
            sse_total: AtomicU64::new(0),
            sse_chunks: AtomicU64::new(0),
            sse_bytes: AtomicU64::new(0),
        }
    }

    /// Increment counter for the given HTTP method.
    #[inline]
    pub fn increment_method(&self, method: &hyper::Method) {
        let counter = match *method {
            hyper::Method::GET => &self.get,
            hyper::Method::POST => &self.post,
            hyper::Method::HEAD => &self.head,
            hyper::Method::PUT => &self.put,
            hyper::Method::DELETE => &self.delete,
            hyper::Method::OPTIONS => &self.options,
            hyper::Method::PATCH => &self.patch,
            _ => &self.other,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment counter for the given HTTP status code.
    #[inline]
    pub fn increment_status(&self, status: u16) {
        let counter = match status {
            200..=299 => &self.status_2xx,
            300..=399 => &self.status_3xx,
            400..=499 => &self.status_4xx,
            _ => &self.status_5xx,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment pending requests (called when request enters queue).
    #[inline]
    pub fn inc_pending(&self) {
        self.pending_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement pending requests (called when worker picks up request).
    #[inline]
    pub fn dec_pending(&self) {
        self.pending_requests.fetch_sub(1, Ordering::Relaxed);
    }

    /// Increment dropped requests (called when queue is full).
    #[inline]
    pub fn inc_dropped(&self) {
        self.dropped_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Create a guard that tracks pending requests (decrements on drop).
    #[inline]
    pub fn pending_guard(metrics: &Arc<Self>) -> PendingGuard {
        metrics.inc_pending();
        PendingGuard(Arc::clone(metrics))
    }

    /// Get total requests count.
    pub fn total(&self) -> usize {
        self.get.load(Ordering::Relaxed)
            + self.post.load(Ordering::Relaxed)
            + self.head.load(Ordering::Relaxed)
            + self.put.load(Ordering::Relaxed)
            + self.delete.load(Ordering::Relaxed)
            + self.options.load(Ordering::Relaxed)
            + self.patch.load(Ordering::Relaxed)
            + self.other.load(Ordering::Relaxed)
    }

    /// Record response time in microseconds.
    #[inline]
    pub fn record_response_time(&self, duration_us: u64) {
        self.total_response_time_us
            .fetch_add(duration_us, Ordering::Relaxed);
        self.response_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get server uptime in seconds.
    pub fn uptime_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Get requests per second (RPS).
    pub fn rps(&self) -> f64 {
        let uptime = self.uptime_secs();
        if uptime > 0.0 {
            self.total() as f64 / uptime
        } else {
            0.0
        }
    }

    /// Get average response time in microseconds.
    pub fn avg_response_time_us(&self) -> f64 {
        let count = self.response_count.load(Ordering::Relaxed);
        if count > 0 {
            self.total_response_time_us.load(Ordering::Relaxed) as f64 / count as f64
        } else {
            0.0
        }
    }

    /// Increment active SSE connections (called when SSE stream starts).
    #[inline]
    pub fn sse_connection_started(&self) {
        self.sse_active.fetch_add(1, Ordering::Relaxed);
        self.sse_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active SSE connections (called when SSE stream ends).
    #[inline]
    pub fn sse_connection_ended(&self) {
        self.sse_active.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record SSE chunk sent.
    #[inline]
    pub fn sse_chunk_sent(&self, bytes: usize) {
        self.sse_chunks.fetch_add(1, Ordering::Relaxed);
        self.sse_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

/// Guard that decrements pending_requests when dropped.
/// Ensures proper cleanup even if async task is cancelled.
pub struct PendingGuard(Arc<RequestMetrics>);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.0.dec_pending();
    }
}

/// Run the internal HTTP server for /health, /metrics, and /config endpoints.
pub async fn run_internal_server(
    addr: SocketAddr,
    active_connections: Arc<AtomicUsize>,
    request_metrics: Arc<RequestMetrics>,
    config_info: Arc<ServerConfigInfo>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let connections = Arc::clone(&active_connections);
        let metrics = Arc::clone(&request_metrics);
        let config = Arc::clone(&config_info);

        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let conns = connections.load(Ordering::Relaxed);
                let m = Arc::clone(&metrics);
                let c = Arc::clone(&config);
                async move { handle_internal_request(req, conns, m, c).await }
            });

            let io = TokioIo::new(stream);
            let _ = http1::Builder::new().serve_connection(io, service).await;
        });
    }
}

/// Handle internal server requests (/health, /metrics, /config).
async fn handle_internal_request(
    req: Request<IncomingBody>,
    active_connections: usize,
    metrics: Arc<RequestMetrics>,
    config: Arc<ServerConfigInfo>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();

    let response = match path {
        "/config" => {
            let body = serde_json::to_string_pretty(&*config).unwrap_or_else(|_| "{}".to_string());
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        "/health" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let body = format!(
                r#"{{"status":"ok","timestamp":{},"active_connections":{},"total_requests":{}}}"#,
                now.as_secs(),
                active_connections,
                metrics.total()
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        "/metrics" => {
            let sys = SystemMetrics::read();
            let body = format!(
                "# HELP tokio_php_uptime_seconds Server uptime in seconds\n\
                 # TYPE tokio_php_uptime_seconds gauge\n\
                 tokio_php_uptime_seconds {:.3}\n\
                 \n\
                 # HELP tokio_php_requests_per_second Current requests per second (lifetime average)\n\
                 # TYPE tokio_php_requests_per_second gauge\n\
                 tokio_php_requests_per_second {:.2}\n\
                 \n\
                 # HELP tokio_php_response_time_avg_seconds Average response time in seconds\n\
                 # TYPE tokio_php_response_time_avg_seconds gauge\n\
                 tokio_php_response_time_avg_seconds {:.6}\n\
                 \n\
                 # HELP tokio_php_active_connections Current number of active connections\n\
                 # TYPE tokio_php_active_connections gauge\n\
                 tokio_php_active_connections {}\n\
                 \n\
                 # HELP tokio_php_pending_requests Requests waiting in queue for PHP worker\n\
                 # TYPE tokio_php_pending_requests gauge\n\
                 tokio_php_pending_requests {}\n\
                 \n\
                 # HELP tokio_php_dropped_requests Total requests dropped due to queue overflow\n\
                 # TYPE tokio_php_dropped_requests counter\n\
                 tokio_php_dropped_requests {}\n\
                 \n\
                 # HELP tokio_php_requests_total Total number of HTTP requests by method\n\
                 # TYPE tokio_php_requests_total counter\n\
                 tokio_php_requests_total{{method=\"GET\"}} {}\n\
                 tokio_php_requests_total{{method=\"POST\"}} {}\n\
                 tokio_php_requests_total{{method=\"HEAD\"}} {}\n\
                 tokio_php_requests_total{{method=\"PUT\"}} {}\n\
                 tokio_php_requests_total{{method=\"DELETE\"}} {}\n\
                 tokio_php_requests_total{{method=\"OPTIONS\"}} {}\n\
                 tokio_php_requests_total{{method=\"PATCH\"}} {}\n\
                 tokio_php_requests_total{{method=\"OTHER\"}} {}\n\
                 \n\
                 # HELP tokio_php_responses_total Total number of HTTP responses by status class\n\
                 # TYPE tokio_php_responses_total counter\n\
                 tokio_php_responses_total{{status=\"2xx\"}} {}\n\
                 tokio_php_responses_total{{status=\"3xx\"}} {}\n\
                 tokio_php_responses_total{{status=\"4xx\"}} {}\n\
                 tokio_php_responses_total{{status=\"5xx\"}} {}\n\
                 \n\
                 # HELP node_load1 1-minute load average\n\
                 # TYPE node_load1 gauge\n\
                 node_load1 {:.2}\n\
                 \n\
                 # HELP node_load5 5-minute load average\n\
                 # TYPE node_load5 gauge\n\
                 node_load5 {:.2}\n\
                 \n\
                 # HELP node_load15 15-minute load average\n\
                 # TYPE node_load15 gauge\n\
                 node_load15 {:.2}\n\
                 \n\
                 # HELP node_memory_MemTotal_bytes Total memory in bytes\n\
                 # TYPE node_memory_MemTotal_bytes gauge\n\
                 node_memory_MemTotal_bytes {}\n\
                 \n\
                 # HELP node_memory_MemAvailable_bytes Available memory in bytes\n\
                 # TYPE node_memory_MemAvailable_bytes gauge\n\
                 node_memory_MemAvailable_bytes {}\n\
                 \n\
                 # HELP node_memory_MemUsed_bytes Used memory in bytes\n\
                 # TYPE node_memory_MemUsed_bytes gauge\n\
                 node_memory_MemUsed_bytes {}\n\
                 \n\
                 # HELP tokio_php_memory_usage_percent Memory usage percentage\n\
                 # TYPE tokio_php_memory_usage_percent gauge\n\
                 tokio_php_memory_usage_percent {:.2}\n\
                 \n\
                 # HELP tokio_php_sse_active_connections Current active SSE connections\n\
                 # TYPE tokio_php_sse_active_connections gauge\n\
                 tokio_php_sse_active_connections {}\n\
                 \n\
                 # HELP tokio_php_sse_connections_total Total SSE connections\n\
                 # TYPE tokio_php_sse_connections_total counter\n\
                 tokio_php_sse_connections_total {}\n\
                 \n\
                 # HELP tokio_php_sse_chunks_total Total SSE chunks sent\n\
                 # TYPE tokio_php_sse_chunks_total counter\n\
                 tokio_php_sse_chunks_total {}\n\
                 \n\
                 # HELP tokio_php_sse_bytes_total Total SSE bytes sent\n\
                 # TYPE tokio_php_sse_bytes_total counter\n\
                 tokio_php_sse_bytes_total {}\n",
                metrics.uptime_secs(),
                metrics.rps(),
                metrics.avg_response_time_us() / 1_000_000.0, // convert us to seconds
                active_connections,
                metrics.pending_requests.load(Ordering::Relaxed),
                metrics.dropped_requests.load(Ordering::Relaxed),
                metrics.get.load(Ordering::Relaxed),
                metrics.post.load(Ordering::Relaxed),
                metrics.head.load(Ordering::Relaxed),
                metrics.put.load(Ordering::Relaxed),
                metrics.delete.load(Ordering::Relaxed),
                metrics.options.load(Ordering::Relaxed),
                metrics.patch.load(Ordering::Relaxed),
                metrics.other.load(Ordering::Relaxed),
                metrics.status_2xx.load(Ordering::Relaxed),
                metrics.status_3xx.load(Ordering::Relaxed),
                metrics.status_4xx.load(Ordering::Relaxed),
                metrics.status_5xx.load(Ordering::Relaxed),
                sys.load_avg_1m,
                sys.load_avg_5m,
                sys.load_avg_15m,
                sys.memory_total_bytes,
                sys.memory_available_bytes,
                sys.memory_used_bytes,
                sys.memory_usage_percent,
                metrics.sse_active.load(Ordering::Relaxed),
                metrics.sse_total.load(Ordering::Relaxed),
                metrics.sse_chunks.load(Ordering::Relaxed),
                metrics.sse_bytes.load(Ordering::Relaxed),
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4")
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/plain")
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap(),
    };

    Ok(response)
}
