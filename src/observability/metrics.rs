//! Prometheus metrics for tokio_php.
//!
//! Implements RED methodology metrics (Rate, Errors, Duration) for
//! HTTP requests, PHP execution, and system resources.

use prometheus::{
    Counter, CounterVec, Encoder, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, Opts,
    Registry, TextEncoder,
};
use regex::Regex;
use std::sync::OnceLock;

/// Global regex for path normalization (compiled once)
static PATH_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_path_regex() -> &'static Regex {
    PATH_REGEX.get_or_init(|| Regex::new(r"/\d+(/|$)").expect("Invalid regex"))
}

/// Prometheus metrics registry with all application metrics.
///
/// Follows RED methodology:
/// - **R**ate: Request throughput (requests/second)
/// - **E**rrors: Error rate (5xx responses)
/// - **D**uration: Latency distribution (histograms)
pub struct Metrics {
    registry: Registry,

    // === HTTP Metrics ===
    /// Total HTTP requests by method, path, status
    pub http_requests_total: CounterVec,

    /// HTTP request duration in seconds
    pub http_request_duration_seconds: HistogramVec,

    /// HTTP request body size in bytes
    pub http_request_size_bytes: HistogramVec,

    /// HTTP response body size in bytes
    pub http_response_size_bytes: HistogramVec,

    /// Active HTTP connections
    pub http_connections_active: Gauge,

    // === PHP Executor Metrics ===
    /// PHP script executions by script, status
    pub php_executions_total: CounterVec,

    /// PHP execution duration in seconds
    pub php_execution_duration_seconds: HistogramVec,

    /// PHP startup time in seconds
    pub php_startup_duration_seconds: Histogram,

    /// PHP shutdown time in seconds
    pub php_shutdown_duration_seconds: Histogram,

    /// Superglobals injection time in seconds
    pub php_superglobals_duration_seconds: Histogram,

    /// Worker queue depth
    pub php_queue_depth: Gauge,

    /// Worker queue capacity
    pub php_queue_capacity: Gauge,

    /// Busy workers count
    pub php_workers_busy: Gauge,

    /// Total workers count
    pub php_workers_total: Gauge,

    // === OPcache Metrics ===
    /// OPcache hits
    pub opcache_hits_total: Counter,

    /// OPcache misses
    pub opcache_misses_total: Counter,

    /// OPcache memory usage in bytes
    pub opcache_memory_used_bytes: Gauge,

    /// OPcache cached scripts count
    pub opcache_cached_scripts: Gauge,

    // === System Metrics ===
    /// Process memory usage in bytes
    pub process_memory_bytes: GaugeVec,

    /// Process CPU usage (0.0 - 1.0)
    pub process_cpu_usage: Gauge,

    /// Process open file descriptors
    pub process_open_fds: Gauge,

    /// Process uptime in seconds
    pub process_uptime_seconds: Gauge,

    // === gRPC Metrics ===
    #[cfg(feature = "grpc")]
    /// gRPC requests by method, status
    pub grpc_requests_total: CounterVec,

    #[cfg(feature = "grpc")]
    /// gRPC request duration in seconds
    pub grpc_request_duration_seconds: HistogramVec,
}

