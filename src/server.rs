use bytes::Bytes;
use futures_util::stream;
use http_body_util::Full;
use hyper_util::server::conn::auto;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response, StatusCode, Method};
use hyper_util::rt::{TokioExecutor, TokioIo};
use multer::Multipart;
use std::convert::Infallible;
use std::io::{BufReader, Read};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig as RustlsConfig;
use tracing::{error, info, debug, warn};
use uuid::Uuid;
use http_body_util::BodyExt;

use crate::executor::ScriptExecutor;
use crate::profiler;
use crate::types::{ScriptRequest, ScriptResponse, UploadedFile};

const MAX_UPLOAD_SIZE: u64 = 10 * 1024 * 1024;

/// TLS connection information for profiling
#[derive(Clone, Default)]
struct TlsInfo {
    handshake_us: u64,
    protocol: String,
    alpn: String,
}

// Pre-allocated static bytes for common responses
static EMPTY_BODY: Bytes = Bytes::from_static(b"");
static NOT_FOUND_BODY: Bytes = Bytes::from_static(b"404 Not Found");
static METHOD_NOT_ALLOWED_BODY: Bytes = Bytes::from_static(b"Method Not Allowed");
static BAD_REQUEST_BODY: Bytes = Bytes::from_static(b"Failed to read request body");

/// Minimum size to consider compression (smaller bodies don't benefit)
const MIN_COMPRESSION_SIZE: usize = 256;

/// Brotli compression quality (0-11, higher = better compression but slower)
const BROTLI_QUALITY: u32 = 4;

/// Brotli compression window size (10-24, affects memory usage)
const BROTLI_WINDOW: u32 = 20;

/// Check if the client accepts Brotli encoding
#[inline]
fn accepts_brotli(accept_encoding: &str) -> bool {
    accept_encoding.split(',')
        .any(|enc| enc.trim().starts_with("br"))
}

/// Check if the MIME type should be compressed
#[inline]
fn should_compress_mime(content_type: &str) -> bool {
    let ct = content_type.split(';').next().unwrap_or("").trim();
    matches!(ct,
        // Text types
        "text/html" |
        "text/css" |
        "text/plain" |
        "text/xml" |
        "text/javascript" |
        // Application types
        "application/javascript" |
        "application/json" |
        "application/xml" |
        "application/xhtml+xml" |
        "application/rss+xml" |
        "application/atom+xml" |
        "application/manifest+json" |
        "application/ld+json" |
        // SVG
        "image/svg+xml" |
        // Fonts (uncompressed formats - WOFF/WOFF2 are already compressed)
        "font/ttf" |
        "font/otf" |
        "application/x-font-ttf" |
        "application/x-font-opentype" |
        "application/vnd.ms-fontobject"
    )
}

/// Compress data using Brotli
#[inline]
fn compress_brotli(data: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(data.len() / 2);
    let mut input = std::io::Cursor::new(data);
    let params = brotli::enc::BrotliEncoderParams {
        quality: BROTLI_QUALITY as i32,
        lgwin: BROTLI_WINDOW as i32,
        ..Default::default()
    };

    match brotli::BrotliCompress(&mut input, &mut output, &params) {
        Ok(_) if output.len() < data.len() => Some(output),
        _ => None,
    }
}

// Pre-built empty response for stub mode
fn empty_stub_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Server", "tokio_php/0.1.0")
        .header("Content-Length", "0")
        .body(Full::new(EMPTY_BODY.clone()))
        .unwrap()
}

/// Server configuration.
#[derive(Clone)]
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
    /// When set: all requests route to this file, direct access returns 404
    pub index_file: Option<String>,
    /// Internal server address for /health and /metrics (e.g., "0.0.0.0:9000")
    pub internal_addr: Option<SocketAddr>,
}

impl ServerConfig {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            document_root: Arc::from("/var/www/html"),
            num_workers: 0, // auto-detect
            tls_cert: None,
            tls_key: None,
            index_file: None,
            internal_addr: None,
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

    pub fn has_tls(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }
}

