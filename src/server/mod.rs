//! HTTP server with pluggable script executor.

pub mod access_log;
pub mod builder;
pub mod config;
pub mod connection;
pub mod error_pages;
mod internal;
pub mod rate_limit;
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

// Re-export builder API (currently unused in main.rs but part of public API)
#[allow(unused_imports)]
pub use builder::{BuildError, ServerBuilder};
use connection::ConnectionContext;
use error_pages::ErrorPages;
use internal::{run_internal_server, RequestMetrics};
use rate_limit::RateLimiter;

use crate::executor::ScriptExecutor;

/// HTTP server with pluggable script executor.
pub struct Server<E: ScriptExecutor> {
    config: ServerConfig,
    executor: Arc<E>,
    tls_acceptor: Option<TlsAcceptor>,
    /// Pre-validated index file path (full path, validated at startup)
    index_file_path: Option<Arc<str>>,
    /// Active connections counter
    active_connections: Arc<AtomicUsize>,
    /// Request metrics by HTTP method
    request_metrics: Arc<RequestMetrics>,
    /// Cached custom error pages
    error_pages: ErrorPages,
    /// Per-IP rate limiter
    rate_limiter: Option<Arc<RateLimiter>>,
    /// Shutdown signal sender
    shutdown_tx: watch::Sender<bool>,
    /// Shutdown signal receiver (cloneable)
    shutdown_rx: watch::Receiver<bool>,
    /// Shutdown initiated flag
    shutdown_initiated: Arc<AtomicBool>,
    /// Profiling enabled (PROFILE=1)
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
        // Validate index file at startup if configured
        let index_file_path = if let Some(ref index_file) = config.index_file {
            let full_path = format!("{}/{}", config.document_root, index_file);
            if !Path::new(&full_path).exists() {
                return Err(format!(
                    "Index file not found: {} (INDEX_FILE={})",
                    full_path, index_file
                )
                .into());
            }
            info!("Single entry point mode: all requests -> {}", index_file);
            Some(Arc::from(full_path.as_str()))
        } else {
            None
        };

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

        Ok(Self {
            config,
            executor: Arc::new(executor),
            tls_acceptor,
            index_file_path,
            active_connections: Arc::new(AtomicUsize::new(0)),
            request_metrics: Arc::new(RequestMetrics::new()),
            error_pages,
            rate_limiter: None,
            shutdown_tx,
            shutdown_rx,
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
            profile_enabled: false,
            access_log_enabled: false,
        })
    }

    /// Enable profiling for this server.
    /// Requests with X-Profile: 1 header will include timing data.
    pub fn with_profile_enabled(mut self, enabled: bool) -> Self {
        self.profile_enabled = enabled;
        if enabled {
            info!("Profiler enabled (PROFILE=1)");
        }
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
    ///
    /// # Arguments
    /// * `limit` - Maximum requests per IP per window (None = disabled)
    /// * `window_secs` - Window duration in seconds
    pub fn with_rate_limiter(mut self, limit: Option<u64>, window_secs: u64) -> Self {
        if let Some(limit) = limit {
            let limiter = RateLimiter::new(limit, window_secs);
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

        // Spawn accept loops on multiple threads
        let mut handles = Vec::with_capacity(num_workers + 1);

        // Spawn internal server if configured
        if let Some(internal_addr) = self.config.internal_addr {
            let active_connections = Arc::clone(&self.active_connections);
            let request_metrics = Arc::clone(&self.request_metrics);
            let mut shutdown_rx = self.shutdown_rx.clone();
            let handle = tokio::spawn(async move {
                tokio::select! {
                    result = run_internal_server(internal_addr, active_connections, request_metrics) => {
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

        // Index file name for blocking direct access
        let index_file_name = self
            .config
            .index_file
            .as_ref()
            .map(|s| Arc::from(s.as_str()));

        for worker_id in 0..num_workers {
            let addr = self.config.addr;
            let tls_acceptor = self.tls_acceptor.clone();
            let mut shutdown_rx = self.shutdown_rx.clone();
            let conn_shutdown_rx = self.shutdown_rx.clone();

            // Create connection context for this worker
            let ctx = Arc::new(ConnectionContext {
                executor: Arc::clone(&self.executor),
                document_root: Arc::clone(&self.config.document_root),
                skip_file_check: self.executor.skip_file_check() || self.index_file_path.is_some(),
                is_stub_mode: self.executor.skip_file_check(),
                index_file_path: self.index_file_path.clone(),
                index_file_name: index_file_name.clone(),
                active_connections: Arc::clone(&self.active_connections),
                request_metrics: Arc::clone(&self.request_metrics),
                error_pages: self.error_pages.clone(),
                rate_limiter: self.rate_limiter.clone(),
                static_cache_ttl: self.config.static_cache_ttl.clone(),
                request_timeout: self.config.request_timeout.clone(),
                profile_enabled: self.profile_enabled,
                access_log_enabled: self.access_log_enabled,
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
                warn!(
                    "Drain timeout reached with {} active connections",
                    active
                );
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