impl Metrics {
    /// Create a new metrics registry with all metrics.
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        // HTTP latency buckets (in seconds)
        let http_buckets = vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];

        // PHP execution buckets (in seconds)
        let php_buckets = vec![
            0.0001, 0.0005, 0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
        ];

        // PHP internal timing buckets (microsecond-scale, in seconds)
        let php_internal_buckets = vec![0.00001, 0.00005, 0.0001, 0.0005, 0.001, 0.005, 0.01];

        // Size buckets (in bytes)
        let size_buckets = vec![100.0, 1000.0, 10000.0, 100000.0, 1000000.0, 10000000.0];

        // HTTP metrics
        let http_requests_total = CounterVec::new(
            Opts::new("tokio_php_http_requests_total", "Total HTTP requests"),
            &["method", "path", "status"],
        )?;
        registry.register(Box::new(http_requests_total.clone()))?;

        let http_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "tokio_php_http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(http_buckets.clone()),
            &["method", "path"],
        )?;
        registry.register(Box::new(http_request_duration_seconds.clone()))?;

        let http_request_size_bytes = HistogramVec::new(
            HistogramOpts::new(
                "tokio_php_http_request_size_bytes",
                "HTTP request body size in bytes",
            )
            .buckets(size_buckets.clone()),
            &["method"],
        )?;
        registry.register(Box::new(http_request_size_bytes.clone()))?;

        let http_response_size_bytes = HistogramVec::new(
            HistogramOpts::new(
                "tokio_php_http_response_size_bytes",
                "HTTP response body size in bytes",
            )
            .buckets(size_buckets),
            &["method", "status"],
        )?;
        registry.register(Box::new(http_response_size_bytes.clone()))?;

        let http_connections_active = Gauge::new(
            "tokio_php_http_connections_active",
            "Active HTTP connections",
        )?;
        registry.register(Box::new(http_connections_active.clone()))?;

        // PHP execution metrics
        let php_executions_total = CounterVec::new(
            Opts::new(
                "tokio_php_php_executions_total",
                "Total PHP script executions",
            ),
            &["script", "status"],
        )?;
        registry.register(Box::new(php_executions_total.clone()))?;

        let php_execution_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "tokio_php_php_execution_duration_seconds",
                "PHP script execution duration in seconds",
            )
            .buckets(php_buckets),
            &["script"],
        )?;
        registry.register(Box::new(php_execution_duration_seconds.clone()))?;

        let php_startup_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "tokio_php_php_startup_duration_seconds",
                "PHP request startup duration in seconds",
            )
            .buckets(php_internal_buckets.clone()),
        )?;
        registry.register(Box::new(php_startup_duration_seconds.clone()))?;

        let php_shutdown_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "tokio_php_php_shutdown_duration_seconds",
                "PHP request shutdown duration in seconds",
            )
            .buckets(php_internal_buckets.clone()),
        )?;
        registry.register(Box::new(php_shutdown_duration_seconds.clone()))?;

        let php_superglobals_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "tokio_php_php_superglobals_duration_seconds",
                "PHP superglobals injection duration in seconds",
            )
            .buckets(php_internal_buckets),
        )?;
        registry.register(Box::new(php_superglobals_duration_seconds.clone()))?;

        let php_queue_depth = Gauge::new("tokio_php_php_queue_depth", "Current queue depth")?;
        registry.register(Box::new(php_queue_depth.clone()))?;

        let php_queue_capacity = Gauge::new("tokio_php_php_queue_capacity", "Queue capacity")?;
        registry.register(Box::new(php_queue_capacity.clone()))?;

        let php_workers_busy = Gauge::new("tokio_php_php_workers_busy", "Number of busy workers")?;
        registry.register(Box::new(php_workers_busy.clone()))?;

        let php_workers_total =
            Gauge::new("tokio_php_php_workers_total", "Total number of workers")?;
        registry.register(Box::new(php_workers_total.clone()))?;

        // OPcache metrics
        let opcache_hits_total = Counter::new("tokio_php_opcache_hits_total", "OPcache hits")?;
        registry.register(Box::new(opcache_hits_total.clone()))?;

        let opcache_misses_total =
            Counter::new("tokio_php_opcache_misses_total", "OPcache misses")?;
        registry.register(Box::new(opcache_misses_total.clone()))?;

        let opcache_memory_used_bytes = Gauge::new(
            "tokio_php_opcache_memory_used_bytes",
            "OPcache memory usage",
        )?;
        registry.register(Box::new(opcache_memory_used_bytes.clone()))?;

        let opcache_cached_scripts = Gauge::new(
            "tokio_php_opcache_cached_scripts",
            "Number of cached scripts",
        )?;
        registry.register(Box::new(opcache_cached_scripts.clone()))?;

        // System metrics
        let process_memory_bytes = GaugeVec::new(
            Opts::new("tokio_php_process_memory_bytes", "Process memory usage"),
            &["type"],
        )?;
        registry.register(Box::new(process_memory_bytes.clone()))?;

        let process_cpu_usage =
            Gauge::new("tokio_php_process_cpu_usage", "Process CPU usage (0.0-1.0)")?;
        registry.register(Box::new(process_cpu_usage.clone()))?;

        let process_open_fds = Gauge::new("tokio_php_process_open_fds", "Open file descriptors")?;
        registry.register(Box::new(process_open_fds.clone()))?;

        let process_uptime_seconds = Gauge::new(
            "tokio_php_process_uptime_seconds",
            "Process uptime in seconds",
        )?;
        registry.register(Box::new(process_uptime_seconds.clone()))?;

        // gRPC metrics (only if grpc feature enabled)
        #[cfg(feature = "grpc")]
        let grpc_requests_total = {
            let m = CounterVec::new(
                Opts::new("tokio_php_grpc_requests_total", "Total gRPC requests"),
                &["method", "status"],
            )?;
            registry.register(Box::new(m.clone()))?;
            m
        };

        #[cfg(feature = "grpc")]
        let grpc_request_duration_seconds = {
            let m = HistogramVec::new(
                HistogramOpts::new(
                    "tokio_php_grpc_request_duration_seconds",
                    "gRPC request duration in seconds",
                )
                .buckets(http_buckets),
                &["method"],
            )?;
            registry.register(Box::new(m.clone()))?;
            m
        };

        Ok(Self {
            registry,
            http_requests_total,
            http_request_duration_seconds,
            http_request_size_bytes,
            http_response_size_bytes,
            http_connections_active,
            php_executions_total,
            php_execution_duration_seconds,
            php_startup_duration_seconds,
            php_shutdown_duration_seconds,
            php_superglobals_duration_seconds,
            php_queue_depth,
            php_queue_capacity,
            php_workers_busy,
            php_workers_total,
            opcache_hits_total,
            opcache_misses_total,
            opcache_memory_used_bytes,
            opcache_cached_scripts,
            process_memory_bytes,
            process_cpu_usage,
            process_open_fds,
            process_uptime_seconds,
            #[cfg(feature = "grpc")]
            grpc_requests_total,
            #[cfg(feature = "grpc")]
            grpc_request_duration_seconds,
        })
    }

    /// Record HTTP request metrics.
    pub fn record_http_request(
        &self,
        method: &str,
        path: &str,
        status: u16,
        duration_secs: f64,
        request_size: usize,
        response_size: usize,
    ) {
        let status_str = status.to_string();
        let path_normalized = normalize_path(path);

        self.http_requests_total
            .with_label_values(&[method, &path_normalized, &status_str])
            .inc();

        self.http_request_duration_seconds
            .with_label_values(&[method, &path_normalized])
            .observe(duration_secs);

        self.http_request_size_bytes
            .with_label_values(&[method])
            .observe(request_size as f64);

        self.http_response_size_bytes
            .with_label_values(&[method, &status_str])
            .observe(response_size as f64);
    }

    /// Record PHP execution metrics.
    pub fn record_php_execution(
        &self,
        script: &str,
        success: bool,
        duration_secs: f64,
        startup_secs: f64,
        shutdown_secs: f64,
        superglobals_secs: f64,
    ) {
        let status = if success { "success" } else { "error" };
        let script_normalized = normalize_script_path(script);

        self.php_executions_total
            .with_label_values(&[&script_normalized, status])
            .inc();

        self.php_execution_duration_seconds
            .with_label_values(&[&script_normalized])
            .observe(duration_secs);

        self.php_startup_duration_seconds.observe(startup_secs);
        self.php_shutdown_duration_seconds.observe(shutdown_secs);
        self.php_superglobals_duration_seconds
            .observe(superglobals_secs);
    }

    /// Update queue metrics.
    pub fn update_queue_metrics(&self, depth: usize, capacity: usize) {
        self.php_queue_depth.set(depth as f64);
        self.php_queue_capacity.set(capacity as f64);
    }

    /// Update worker metrics.
    pub fn update_worker_metrics(&self, busy: usize, total: usize) {
        self.php_workers_busy.set(busy as f64);
        self.php_workers_total.set(total as f64);
    }

    /// Increment active connections.
    pub fn inc_connections(&self) {
        self.http_connections_active.inc();
    }

    /// Decrement active connections.
    pub fn dec_connections(&self) {
        self.http_connections_active.dec();
    }

    /// Update process memory metrics.
    pub fn update_memory(&self, virtual_bytes: u64, resident_bytes: u64) {
        self.process_memory_bytes
            .with_label_values(&["virtual"])
            .set(virtual_bytes as f64);
        self.process_memory_bytes
            .with_label_values(&["resident"])
            .set(resident_bytes as f64);
    }

    /// Update process uptime.
    pub fn update_uptime(&self, seconds: f64) {
        self.process_uptime_seconds.set(seconds);
    }

    /// Update open file descriptors count.
    pub fn update_open_fds(&self, count: usize) {
        self.process_open_fds.set(count as f64);
    }

    /// Record gRPC request metrics (if grpc feature enabled).
    #[cfg(feature = "grpc")]
    pub fn record_grpc_request(&self, method: &str, status: &str, duration_secs: f64) {
        self.grpc_requests_total
            .with_label_values(&[method, status])
            .inc();
        self.grpc_request_duration_seconds
            .with_label_values(&[method])
            .observe(duration_secs);
    }

    /// Export metrics in Prometheus text format.
    pub fn export(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("Failed to encode metrics");
        String::from_utf8(buffer).expect("Invalid UTF-8 in metrics")
    }

    /// Get the Prometheus registry (for custom metrics).
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Failed to create metrics")
    }
}

