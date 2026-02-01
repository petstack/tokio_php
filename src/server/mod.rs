//! HTTP server with pluggable script executor.
//!
//! This module provides the main [`Server`] type that handles HTTP requests
//! and delegates script execution to a pluggable [`ScriptExecutor`].
//!
//! # Features
//!
//! - **HTTP/1.1 and HTTP/2** - Full protocol support with automatic detection
//! - **TLS/HTTPS** - TLS 1.3 with ALPN negotiation
//! - **Graceful Shutdown** - Connection draining with configurable timeout
//! - **Rate Limiting** - Per-IP request limiting
//! - **Static File Serving** - With Brotli compression and cache headers
//! - **Custom Error Pages** - HTML error pages for 4xx/5xx responses
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::server::{Server, ServerConfig};
//! use tokio_php::executor::ExtExecutor;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ServerConfig::default();
//!     let executor = ExtExecutor::new(4, 400)?;
//!
//!     let server = Server::new(config, executor)?
//!         .with_access_log_enabled(true)
//!         .with_profile_enabled(true);
//!
//!     server.run().await
//! }
//! ```
//!
//! # Graceful Shutdown
//!
//! The server supports graceful shutdown via [`Server::trigger_shutdown`]:
//!
//! ```rust,ignore
//! // Trigger shutdown
//! server.trigger_shutdown();
//!
//! // Wait for connections to drain (with timeout)
//! server.wait_for_drain(Duration::from_secs(30)).await;
//! ```
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                     Server                          │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
//! │  │  Worker 0   │  │  Worker 1   │  │  Worker N   │  │
//! │  │ (SO_REUSEPORT) │ (SO_REUSEPORT) │ (SO_REUSEPORT) │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  │
//! │         │                │                │         │
//! │         ▼                ▼                ▼         │
//! │  ┌─────────────────────────────────────────────┐    │
//! │  │           ConnectionContext                 │    │
//! │  │  • Rate limiting  • Static file serving     │    │
//! │  │  • Request parsing • Response compression   │    │
//! │  └─────────────────────┬───────────────────────┘    │
//! │                        │                            │
//! │                        ▼                            │
//! │  ┌─────────────────────────────────────────────┐    │
//! │  │           ScriptExecutor                    │    │
//! │  │  (ExtExecutor / PhpExecutor / StubExecutor) │    │
//! │  └─────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────┘
//! ```

pub mod access_log;
pub mod config;
pub mod connection;
pub mod error_pages;
pub mod file_cache;
mod internal;
pub mod request;
pub mod response;
mod routing;

use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use socket2::{Domain, Protocol, SockRef, Socket, TcpKeepalive, Type};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::ServerConfig as RustlsConfig;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};

pub use config::ServerConfig;
use connection::ConnectionContext;
use error_pages::ErrorPages;
use file_cache::FileCache;
use internal::{run_internal_server, RequestMetrics, ServerConfigInfo};
use routing::RouteConfig;

use crate::config::RateLimitConfig;
use crate::executor::ScriptExecutor;
use crate::health::HealthChecker;
use crate::middleware::rate_limit::RateLimiter;

/// HTTP server with pluggable script executor.
///
/// The server is generic over [`ScriptExecutor`],
/// allowing different backends for script execution.
///
/// # Type Parameter
///
/// * `E` - The script executor type (e.g., `ExtExecutor`, `PhpExecutor`, `StubExecutor`)
///
/// # Example
///
/// ```rust,ignore
/// use tokio_php::server::{Server, ServerConfig};
/// use tokio_php::executor::ExtExecutor;
///
/// // Create server with ExtExecutor
/// let config = ServerConfig::default();
/// let executor = ExtExecutor::new(4, 400)?;
/// let server = Server::new(config, executor)?;
///
/// // Run the server
/// server.run().await?;
/// ```
pub struct Server<E: ScriptExecutor> {
    config: ServerConfig,
    executor: Arc<E>,
    tls_acceptor: Option<TlsAcceptor>,
    /// Route configuration (INDEX_FILE handling)
    route_config: Arc<RouteConfig>,
    /// Active connections counter
    active_connections: Arc<AtomicUsize>,
    /// Request metrics by HTTP method
    request_metrics: Arc<RequestMetrics>,
    /// Health checker for Kubernetes probes
    health_checker: Arc<HealthChecker>,
    /// Cached custom error pages
    error_pages: ErrorPages,
    /// Per-IP rate limiter
    rate_limiter: Option<Arc<RateLimiter>>,
    /// File cache (LRU, max 200 entries)
    file_cache: Arc<FileCache>,
    /// Cached document root as static str (zero allocation per request)
    document_root_static: std::borrow::Cow<'static, str>,
    /// Shutdown signal sender
    shutdown_tx: watch::Sender<bool>,
    /// Shutdown signal receiver (cloneable)
    shutdown_rx: watch::Receiver<bool>,
    /// Shutdown initiated flag
    shutdown_initiated: Arc<AtomicBool>,
    /// Profiling enabled (compile-time with debug-profile feature)
    profile_enabled: bool,
    /// Access logging enabled (ACCESS_LOG=1)
    access_log_enabled: bool,
}