/// HTTP server with pluggable script executor.
pub struct Server<E: ScriptExecutor> {
    config: ServerConfig,
    executor: Arc<E>,
    tls_acceptor: Option<TlsAcceptor>,
    /// Pre-validated index file path (full path, validated at startup)
    index_file_path: Option<Arc<str>>,
    /// Active connections counter
    active_connections: Arc<AtomicUsize>,
}

impl<E: ScriptExecutor + 'static> Server<E> {
    pub fn new(config: ServerConfig, executor: E) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Validate index file at startup if configured
        let index_file_path = if let Some(ref index_file) = config.index_file {
            let full_path = format!("{}/{}", config.document_root, index_file);
            if !Path::new(&full_path).exists() {
                return Err(format!(
                    "Index file not found: {} (INDEX_FILE={})",
                    full_path, index_file
                ).into());
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
        })
    }

    /// Get current active connections count
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    fn load_tls_config(config: &ServerConfig) -> Result<RustlsConfig, Box<dyn std::error::Error + Send + Sync>> {
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
        use socket2::{Domain, Protocol, Socket, Type};

        let domain = if addr.is_ipv6() {
            Domain::IPV6
        } else {
            Domain::IPV4
        };

        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_address(true)?;

        // SO_REUSEPORT allows multiple sockets to bind to the same port
        // The kernel will load-balance incoming connections across all listeners
        #[cfg(unix)]
        socket.set_reuse_port(true)?;

        socket.set_nonblocking(true)?;
        socket.bind(&addr.into())?;
        socket.listen(1024)?;

        Ok(socket.into())
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let num_workers = if self.config.num_workers == 0 {
            num_cpus::get()
        } else {
            self.config.num_workers
        };

        let protocol = if self.tls_acceptor.is_some() { "https" } else { "http" };
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
            let handle = tokio::spawn(async move {
                if let Err(e) = run_internal_server(internal_addr, active_connections).await {
                    error!("Internal server error: {}", e);
                }
            });
            handles.push(handle);
            info!("Internal server listening on http://{}", internal_addr);
        }

        // Index file name for blocking direct access (e.g., "index.php")
        let index_file_name = self.config.index_file.as_ref().map(|s| Arc::from(s.as_str()));

        for worker_id in 0..num_workers {
            let addr = self.config.addr;
            let executor = Arc::clone(&self.executor);
            let document_root = Arc::clone(&self.config.document_root);
            let skip_file_check = self.executor.skip_file_check() || self.index_file_path.is_some();
            let tls_acceptor = self.tls_acceptor.clone();
            let index_file_path = self.index_file_path.clone();
            let index_file_name = index_file_name.clone();
            let active_connections = Arc::clone(&self.active_connections);

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

                    let executor = Arc::clone(&executor);
                    let document_root = Arc::clone(&document_root);
                    let tls_acceptor = tls_acceptor.clone();
                    let index_file_path = index_file_path.clone();
                    let index_file_name = index_file_name.clone();
                    let conn_counter = Arc::clone(&active_connections);

                    tokio::task::spawn(async move {
                        // Increment active connections
                        conn_counter.fetch_add(1, Ordering::Relaxed);

                        // Handle TLS or plain TCP
                        if let Some(acceptor) = tls_acceptor {
                            // Measure TLS handshake time
                            let tls_start = std::time::Instant::now();
                            match acceptor.accept(stream).await {
                                Ok(tls_stream) => {
                                    let handshake_us = tls_start.elapsed().as_micros() as u64;

                                    // Extract TLS info from the connection
                                    let (_, server_conn) = tls_stream.get_ref();
                                    let tls_info = TlsInfo {
                                        handshake_us,
                                        protocol: server_conn.protocol_version()
                                            .map(|v| format!("{:?}", v))
                                            .unwrap_or_default(),
                                        alpn: server_conn.alpn_protocol()
                                            .map(|p| String::from_utf8_lossy(p).to_string())
                                            .unwrap_or_default(),
                                    };

                                    let service = service_fn(move |req| {
                                        let executor = Arc::clone(&executor);
                                        let doc_root = Arc::clone(&document_root);
                                        let tls = tls_info.clone();
                                        let idx_path = index_file_path.clone();
                                        let idx_name = index_file_name.clone();
                                        async move {
                                            handle_request(req, remote_addr, executor, doc_root, skip_file_check, Some(tls), idx_path, idx_name).await
                                        }
                                    });

                                    let io = TokioIo::new(tls_stream);
                                    if let Err(err) = auto::Builder::new(TokioExecutor::new())
                                        .http1()
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
                                Err(e) => {
                                    debug!("TLS handshake failed: {:?}", e);
                                }
                            }
                        } else {
                            let service = service_fn(move |req| {
                                let executor = Arc::clone(&executor);
                                let doc_root = Arc::clone(&document_root);
                                let idx_path = index_file_path.clone();
                                let idx_name = index_file_name.clone();
                                async move {
                                    handle_request(req, remote_addr, executor, doc_root, skip_file_check, None, idx_path, idx_name).await
                                }
                            });

                            let io = TokioIo::new(stream);
                            if let Err(err) = auto::Builder::new(TokioExecutor::new())
                                .http1()
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

                        // Decrement active connections
                        conn_counter.fetch_sub(1, Ordering::Relaxed);
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

    pub fn shutdown(&self) {
        self.executor.shutdown();
    }
}

#[inline]
fn is_connection_error(err_str: &str) -> bool {
    err_str.contains("connection reset")
        || err_str.contains("broken pipe")
        || err_str.contains("Connection reset")
        || err_str.contains("os error 104")
        || err_str.contains("os error 32")
}

/// Internal HTTP server for /health and /metrics endpoints
async fn run_internal_server(
    addr: SocketAddr,
    active_connections: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let connections = Arc::clone(&active_connections);

        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let conns = connections.load(Ordering::Relaxed);
                async move { handle_internal_request(req, conns).await }
            });

            let io = TokioIo::new(stream);
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await;
        });
    }
}