/// Normalize path for metrics (replace IDs with placeholders).
///
/// Examples:
/// - `/users/123` -> `/users/:id`
/// - `/users/123/posts/456` -> `/users/:id/posts/:id`
fn normalize_path(path: &str) -> String {
    get_path_regex().replace_all(path, "/:id$1").to_string()
}

/// Normalize script path for metrics (extract filename only).
///
/// Examples:
/// - `/var/www/html/index.php` -> `index.php`
/// - `api/users.php` -> `users.php`
fn normalize_script_path(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new().expect("Should create metrics");
        assert!(metrics.export().contains("# HELP"));
    }

    #[test]
    fn test_http_request_recording() {
        let metrics = Metrics::new().expect("Should create metrics");
        metrics.record_http_request("GET", "/api/users", 200, 0.1, 100, 1000);

        let output = metrics.export();
        assert!(output.contains("tokio_php_http_requests_total"));
        assert!(output.contains("method=\"GET\""));
    }

    #[test]
    fn test_path_normalization() {
        assert_eq!(normalize_path("/users/123"), "/users/:id");
        assert_eq!(normalize_path("/users/123/posts"), "/users/:id/posts");
        assert_eq!(
            normalize_path("/users/123/posts/456"),
            "/users/:id/posts/:id"
        );
        assert_eq!(normalize_path("/api/v1/users"), "/api/v1/users");
    }

    #[test]
    fn test_script_path_normalization() {
        assert_eq!(
            normalize_script_path("/var/www/html/index.php"),
            "index.php"
        );
        assert_eq!(normalize_script_path("api/users.php"), "users.php");
        assert_eq!(normalize_script_path("index.php"), "index.php");
    }

    #[test]
    fn test_php_execution_recording() {
        let metrics = Metrics::new().expect("Should create metrics");
        metrics.record_php_execution("index.php", true, 0.05, 0.001, 0.0005, 0.0002);

        let output = metrics.export();
        assert!(output.contains("tokio_php_php_executions_total"));
        assert!(output.contains("status=\"success\""));
    }
}
