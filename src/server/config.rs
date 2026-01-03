//! Server configuration and TLS types.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// Static file cache TTL configuration.
/// - None: caching disabled ("off")
/// - Some(duration): cache for specified duration
#[derive(Clone, Debug)]
pub struct StaticCacheTtl(pub Option<Duration>);

impl StaticCacheTtl {
    /// Check if caching is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.0.is_some()
    }

    /// Get TTL in seconds (0 if disabled).
    #[inline]
    pub fn as_secs(&self) -> u64 {
        self.0.map(|d| d.as_secs()).unwrap_or(0)
    }
}

impl Default for StaticCacheTtl {
    fn default() -> Self {
        // Default: 1 day
        Self(Some(Duration::from_secs(86400)))
    }
}

/// Request timeout configuration.
/// - None: timeout disabled ("off")
/// - Some(duration): timeout after specified duration
#[derive(Clone, Debug)]
pub struct RequestTimeout(pub Option<Duration>);

impl RequestTimeout {
    /// Get timeout duration.
    #[inline]
    pub fn as_duration(&self) -> Option<Duration> {
        self.0
    }
}

impl Default for RequestTimeout {
    fn default() -> Self {
        // Default: 2 minutes
        Self(Some(Duration::from_secs(120)))
    }
}

/// TLS connection information for profiling
#[derive(Clone, Default)]
pub struct TlsInfo {
    pub handshake_us: u64,
    pub protocol: String,
    pub alpn: String,
}

/// Server configuration.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub document_root: Arc<str>,
    /// Number of accept loop workers. 0 = auto-detect from CPU cores.
    pub num_workers: usize,
    /// TLS certificate file path (PEM format)
    pub tls_cert: Option<String>,
    /// TLS private key file path (PEM format)
    pub tls_key: Option<String>,
    /// Index file for single entry point mode (e.g., "index.php")
    pub index_file: Option<String>,
    /// Internal server address for /health and /metrics
    pub internal_addr: Option<SocketAddr>,
    /// Directory with custom error pages ({status_code}.html)
    pub error_pages_dir: Option<String>,
    /// Graceful shutdown drain timeout
    pub drain_timeout: Duration,
    /// Static file cache TTL (default: 1d, "off" to disable)
    pub static_cache_ttl: StaticCacheTtl,
    /// Request timeout (default: 2m, "off" to disable)
    pub request_timeout: RequestTimeout,
}

impl ServerConfig {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            document_root: Arc::from("/var/www/html"),
            num_workers: 0,
            tls_cert: None,
            tls_key: None,
            index_file: None,
            internal_addr: None,
            error_pages_dir: None,
            drain_timeout: Duration::from_secs(30),
            static_cache_ttl: StaticCacheTtl::default(),
            request_timeout: RequestTimeout::default(),
        }
    }

    pub fn with_document_root(mut self, path: &str) -> Self {
        self.document_root = Arc::from(path);
        self
    }

    pub fn with_workers(mut self, num: usize) -> Self {
        self.num_workers = num;
        self
    }

    pub fn with_tls(mut self, cert_path: String, key_path: String) -> Self {
        self.tls_cert = Some(cert_path);
        self.tls_key = Some(key_path);
        self
    }

    pub fn with_index_file(mut self, index_file: String) -> Self {
        self.index_file = Some(index_file);
        self
    }

    pub fn with_internal_addr(mut self, addr: SocketAddr) -> Self {
        self.internal_addr = Some(addr);
        self
    }

    pub fn with_error_pages_dir(mut self, dir: String) -> Self {
        self.error_pages_dir = Some(dir);
        self
    }

    pub fn with_drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    pub fn with_static_cache_ttl(mut self, ttl: StaticCacheTtl) -> Self {
        self.static_cache_ttl = ttl;
        self
    }

    pub fn with_request_timeout(mut self, timeout: RequestTimeout) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub fn has_tls(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }
}