/// Handle internal server requests (/health, /metrics)
async fn handle_internal_request(
    req: Request<IncomingBody>,
    active_connections: usize,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();

    let response = match path {
        "/health" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let body = format!(
                r#"{{"status":"ok","timestamp":{},"active_connections":{}}}"#,
                now.as_secs(), active_connections
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        "/metrics" => {
            // Stub for now - will be expanded later
            let body = format!(
                "# HELP tokio_php_active_connections Current number of active connections\n\
                 # TYPE tokio_php_active_connections gauge\n\
                 tokio_php_active_connections {}\n",
                active_connections
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4")
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        _ => {
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "text/plain")
                .body(Full::new(Bytes::from("Not Found")))
                .unwrap()
        }
    };

    Ok(response)
}

async fn handle_request<E: ScriptExecutor>(
    req: Request<IncomingBody>,
    remote_addr: SocketAddr,
    executor: Arc<E>,
    document_root: Arc<str>,
    skip_file_check: bool,
    tls_info: Option<TlsInfo>,
    index_file_path: Option<Arc<str>>,
    index_file_name: Option<Arc<str>>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let is_head = *req.method() == Method::HEAD;

    let response = match *req.method() {
        Method::GET | Method::POST | Method::HEAD => {
            let mut resp = process_request(req, remote_addr, executor, document_root, skip_file_check, tls_info, index_file_path, index_file_name).await;

            // HEAD: return headers only, no body
            if is_head {
                let (parts, _) = resp.into_parts();
                resp = Response::from_parts(parts, Full::new(EMPTY_BODY.clone()));
            }
            resp
        }
        _ => Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header("Content-Type", "text/plain")
            .body(Full::new(METHOD_NOT_ALLOWED_BODY.clone()))
            .unwrap()
    };

    Ok(response)
}

/// Fast percent decode - only allocates if '%' is present
#[inline]
fn fast_percent_decode(s: &str) -> String {
    if s.contains('%') {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    } else {
        s.to_string()
    }
}

#[inline]
fn parse_query_string(query: &str) -> Vec<(String, String)> {
    let pair_count = query.matches('&').count() + 1;
    let mut params = Vec::with_capacity(pair_count.min(16));

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let (key, value) = match pair.find('=') {
            Some(pos) => (&pair[..pos], &pair[pos + 1..]),
            None => (pair, ""),
        };

        if !key.is_empty() {
            params.push((fast_percent_decode(key), fast_percent_decode(value)));
        }
    }

    params
}

#[inline]
fn parse_cookies(cookie_header: &str) -> Vec<(String, String)> {
    let cookie_count = cookie_header.matches(';').count() + 1;
    let mut cookies = Vec::with_capacity(cookie_count.min(16));

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if cookie.is_empty() {
            continue;
        }

        let (name, value) = match cookie.find('=') {
            Some(pos) => (cookie[..pos].trim(), cookie[pos + 1..].trim()),
            None => continue,
        };

        if !name.is_empty() {
            cookies.push((name.to_string(), fast_percent_decode(value)));
        }
    }

    cookies
}

