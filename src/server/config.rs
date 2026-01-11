//! Server configuration and TLS types.
//!
//! This module provides [`ServerConfig`] for configuring the HTTP server,
//! including listen address, document root, TLS, and various options.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::net::SocketAddr;
//! use std::time::Duration;
//! use tokio_php::server::ServerConfig;
//!
//! let config = ServerConfig::new("0.0.0.0:8080".parse().unwrap())
//!     .with_document_root("/var/www/html")
//!     .with_workers(4)
//!     .with_index_file("index.php".to_string())
//!     .with_drain_timeout(Duration::from_secs(30));
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

// Re-export unified types from config module
pub use crate::config::{OptionalDuration, RequestTimeout, StaticCacheTtl};

/// TLS connection information for profiling
#[derive(Clone, Default)]
pub struct TlsInfo {
    pub handshake_us: u64,
    pub protocol: String,
    pub alpn: String,
}

/// Server configuration.
///
/// Use the builder pattern to construct a configuration:
///
/// ```rust,ignore
/// let config = ServerConfig::new("0.0.0.0:8080".parse()?)
///     .with_document_root("/var/www/html")
///     .with_workers(4)
///     .with_tls("cert.pem".into(), "key.pem".into());
/// ```
///
/// # Environment Variables
///
/// When using [`crate::config::Config::from_env()`], these environment variables
/// are used to configure the server:
///
/// | Variable | Default | Description |
/// |----------|---------|-------------|
/// | `LISTEN_ADDR` | `0.0.0.0:8080` | Server bind address |
/// | `DOCUMENT_ROOT` | `/var/www/html` | Web root directory |
/// | `INDEX_FILE` | _(empty)_ | Single entry point mode |
/// | `TLS_CERT` | _(empty)_ | TLS certificate path |
/// | `TLS_KEY` | _(empty)_ | TLS private key path |
/// | `DRAIN_TIMEOUT_SECS` | `30` | Graceful shutdown timeout |
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
            static_cache_ttl: OptionalDuration::from_secs(86400), // 1 day
            request_timeout: OptionalDuration::from_secs(120),    // 2 minutes
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
