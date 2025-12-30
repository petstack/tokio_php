//! Server builder for constructing servers with a fluent API.
//!
//! # Example
//!
//! ```ignore
//! use tokio_php::server::ServerBuilder;
//!
//! let server = ServerBuilder::new()
//!     .bind("127.0.0.1:8080".parse()?)
//!     .document_root("/var/www/html")
//!     .workers(4)
//!     .index_file("index.php")
//!     .build(executor)?;
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::executor::ScriptExecutor;

use super::config::{RequestTimeout, ServerConfig, StaticCacheTtl};
use super::Server;

/// Builder for creating server instances with a fluent API.
///
/// Provides a clean, readable way to configure all server options
/// before constructing the server.
#[derive(Default)]
pub struct ServerBuilder {
    addr: Option<SocketAddr>,
    document_root: Option<String>,
    num_workers: usize,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    index_file: Option<String>,
    internal_addr: Option<SocketAddr>,
    error_pages_dir: Option<String>,
    drain_timeout: Duration,
    static_cache_ttl: StaticCacheTtl,
    request_timeout: RequestTimeout,
}

impl ServerBuilder {
    /// Create a new server builder with default settings.
    pub fn new() -> Self {
        Self {
            addr: None,
            document_root: None,
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

    /// Set the address to bind to.
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.addr = Some(addr);
        self
    }

    /// Set the document root directory.
    pub fn document_root(mut self, path: impl Into<String>) -> Self {
        self.document_root = Some(path.into());
        self
    }

    /// Set the number of accept workers.
    /// 0 = auto-detect from CPU cores.
    pub fn workers(mut self, num: usize) -> Self {
        self.num_workers = num;
        self
    }

    /// Enable TLS with the given certificate and key files.
    pub fn tls(mut self, cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        self.tls_cert = Some(cert_path.into());
        self.tls_key = Some(key_path.into());
        self
    }

    /// Set the index file for single entry point mode.
    pub fn index_file(mut self, file: impl Into<String>) -> Self {
        self.index_file = Some(file.into());
        self
    }

    /// Set the internal server address for /health and /metrics.
    pub fn internal_addr(mut self, addr: SocketAddr) -> Self {
        self.internal_addr = Some(addr);
        self
    }

    /// Set the directory containing custom error pages.
    pub fn error_pages_dir(mut self, dir: impl Into<String>) -> Self {
        self.error_pages_dir = Some(dir.into());
        self
    }

    /// Set the graceful shutdown drain timeout.
    pub fn drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    /// Set the drain timeout in seconds.
    pub fn drain_timeout_secs(mut self, secs: u64) -> Self {
        self.drain_timeout = Duration::from_secs(secs);
        self
    }

    /// Set the static file cache TTL.
    pub fn static_cache_ttl(mut self, ttl: StaticCacheTtl) -> Self {
        self.static_cache_ttl = ttl;
        self
    }

    /// Set the request timeout.
    pub fn request_timeout(mut self, timeout: RequestTimeout) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Set the request timeout as a Duration.
    pub fn request_timeout_duration(mut self, timeout: Option<Duration>) -> Self {
        self.request_timeout = RequestTimeout(timeout);
        self
    }

    /// Build the server config (without an executor).
    ///
    /// Use this when you want to inspect or modify the config before
    /// creating the server.
    pub fn build_config(self) -> Result<ServerConfig, BuildError> {
        let addr = self.addr.ok_or(BuildError::MissingAddress)?;

        let document_root = self
            .document_root
            .unwrap_or_else(|| "/var/www/html".to_string());

        let mut config = ServerConfig::new(addr)
            .with_document_root(&document_root)
            .with_workers(self.num_workers)
            .with_drain_timeout(self.drain_timeout)
            .with_static_cache_ttl(self.static_cache_ttl)
            .with_request_timeout(self.request_timeout);

        if let (Some(cert), Some(key)) = (self.tls_cert, self.tls_key) {
            config = config.with_tls(cert, key);
        }

        if let Some(index) = self.index_file {
            config = config.with_index_file(index);
        }

        if let Some(addr) = self.internal_addr {
            config = config.with_internal_addr(addr);
        }

        if let Some(dir) = self.error_pages_dir {
            config = config.with_error_pages_dir(dir);
        }

        Ok(config)
    }

    /// Build the server with the given executor.
    pub fn build<E: ScriptExecutor + 'static>(
        self,
        executor: E,
    ) -> Result<Server<E>, BuildError> {
        let config = self.build_config()?;
        Server::new(config, executor).map_err(|e| BuildError::ServerCreation(e.to_string()))
    }
}