impl<E: ScriptExecutor + 'static> Server<E> {
    /// Create a new server with the given configuration and executor.
    pub fn new(
        config: ServerConfig,
        executor: E,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create route configuration
        let route_config = RouteConfig::new(&config.document_root, config.index_file.as_deref());

        // Validate index file at startup if configured
        if let Some(ref index_file_path) = route_config.index_file_path {
            if !Path::new(index_file_path.as_ref()).exists() {
                return Err(format!(
                    "Index file not found: {} (INDEX_FILE={})",
                    index_file_path,
                    route_config
                        .index_file
                        .as_ref()
                        .map(|s| s.as_ref())
                        .unwrap_or("")
                )
                .into());
            }
            info!(
                "Single entry point mode: all requests -> {}",
                route_config
                    .index_file
                    .as_ref()
                    .map(|s| s.as_ref())
                    .unwrap_or("")
            );
        } else {
            // Warn if no index.php exists in document root (common misconfiguration)
            if !executor.skip_file_check() {
                let index_path = format!("{}/index.php", config.document_root);
                if !Path::new(&index_path).exists() {
                    debug!(
                        "No index.php in document root: {}. Traditional mode: index.php -> index.html -> 404",
                        config.document_root
                    );
                }
            }
        }

        let tls_acceptor = if config.has_tls() {
            match Self::load_tls_config(&config) {
                Ok(tls_config) => Some(TlsAcceptor::from(Arc::new(tls_config))),
                Err(e) => {
                    warn!("Failed to load TLS config: {}. Running without TLS.", e);
                    None
                }
            }
        } else {
            None
        };

        // Load custom error pages if configured
        let error_pages = if let Some(ref dir) = config.error_pages_dir {
            ErrorPages::from_directory(dir)
        } else {
            ErrorPages::new()
        };

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Leak document_root to get 'static lifetime (lives for entire process)
        // This avoids string allocation on every request for $_SERVER['DOCUMENT_ROOT']
        let document_root_static: std::borrow::Cow<'static, str> = std::borrow::Cow::Borrowed(
            Box::leak(config.document_root.to_string().into_boxed_str()),
        );

        // Create health checker for Kubernetes probes
        let active_connections = Arc::new(AtomicUsize::new(0));
        let queue_capacity = config.num_workers * 100; // Default capacity
        let health_checker = Arc::new(HealthChecker::new(
            config.num_workers,
            queue_capacity,
            Arc::clone(&active_connections),
        ));

        Ok(Self {
            config,
            executor: Arc::new(executor),
            tls_acceptor,
            route_config: Arc::new(route_config),
            active_connections,
            request_metrics: Arc::new(RequestMetrics::new()),
            health_checker,
            error_pages,
            rate_limiter: None,
            file_cache: Arc::new(FileCache::new()),
            document_root_static,
            shutdown_tx,
            shutdown_rx,
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
            profile_enabled: false,
            access_log_enabled: false,
        })
    }

    /// Enable profiling for this server.
    ///
    /// Note: With `debug-profile` feature, profiling is always enabled at compile time.
    /// This method is kept for API compatibility but the `enabled` parameter is ignored.
    #[allow(unused_variables)]
    pub fn with_profile_enabled(mut self, enabled: bool) -> Self {
        self.profile_enabled = enabled;
        #[cfg(feature = "debug-profile")]
        info!("Profiler enabled (debug-profile build)");
        self
    }

    /// Enable access logging for this server.
    pub fn with_access_log_enabled(mut self, enabled: bool) -> Self {
        self.access_log_enabled = enabled;
        if enabled {
            info!("Access logging enabled (ACCESS_LOG=1)");
        }
        self
    }

    /// Configure rate limiting for this server.
    pub fn with_rate_limiter(mut self, config: Option<RateLimitConfig>) -> Self {
        if let Some(rl) = config {
            let limiter = RateLimiter::new(rl.limit(), rl.window_secs());
            info!(
                "Rate limiting enabled: {} requests per {} seconds per IP",
                limiter.limit(),
                limiter.window_secs()
            );
            self.rate_limiter = Some(Arc::new(limiter));
        }
        self
    }

    /// Get current active connections count.
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    fn load_tls_config(
        config: &ServerConfig,
    ) -> Result<RustlsConfig, Box<dyn std::error::Error + Send + Sync>> {
        let cert_path = config.tls_cert.as_ref().ok_or("TLS cert path not set")?;
        let key_path = config.tls_key.as_ref().ok_or("TLS key path not set")?;

        // Load certificate chain
        let cert_file = std::fs::File::open(cert_path)?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
            .filter_map(|r| r.ok())
            .collect();

        if certs.is_empty() {
            return Err("No certificates found in cert file".into());
        }

        // Load private key
        let key_file = std::fs::File::open(key_path)?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)?
            .ok_or("No private key found in key file")?;

        // Build TLS config with ALPN for HTTP/2
        let mut tls_config = RustlsConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        // Enable ALPN for HTTP/2 and HTTP/1.1
        tls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(tls_config)
    }

    /// Creates a socket with SO_REUSEPORT for multi-threaded accept.
    fn create_reuse_port_listener(addr: SocketAddr) -> std::io::Result<std::net::TcpListener> {
        let domain = if addr.is_ipv6() {
            Domain::IPV6
        } else {
            Domain::IPV4
        };

        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_address(true)?;

        // SO_REUSEPORT allows multiple sockets to bind to the same port
        #[cfg(unix)]
        socket.set_reuse_port(true)?;

        socket.set_nonblocking(true)?;
        socket.bind(&addr.into())?;
        socket.listen(1024)?;

        Ok(socket.into())
    }

    /// Run the server.
    /// Spawns worker accept loops and waits for shutdown signal.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let num_workers = if self.config.num_workers == 0 {
            num_cpus::get()
        } else {
            self.config.num_workers
        };

        let protocol = if self.tls_acceptor.is_some() {
            "https"
        } else {
            "http"
        };
        info!(
            "Server listening on {}://{} (executor: {}, workers: {})",
            protocol,
            self.config.addr,
            self.executor.name(),
            num_workers
        );

        // Mark PHP as initialized and startup complete
        self.health_checker.mark_php_initialized();
        self.health_checker.mark_startup_complete();

        // Spawn accept loops on multiple threads
        let mut handles = Vec::with_capacity(num_workers + 1);

        // Spawn internal server if configured
        if let Some(internal_addr) = self.config.internal_addr {
            let active_connections = Arc::clone(&self.active_connections);
            let request_metrics = Arc::clone(&self.request_metrics);
            let mut shutdown_rx = self.shutdown_rx.clone();

            // Build config info for /config endpoint (env var names as keys)
            let executor_name = self.executor.name();
            let config_info = Arc::new(ServerConfigInfo {
                listen_addr: self.config.addr.to_string(),
                document_root: self.config.document_root.to_string(),
                php_workers: num_workers.to_string(),
                queue_capacity: (num_workers * 100).to_string(),
                index_file: self.config.index_file.clone().unwrap_or_default(),
                internal_addr: internal_addr.to_string(),
                error_pages_dir: self.config.error_pages_dir.clone().unwrap_or_default(),
                drain_timeout_secs: self.config.drain_timeout.as_secs().to_string(),
                static_cache_ttl: format_optional_duration(&self.config.static_cache_ttl),
                request_timeout: format_optional_duration(&self.config.request_timeout),
                sse_timeout: format_optional_duration(&self.config.sse_timeout),
                access_log: if self.access_log_enabled {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
                rate_limit: self
                    .rate_limiter
                    .as_ref()
                    .map(|r| r.limit().to_string())
                    .unwrap_or_else(|| "0".to_string()),
                rate_window: self
                    .rate_limiter
                    .as_ref()
                    .map(|r| r.window_secs().to_string())
                    .unwrap_or_else(|| "60".to_string()),
                executor: executor_name.to_string(),
                profile: if self.profile_enabled {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
                tls_cert: self.config.tls_cert.clone().unwrap_or_default(),
                tls_key: self.config.tls_key.clone().unwrap_or_default(),
                log_level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
                service_name: std::env::var("SERVICE_NAME")
                    .unwrap_or_else(|_| "tokio_php".to_string()),
            });

            // Clone health_checker for internal server
            let health_checker = Arc::clone(&self.health_checker);

            let handle = tokio::spawn(async move {
                tokio::select! {
                    result = run_internal_server(internal_addr, active_connections, request_metrics, config_info, Some(health_checker)) => {
                        if let Err(e) = result {
                            error!("Internal server error: {}", e);
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        debug!("Internal server received shutdown signal");
                    }
                }
            });
            handles.push(handle);
            info!("Internal server listening on http://{}", internal_addr);
        }

        for worker_id in 0..num_workers {
            let addr = self.config.addr;
            let tls_acceptor = self.tls_acceptor.clone();
            let mut shutdown_rx = self.shutdown_rx.clone();
            let conn_shutdown_rx = self.shutdown_rx.clone();

            // Create connection context for this worker
            let ctx = Arc::new(ConnectionContext {
                executor: Arc::clone(&self.executor),
                document_root: Arc::clone(&self.config.document_root),
                document_root_static: self.document_root_static.clone(),
                is_stub_mode: self.executor.skip_file_check(),
                route_config: Arc::clone(&self.route_config),
                active_connections: Arc::clone(&self.active_connections),
                request_metrics: Arc::clone(&self.request_metrics),
                error_pages: self.error_pages.clone(),
                rate_limiter: self.rate_limiter.clone(),
                static_cache_ttl: self.config.static_cache_ttl,
                request_timeout: self.config.request_timeout,
                sse_timeout: self.config.sse_timeout,
                profile_enabled: self.profile_enabled,
                access_log_enabled: self.access_log_enabled,
                file_cache: Arc::clone(&self.file_cache),
            });

            let handle = tokio::spawn(async move {
                // Each worker creates its own listener with SO_REUSEPORT
                let std_listener = match Self::create_reuse_port_listener(addr) {
                    Ok(l) => l,
                    Err(e) => {
                        error!("Worker {}: Failed to create listener: {}", worker_id, e);
                        return;
                    }
                };

                let listener = match TcpListener::from_std(std_listener) {
                    Ok(l) => l,
                    Err(e) => {
                        error!("Worker {}: Failed to convert listener: {}", worker_id, e);
                        return;
                    }
                };

                debug!("Worker {} started", worker_id);

                loop {
                    tokio::select! {
                        result = listener.accept() => {
                            let (stream, remote_addr) = match result {
                                Ok(conn) => conn,
                                Err(e) => {
                                    error!("Worker {}: Accept error: {}", worker_id, e);
                                    continue;
                                }
                            };

                            let _ = stream.set_nodelay(true);

                            // Set TCP keepalive
                            let keepalive = TcpKeepalive::new()
                                .with_time(Duration::from_secs(5))
                                .with_interval(Duration::from_secs(1))
                                .with_retries(3);
                            let sock_ref = SockRef::from(&stream);
                            let _ = sock_ref.set_tcp_keepalive(&keepalive);

                            let ctx = Arc::clone(&ctx);
                            let tls = tls_acceptor.clone();
                            // Each connection gets its own shutdown receiver for graceful shutdown
                            let conn_shutdown = conn_shutdown_rx.clone();

                            tokio::spawn(async move {
                                ctx.handle_connection_graceful(stream, remote_addr, tls, conn_shutdown).await;
                            });
                        }
                        _ = shutdown_rx.changed() => {
                            debug!("Worker {} received shutdown signal, stopping accept loop", worker_id);
                            break;
                        }
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for all workers to stop accepting
        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    /// Trigger graceful shutdown.
    /// Signals all workers to stop accepting new connections.
    pub fn trigger_shutdown(&self) {
        if self.shutdown_initiated.swap(true, Ordering::SeqCst) {
            return; // Already initiated
        }
        let _ = self.shutdown_tx.send(true);
    }

    /// Get the configured drain timeout.
    pub fn drain_timeout(&self) -> Duration {
        self.config.drain_timeout
    }

    /// Get the executor (for gRPC server).
    pub fn executor(&self) -> Arc<E> {
        Arc::clone(&self.executor)
    }

    /// Get the health checker (for gRPC server).
    pub fn health_checker(&self) -> Arc<HealthChecker> {
        Arc::clone(&self.health_checker)
    }

    /// Get the document root.
    pub fn document_root(&self) -> &str {
        &self.config.document_root
    }

    /// Wait for all active connections to drain.
    /// Returns true if drained successfully, false if timeout was reached.
    pub async fn wait_for_drain(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        let check_interval = Duration::from_millis(100);

        loop {
            let active = self.active_connections.load(Ordering::Relaxed);
            if active == 0 {
                return true;
            }

            if start.elapsed() >= timeout {
                warn!("Drain timeout reached with {} active connections", active);
                return false;
            }

            debug!("Waiting for {} connections to drain...", active);
            tokio::time::sleep(check_interval).await;
        }
    }

    /// Shutdown the server.
    pub fn shutdown(&self) {
        self.executor.shutdown();
    }
}

/// Format OptionalDuration for config display.
fn format_optional_duration(d: &config::OptionalDuration) -> String {
    if !d.is_enabled() {
        return "off".to_string();
    }
    let secs = d.as_secs();
    if secs.is_multiple_of(86400) {
        format!("{}d", secs / 86400)
    } else if secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}