async fn parse_multipart(
    content_type: &str,
    body: Bytes,
) -> Result<(Vec<(String, String)>, Vec<(String, Vec<UploadedFile>)>), String> {
    let boundary = content_type
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            if part.starts_with("boundary=") {
                Some(part[9..].trim_matches('"').to_string())
            } else {
                None
            }
        })
        .ok_or("Missing boundary in multipart content-type")?;

    let mut multipart = Multipart::new(stream::once(async { Ok::<_, std::io::Error>(body) }), boundary);

    let mut params = Vec::new();
    let mut files: Vec<(String, Vec<UploadedFile>)> = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let field_name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().map(|s| s.to_string());
        let field_content_type = field.content_type().map(|m| m.to_string()).unwrap_or_default();

        if let Some(original_name) = file_name {
            if original_name.is_empty() {
                continue;
            }

            let data = field.bytes().await.map_err(|e| e.to_string())?;
            let size = data.len() as u64;

            let normalized_name = if field_name.ends_with("[]") {
                field_name[..field_name.len() - 2].to_string()
            } else {
                field_name
            };

            let uploaded_file = if size > MAX_UPLOAD_SIZE {
                UploadedFile {
                    name: original_name,
                    mime_type: field_content_type,
                    tmp_name: String::new(),
                    size,
                    error: 1,
                }
            } else {
                let tmp_name = format!("/tmp/php{}", Uuid::new_v4().simple());

                let mut file = File::create(&tmp_name).await.map_err(|e| e.to_string())?;
                file.write_all(&data).await.map_err(|e| e.to_string())?;
                file.flush().await.map_err(|e| e.to_string())?;

                UploadedFile {
                    name: original_name,
                    mime_type: field_content_type,
                    tmp_name,
                    size,
                    error: 0,
                }
            };

            // Find existing entry or create new one
            if let Some(entry) = files.iter_mut().find(|(name, _)| name == &normalized_name) {
                entry.1.push(uploaded_file);
            } else {
                files.push((normalized_name, vec![uploaded_file]));
            }
        } else {
            let value = field.text().await.map_err(|e| e.to_string())?;
            params.push((field_name, value));
        }
    }

    Ok((params, files))
}

