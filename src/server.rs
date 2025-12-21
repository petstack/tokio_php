use bytes::Bytes;
use futures_util::stream;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response, StatusCode, Method};
use hyper_util::rt::TokioIo;
use multer::Multipart;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tracing::{error, info, debug};
use uuid::Uuid;
use http_body_util::BodyExt;

use crate::executor::ScriptExecutor;
use crate::profiler;
use crate::types::{ScriptRequest, ScriptResponse, UploadedFile};

const MAX_UPLOAD_SIZE: u64 = 10 * 1024 * 1024;

// Pre-allocated static bytes for common responses
static EMPTY_BODY: Bytes = Bytes::from_static(b"");
static NOT_FOUND_BODY: Bytes = Bytes::from_static(b"404 Not Found");
static METHOD_NOT_ALLOWED_BODY: Bytes = Bytes::from_static(b"Method Not Allowed");
static BAD_REQUEST_BODY: Bytes = Bytes::from_static(b"Failed to read request body");

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
}

impl ServerConfig {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            document_root: Arc::from("/var/www/html"),
            num_workers: 0, // auto-detect
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
}

/// HTTP server with pluggable script executor.
pub struct Server<E: ScriptExecutor> {
    config: ServerConfig,
    executor: Arc<E>,
}

impl<E: ScriptExecutor + 'static> Server<E> {
    pub fn new(config: ServerConfig, executor: E) -> Self {
        Self {
            config,
            executor: Arc::new(executor),
        }
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

        info!(
            "Server listening on http://{} (executor: {}, workers: {})",
            self.config.addr,
            self.executor.name(),
            num_workers
        );

        // Spawn accept loops on multiple threads
        let mut handles = Vec::with_capacity(num_workers);

        for worker_id in 0..num_workers {
            let addr = self.config.addr;
            let executor = Arc::clone(&self.executor);
            let document_root = Arc::clone(&self.config.document_root);
            let skip_file_check = self.executor.skip_file_check();

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

                    let io = TokioIo::new(stream);
                    let executor = Arc::clone(&executor);
                    let document_root = Arc::clone(&document_root);

                    tokio::task::spawn(async move {
                        let service = service_fn(move |req| {
                            let executor = Arc::clone(&executor);
                            let doc_root = Arc::clone(&document_root);
                            async move {
                                handle_request(req, remote_addr, executor, doc_root, skip_file_check).await
                            }
                        });

                        if let Err(err) = http1::Builder::new()
                            .keep_alive(true)
                            .pipeline_flush(true)
                            .serve_connection(io, service)
                            .await
                        {
                            let err_str = format!("{:?}", err);
                            if !is_connection_error(&err_str) {
                                debug!("Connection error: {:?}", err);
                            }
                        }
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

async fn handle_request<E: ScriptExecutor>(
    req: Request<IncomingBody>,
    remote_addr: SocketAddr,
    executor: Arc<E>,
    document_root: Arc<str>,
    skip_file_check: bool,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let is_head = *req.method() == Method::HEAD;

    let response = match *req.method() {
        Method::GET | Method::POST | Method::HEAD => {
            let mut resp = process_request(req, remote_addr, executor, document_root, skip_file_check).await;

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
) -> Response<Full<Bytes>> {
    use std::time::Instant;

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
    let uri = req.uri().clone();
    let uri_path = uri.path();
    let query_string = uri.query().unwrap_or("");

    // Check for profiling header
    let profile_requested = req
        .headers()
        .get("x-profile")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let profiling_enabled = profile_requested && profiler::is_enabled();

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
    let content_type_str = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let cookie_header_str = req
        .headers()
        .get("cookie")
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

    // Build server variables
    let server_vars_start = Instant::now();
    let has_cookie = !cookie_header_str.is_empty();
    let capacity = if has_cookie { 9 } else { 8 };
    let mut server_vars = Vec::with_capacity(capacity);

    server_vars.push(("REQUEST_METHOD".into(), method.as_str().to_string()));
    server_vars.push(("REQUEST_URI".into(), uri.to_string()));
    server_vars.push(("QUERY_STRING".into(), query_string.to_string()));
    server_vars.push(("REMOTE_ADDR".into(), remote_addr.ip().to_string()));
    server_vars.push(("REMOTE_PORT".into(), remote_addr.port().to_string()));
    server_vars.push(("SERVER_SOFTWARE".into(), "tokio_php/0.1.0".into()));
    server_vars.push(("SERVER_PROTOCOL".into(), "HTTP/1.1".into()));
    server_vars.push(("CONTENT_TYPE".into(), content_type_str));
    if has_cookie {
        server_vars.push(("HTTP_COOKIE".into(), cookie_header_str));
    }
    if profiling_enabled {
        server_vars_us = server_vars_start.elapsed().as_micros() as u64;
    }

    // Decode URL path and resolve file
    let path_start = Instant::now();
    let decoded_path = percent_encoding::percent_decode_str(uri_path)
        .decode_utf8_lossy();

    let clean_path = decoded_path
        .trim_start_matches('/')
        .replace("..", "");

    let file_path = if clean_path.is_empty() || clean_path.ends_with('/') {
        format!("{}/{}/index.php", document_root, clean_path)
    } else {
        format!("{}/{}", document_root, clean_path)
    };

    let file_path = Path::new(&file_path);

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
                create_script_response_fast(resp, profiling_enabled)
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
        serve_static_file(file_path).await
    }
}

#[inline]
fn create_script_response_fast(script_response: ScriptResponse, profiling: bool) -> Response<Full<Bytes>> {
    // Fast path: no headers to process and no profiling
    if script_response.headers.is_empty() && !profiling {
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/html; charset=utf-8")
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
    let mut content_type = "text/html; charset=utf-8";
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
                // Use the value as-is, we'll copy it when building
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

    let mut builder = Response::builder()
        .status(status)
        .header("Server", "tokio_php/0.1.0");

    // Check if content-type was set
    let has_content_type = custom_headers.iter().any(|(n, _)| *n == "Content-Type");
    if !has_content_type {
        builder = builder.header("Content-Type", content_type);
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
        .body(Full::new(if script_response.body.is_empty() {
            EMPTY_BODY.clone()
        } else {
            Bytes::from(script_response.body)
        }))
        .unwrap()
}

#[inline]
fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| {
        matches!(b, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
                    b'0'..=b'9' | b'A'..=b'Z' | b'^' | b'_' | b'`' | b'a'..=b'z' | b'|' | b'~')
    })
}

async fn serve_static_file(file_path: &Path) -> Response<Full<Bytes>> {
    match tokio::fs::read(file_path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(file_path)
                .first_or_octet_stream()
                .to_string();

            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", mime)
                .body(Full::new(Bytes::from(contents)))
                .unwrap()
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
