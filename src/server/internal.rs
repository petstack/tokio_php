//! Internal HTTP server for health and metrics endpoints.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming as IncomingBody;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

/// Request counters by HTTP method and status code.
#[derive(Default)]
pub struct RequestMetrics {
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
}

impl RequestMetrics {
    pub fn new() -> Self {
        Self::default()
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
}

/// Guard that decrements pending_requests when dropped.
/// Ensures proper cleanup even if async task is cancelled.
pub struct PendingGuard(Arc<RequestMetrics>);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.0.dec_pending();
    }
}

/// Run the internal HTTP server for /health and /metrics endpoints.
pub async fn run_internal_server(
    addr: SocketAddr,
    active_connections: Arc<AtomicUsize>,
    request_metrics: Arc<RequestMetrics>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let connections = Arc::clone(&active_connections);
        let metrics = Arc::clone(&request_metrics);

        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let conns = connections.load(Ordering::Relaxed);
                let m = Arc::clone(&metrics);
                async move { handle_internal_request(req, conns, m).await }
            });

            let io = TokioIo::new(stream);
            let _ = http1::Builder::new().serve_connection(io, service).await;
        });
    }
}

/// Handle internal server requests (/health, /metrics).
async fn handle_internal_request(
    req: Request<IncomingBody>,
    active_connections: usize,
    metrics: Arc<RequestMetrics>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();

    let response = match path {
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
            let body = format!(
                "# HELP tokio_php_active_connections Current number of active connections\n\
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
                 tokio_php_responses_total{{status=\"5xx\"}} {}\n",
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