async fn process_request<E: ScriptExecutor>(
    req: Request<IncomingBody>,
    remote_addr: SocketAddr,
    executor: Arc<E>,
    document_root: Arc<str>,
    skip_file_check: bool,
    tls_info: Option<TlsInfo>,
    index_file_path: Option<Arc<str>>,
    index_file_name: Option<Arc<str>>,
) -> Response<Full<Bytes>> {
    use std::time::Instant;

    // Capture request timestamp at the very start
    let request_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let request_time_secs = request_time.as_secs();
    let request_time_float = request_time.as_secs_f64();

    let parse_start = Instant::now();

    // Profile timing variables
    let mut headers_extract_us = 0u64;
    let mut query_parse_us = 0u64;
    let mut cookies_parse_us = 0u64;
    let mut body_read_us = 0u64;
    let mut body_parse_us = 0u64;
    let mut server_vars_us = 0u64;
    let mut path_resolve_us = 0u64;
    let mut file_check_us = 0u64;

    let method = req.method().clone();
    let http_version = match req.version() {
        hyper::Version::HTTP_2 => "HTTP/2.0",
        hyper::Version::HTTP_11 => "HTTP/1.1",
        hyper::Version::HTTP_10 => "HTTP/1.0",
        hyper::Version::HTTP_3 => "HTTP/3.0",
        _ => "HTTP/1.1",
    }.to_string();
    let uri = req.uri().clone();
    let uri_path = uri.path();
    let query_string = uri.query().unwrap_or("");

    // Block direct access to index file in single entry point mode
    // e.g., /index.php -> 404 when INDEX_FILE=index.php
    if let Some(ref idx_name) = index_file_name {
        let direct_path = format!("/{}", idx_name.as_ref());
        if uri_path == direct_path || uri_path.starts_with(&format!("{}/", direct_path)) {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "text/html")
                .body(Full::new(NOT_FOUND_BODY.clone()))
                .unwrap();
        }
    }

    // Check for profiling header
    let profile_requested = req
        .headers()
        .get("x-profile")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let profiling_enabled = profile_requested && profiler::is_enabled();

    // Check if client accepts Brotli compression
    let use_brotli = req
        .headers()
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .map(accepts_brotli)
        .unwrap_or(false);

    // Fast path for stub: minimal processing
    if skip_file_check {
        // Only need to check if it's a PHP file for the response
        let is_php = uri_path.ends_with(".php")
            || uri_path.ends_with('/')
            || uri_path == "/";

        if is_php {
            // Ultra-fast path for stub - direct response without executor call
            return empty_stub_response();
        }
    }

    // Full processing path - extract headers before consuming body
    let headers_start = Instant::now();
    let headers = req.headers();

    let content_type_str = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let cookie_header_str = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Extract additional HTTP headers for $_SERVER
    // For HTTP/2, the :authority pseudo-header is in uri.authority()
    let host_header = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| uri.authority().map(|a| a.to_string()))
        .unwrap_or_default();

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let referer = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let accept_language = headers
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let accept = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if profiling_enabled {
        headers_extract_us = headers_start.elapsed().as_micros() as u64;
    }

    // Parse cookies
    let cookies_start = Instant::now();
    let cookies = if cookie_header_str.is_empty() {
        Vec::new()
    } else {
        parse_cookies(&cookie_header_str)
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

    // Handle POST body
    let (post_params, files) = if method == Method::POST {
        let body_read_start = Instant::now();
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => {
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "text/plain")
                    .body(Full::new(BAD_REQUEST_BODY.clone()))
                    .unwrap();
            }
        };
        if profiling_enabled {
            body_read_us = body_read_start.elapsed().as_micros() as u64;
        }

        let body_parse_start = Instant::now();
        let result = if content_type_str.starts_with("application/x-www-form-urlencoded") {
            let body_str = String::from_utf8_lossy(&body_bytes);
            (parse_query_string(&body_str), Vec::new())
        } else if content_type_str.starts_with("multipart/form-data") {
            match parse_multipart(&content_type_str, body_bytes).await {
                Ok((params, uploaded_files)) => (params, uploaded_files),
                Err(e) => {
                    return Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("Content-Type", "text/plain")
                        .body(Full::new(Bytes::from(format!("Failed to parse multipart form: {}", e))))
                        .unwrap();
                }
            }
        } else {
            (Vec::new(), Vec::new())
        };
        if profiling_enabled {
            body_parse_us = body_parse_start.elapsed().as_micros() as u64;
        }
        result
    } else {
        (Vec::new(), Vec::new())
    };

    // Decode URL path and resolve file (moved before server_vars)
    let path_start = Instant::now();

    // In single entry point mode, always use the pre-validated index file
    let file_path_string = if let Some(ref idx_path) = index_file_path {
        idx_path.to_string()
    } else {
        let decoded_path = percent_encoding::percent_decode_str(uri_path)
            .decode_utf8_lossy();

        let clean_path = decoded_path
            .trim_start_matches('/')
            .replace("..", "");

        if clean_path.is_empty() || clean_path.ends_with('/') {
            format!("{}/{}/index.php", document_root, clean_path)
        } else {
            format!("{}/{}", document_root, clean_path)
        }
    };

    let file_path = Path::new(&file_path_string);

    let extension = file_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if profiling_enabled {
        path_resolve_us = path_start.elapsed().as_micros() as u64;
    }

    // Check if file exists (sync - fast for stat syscall)
    let file_check_start = Instant::now();
    if !skip_file_check && !file_path.exists() {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/html")
            .body(Full::new(NOT_FOUND_BODY.clone()))
            .unwrap();
    }
    if profiling_enabled {
        file_check_us = file_check_start.elapsed().as_micros() as u64;
    }

    // Build server variables (after path resolution for SCRIPT_* vars)
    let server_vars_start = Instant::now();

    // Parse Host header for SERVER_NAME and SERVER_PORT
    let (server_name, server_port) = if !host_header.is_empty() {
        if let Some(colon_pos) = host_header.rfind(':') {
            // Check if it's not an IPv6 address without port
            if host_header.starts_with('[') && !host_header.contains("]:") {
                (host_header.clone(), if tls_info.is_some() { "443" } else { "80" }.to_string())
            } else {
                (host_header[..colon_pos].to_string(), host_header[colon_pos + 1..].to_string())
            }
        } else {
            (host_header.clone(), if tls_info.is_some() { "443" } else { "80" }.to_string())
        }
    } else {
        ("localhost".to_string(), if tls_info.is_some() { "443" } else { "80" }.to_string())
    };

    // Calculate SCRIPT_NAME and PHP_SELF (path relative to document root)
    let script_name = file_path_string
        .strip_prefix(document_root.as_ref())
        .unwrap_or(&file_path_string)
        .to_string();
    let script_name = if script_name.starts_with('/') {
        script_name
    } else {
        format!("/{}", script_name)
    };

    // PATH_INFO: additional path after script name (for now, empty - requires PATH_TRANSLATED logic)
    let path_info = String::new();

    // Estimate capacity for server_vars
    let mut server_vars = Vec::with_capacity(32);

    // Request timing
    server_vars.push(("REQUEST_TIME".into(), request_time_secs.to_string()));
    server_vars.push(("REQUEST_TIME_FLOAT".into(), format!("{:.6}", request_time_float)));

    // Request method and URI
    server_vars.push(("REQUEST_METHOD".into(), method.as_str().to_string()));
    server_vars.push(("REQUEST_URI".into(), uri.to_string()));
    server_vars.push(("QUERY_STRING".into(), query_string.to_string()));

    // Client info
    server_vars.push(("REMOTE_ADDR".into(), remote_addr.ip().to_string()));
    server_vars.push(("REMOTE_PORT".into(), remote_addr.port().to_string()));

    // Server info
    server_vars.push(("SERVER_NAME".into(), server_name));
    server_vars.push(("SERVER_PORT".into(), server_port));
    server_vars.push(("SERVER_ADDR".into(), "0.0.0.0".into())); // Bound address
    server_vars.push(("SERVER_SOFTWARE".into(), "tokio_php/0.1.0".into()));
    server_vars.push(("SERVER_PROTOCOL".into(), http_version.clone()));
    server_vars.push(("DOCUMENT_ROOT".into(), document_root.to_string()));
    server_vars.push(("GATEWAY_INTERFACE".into(), "CGI/1.1".into()));

    // Script paths
    server_vars.push(("SCRIPT_NAME".into(), script_name.clone()));
    server_vars.push(("SCRIPT_FILENAME".into(), file_path_string.clone()));
    server_vars.push(("PHP_SELF".into(), script_name.clone()));
    if !path_info.is_empty() {
        server_vars.push(("PATH_INFO".into(), path_info));
    }

    // Content info
    server_vars.push(("CONTENT_TYPE".into(), content_type_str));

    // HTTP headers (with HTTP_ prefix)
    if !host_header.is_empty() {
        server_vars.push(("HTTP_HOST".into(), host_header));
    }
    if !cookie_header_str.is_empty() {
        server_vars.push(("HTTP_COOKIE".into(), cookie_header_str));
    }
    if !user_agent.is_empty() {
        server_vars.push(("HTTP_USER_AGENT".into(), user_agent));
    }
    if !referer.is_empty() {
        server_vars.push(("HTTP_REFERER".into(), referer));
    }
    if !accept_language.is_empty() {
        server_vars.push(("HTTP_ACCEPT_LANGUAGE".into(), accept_language));
    }
    if !accept.is_empty() {
        server_vars.push(("HTTP_ACCEPT".into(), accept));
    }

    // HTTPS/TLS info
    if let Some(ref tls) = tls_info {
        server_vars.push(("HTTPS".into(), "on".into()));
        if !tls.protocol.is_empty() {
            server_vars.push(("SSL_PROTOCOL".into(), tls.protocol.clone()));
        }
    }

    if profiling_enabled {
        server_vars_us = server_vars_start.elapsed().as_micros() as u64;
    }

    if extension == "php" {
        let temp_files: Vec<String> = files.iter()
            .flat_map(|(_, file_vec)| file_vec.iter().map(|f| f.tmp_name.clone()))
            .filter(|path| !path.is_empty())
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
            profile: profiling_enabled,
        };

        let response = match executor.execute(script_request).await {
            Ok(mut resp) => {
                // Add parse breakdown to profile data if profiling
                if profiling_enabled {
                    if let Some(ref mut profile) = resp.profile {
                        // TLS and connection info
                        profile.http_version = http_version.clone();
                        if let Some(ref tls) = tls_info {
                            profile.tls_handshake_us = tls.handshake_us;
                            profile.tls_protocol = tls.protocol.clone();
                            profile.tls_alpn = tls.alpn.clone();
                        }

                        // Parse timing breakdown
                        profile.parse_request_us = parse_request_us;
                        profile.headers_extract_us = headers_extract_us;
                        profile.query_parse_us = query_parse_us;
                        profile.cookies_parse_us = cookies_parse_us;
                        profile.body_read_us = body_read_us;
                        profile.body_parse_us = body_parse_us;
                        profile.server_vars_us = server_vars_us;
                        profile.path_resolve_us = path_resolve_us;
                        profile.file_check_us = file_check_us;
                    }
                }
                create_script_response_fast(resp, profiling_enabled, use_brotli)
            }
            Err(e) => {
                error!("Script execution error: {}", e);
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "text/html")
                    .body(Full::new(Bytes::from(format!("<h1>500 Internal Server Error</h1><pre>{}</pre>", e))))
                    .unwrap()
            }
        };

        // Clean up temp files
        for temp_file in temp_files {
            let _ = tokio::fs::remove_file(&temp_file).await;
        }

        response
    } else {
        serve_static_file(file_path, use_brotli).await
    }
}

