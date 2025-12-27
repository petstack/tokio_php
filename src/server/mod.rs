//! HTTP server with pluggable script executor.

pub mod config;
mod connection;
mod internal;
pub mod request;
pub mod response;
mod routing;

use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use socket2::{Domain, Protocol, SockRef, Socket, TcpKeepalive, Type};
use tokio::net::TcpListener;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::ServerConfig as RustlsConfig;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};

pub use config::ServerConfig;
use connection::ConnectionContext;
use internal::{run_internal_server, RequestMetrics};

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

        Ok(Self {
            config,
            executor: Arc::new(executor),
            tls_acceptor,
            index_file_path,
            active_connections: Arc::new(AtomicUsize::new(0)),
            request_metrics: Arc::new(RequestMetrics::new()),
        })
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
            let handle = tokio::spawn(async move {
                if let Err(e) =
                    run_internal_server(internal_addr, active_connections, request_metrics).await
                {
                    error!("Internal server error: {}", e);
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
                    let (stream, remote_addr) = match listener.accept().await {
                        Ok(conn) => conn,
                        Err(e) => {
                            error!("Worker {}: Accept error: {}", worker_id, e);
                            continue;
                        }
                    };

                    let _ = stream.set_nodelay(true);

                    // Set TCP keepalive to detect dead connections faster (5s idle, 1s interval, 3 retries)
                    let keepalive = TcpKeepalive::new()
                        .with_time(Duration::from_secs(5))
                        .with_interval(Duration::from_secs(1))
                        .with_retries(3);
                    let sock_ref = SockRef::from(&stream);
                    let _ = sock_ref.set_tcp_keepalive(&keepalive);

                    let ctx = Arc::clone(&ctx);
                    let tls = tls_acceptor.clone();

                    tokio::task::spawn(async move {
                        ctx.handle_connection(stream, remote_addr, tls).await;
                    });
                }
            });

            handles.push(handle);
        }

        // Wait for all workers (they run forever unless cancelled)
        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    /// Shutdown the server.
    pub fn shutdown(&self) {
        self.executor.shutdown();
    }
}