/// Errors that can occur when building a server.
#[derive(Debug, Clone)]
pub enum BuildError {
    /// The bind address was not specified.
    MissingAddress,
    /// Server creation failed.
    ServerCreation(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::MissingAddress => {
                write!(f, "bind address not specified")
            }
            BuildError::ServerCreation(msg) => {
                write!(f, "failed to create server: {}", msg)
            }
        }
    }
}

impl std::error::Error for BuildError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_builder_new() {
        let builder = ServerBuilder::new();
        assert!(builder.addr.is_none());
        assert!(builder.document_root.is_none());
        assert_eq!(builder.num_workers, 0);
    }

    #[test]
    fn test_builder_bind() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let builder = ServerBuilder::new().bind(addr);
        assert_eq!(builder.addr, Some(addr));
    }

    #[test]
    fn test_builder_document_root() {
        let builder = ServerBuilder::new().document_root("/var/www");
        assert_eq!(builder.document_root, Some("/var/www".to_string()));
    }

    #[test]
    fn test_builder_workers() {
        let builder = ServerBuilder::new().workers(4);
        assert_eq!(builder.num_workers, 4);
    }

    #[test]
    fn test_builder_tls() {
        let builder = ServerBuilder::new().tls("cert.pem", "key.pem");
        assert_eq!(builder.tls_cert, Some("cert.pem".to_string()));
        assert_eq!(builder.tls_key, Some("key.pem".to_string()));
    }

    #[test]
    fn test_builder_index_file() {
        let builder = ServerBuilder::new().index_file("index.php");
        assert_eq!(builder.index_file, Some("index.php".to_string()));
    }

    #[test]
    fn test_builder_internal_addr() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090);
        let builder = ServerBuilder::new().internal_addr(addr);
        assert_eq!(builder.internal_addr, Some(addr));
    }

    #[test]
    fn test_builder_error_pages_dir() {
        let builder = ServerBuilder::new().error_pages_dir("/var/www/errors");
        assert_eq!(builder.error_pages_dir, Some("/var/www/errors".to_string()));
    }

    #[test]
    fn test_builder_drain_timeout() {
        let builder = ServerBuilder::new().drain_timeout(Duration::from_secs(60));
        assert_eq!(builder.drain_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_builder_drain_timeout_secs() {
        let builder = ServerBuilder::new().drain_timeout_secs(45);
        assert_eq!(builder.drain_timeout, Duration::from_secs(45));
    }

    #[test]
    fn test_builder_build_config_success() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let config = ServerBuilder::new()
            .bind(addr)
            .document_root("/var/www")
            .workers(4)
            .build_config()
            .unwrap();

        assert_eq!(config.addr, addr);
        assert_eq!(config.document_root.as_ref(), "/var/www");
        assert_eq!(config.num_workers, 4);
    }

    #[test]
    fn test_builder_build_config_missing_address() {
        let result = ServerBuilder::new().build_config();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BuildError::MissingAddress));
    }

    #[test]
    fn test_builder_build_config_default_document_root() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let config = ServerBuilder::new().bind(addr).build_config().unwrap();

        assert_eq!(config.document_root.as_ref(), "/var/www/html");
    }

    #[test]
    fn test_builder_fluent_chain() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let internal = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090);

        let config = ServerBuilder::new()
            .bind(addr)
            .document_root("/var/www")
            .workers(8)
            .tls("cert.pem", "key.pem")
            .index_file("index.php")
            .internal_addr(internal)
            .error_pages_dir("/var/www/errors")
            .drain_timeout_secs(60)
            .build_config()
            .unwrap();

        assert_eq!(config.addr, addr);
        assert_eq!(config.document_root.as_ref(), "/var/www");
        assert_eq!(config.num_workers, 8);
        assert!(config.has_tls());
        assert_eq!(config.index_file, Some("index.php".to_string()));
        assert_eq!(config.internal_addr, Some(internal));
        assert_eq!(config.error_pages_dir, Some("/var/www/errors".to_string()));
        assert_eq!(config.drain_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_build_error_display() {
        let err = BuildError::MissingAddress;
        assert_eq!(err.to_string(), "bind address not specified");

        let err = BuildError::ServerCreation("test error".to_string());
        assert!(err.to_string().contains("test error"));
    }
}