#[inline]
fn create_script_response_fast(script_response: ScriptResponse, profiling: bool, use_brotli: bool) -> Response<Full<Bytes>> {
    let default_content_type = "text/html; charset=utf-8";

    // Fast path: no headers to process, no profiling, no compression
    if script_response.headers.is_empty() && !profiling && !use_brotli {
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", default_content_type)
            .header("Server", "tokio_php/0.1.0")
            .body(Full::new(if script_response.body.is_empty() {
                EMPTY_BODY.clone()
            } else {
                Bytes::from(script_response.body)
            }))
            .unwrap();
    }

    // Full header processing
    let mut status = StatusCode::OK;
    let mut actual_content_type = default_content_type.to_string();
    let mut custom_headers: Vec<(&str, String)> = Vec::with_capacity(script_response.headers.len());

    for (name, value) in &script_response.headers {
        let name_lower = name.to_lowercase();

        if name_lower.starts_with("http/") {
            if let Some(code_str) = value.split_whitespace().next() {
                if let Ok(code) = code_str.parse::<u16>() {
                    if code >= 200 {
                        if let Ok(s) = StatusCode::from_u16(code) {
                            status = s;
                        }
                    }
                }
            }
            continue;
        }

        match name_lower.as_str() {
            "content-type" => {
                actual_content_type = value.clone();
                custom_headers.push(("Content-Type", value.clone()));
            }
            "location" => {
                if !status.is_redirection() {
                    status = StatusCode::FOUND;
                }
                custom_headers.push(("Location", value.clone()));
            }
            "status" => {
                if let Some(code_str) = value.split_whitespace().next() {
                    if let Ok(code) = code_str.parse::<u16>() {
                        if code >= 200 {
                            if let Ok(s) = StatusCode::from_u16(code) {
                                status = s;
                            }
                        }
                    }
                }
            }
            _ => {
                if is_valid_header_name(name) {
                    custom_headers.push((name.as_str(), value.clone()));
                }
            }
        }
    }

    // Determine body and compression
    let body_bytes = script_response.body;
    let should_compress = use_brotli
        && body_bytes.len() >= MIN_COMPRESSION_SIZE
        && should_compress_mime(&actual_content_type);

    let (final_body, is_compressed) = if should_compress {
        match compress_brotli(body_bytes.as_bytes()) {
            Some(compressed) => (Bytes::from(compressed), true),
            None => (Bytes::from(body_bytes), false),
        }
    } else if body_bytes.is_empty() {
        (EMPTY_BODY.clone(), false)
    } else {
        (Bytes::from(body_bytes), false)
    };

    let mut builder = Response::builder()
        .status(status)
        .header("Server", "tokio_php/0.1.0");

    // Add Content-Encoding if compressed
    if is_compressed {
        builder = builder.header("Content-Encoding", "br");
        builder = builder.header("Vary", "Accept-Encoding");
    }

    // Check if content-type was set
    let has_content_type = custom_headers.iter().any(|(n, _)| *n == "Content-Type");
    if !has_content_type {
        builder = builder.header("Content-Type", default_content_type);
    }

    for (name, value) in custom_headers {
        builder = builder.header(name, value);
    }

    // Add profiling headers if profiling is enabled
    if profiling {
        if let Some(ref profile) = script_response.profile {
            for (name, value) in profile.to_headers() {
                builder = builder.header(name, value);
            }
        }
    }

    builder
        .body(Full::new(final_body))
        .unwrap()
}

#[inline]
fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| {
        matches!(b, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
                    b'0'..=b'9' | b'A'..=b'Z' | b'^' | b'_' | b'`' | b'a'..=b'z' | b'|' | b'~')
    })
}

async fn serve_static_file(file_path: &Path, use_brotli: bool) -> Response<Full<Bytes>> {
    match tokio::fs::read(file_path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(file_path)
                .first_or_octet_stream()
                .to_string();

            // Check if we should compress this file
            let should_compress = use_brotli
                && contents.len() >= MIN_COMPRESSION_SIZE
                && should_compress_mime(&mime);

            let (final_body, is_compressed) = if should_compress {
                if let Some(compressed) = compress_brotli(&contents) {
                    (Bytes::from(compressed), true)
                } else {
                    (Bytes::from(contents), false)
                }
            } else {
                (Bytes::from(contents), false)
            };

            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", &mime)
                .header("Server", "tokio_php/0.1.0");

            if is_compressed {
                builder = builder
                    .header("Content-Encoding", "br")
                    .header("Vary", "Accept-Encoding");
            }

            builder.body(Full::new(final_body)).unwrap()
        }
        Err(e) => {
            error!("Failed to read file {:?}: {}", file_path, e);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "text/plain")
                .body(Full::new(NOT_FOUND_BODY.clone()))
                .unwrap()
        }
    }
}
