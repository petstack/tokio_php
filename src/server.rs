use bytes::Bytes;
use futures_util::stream;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response, StatusCode, Method};
use hyper_util::rt::TokioIo;
use multer::Multipart;
use std::collections::HashMap;
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
}

impl ServerConfig {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            document_root: Arc::from("/var/www/html"),
        }
    }

    pub fn with_document_root(mut self, path: &str) -> Self {
        self.document_root = Arc::from(path);
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

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(self.config.addr).await?;

        // Set socket options for better performance
        let std_listener = listener.into_std()?;
        let socket = socket2::Socket::from(std_listener);
        socket.set_reuse_address(true)?;
        socket.set_nodelay(true)?;
        let std_listener = std::net::TcpListener::from(socket);
        let listener = TcpListener::from_std(std_listener)?;

        info!(
            "Server listening on http://{} (executor: {})",
            self.config.addr,
            self.executor.name()
        );

        // Pre-compute skip_file_check once
        let skip_file_check = self.executor.skip_file_check();

        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Accept error: {}", e);
                    continue;
                }
            };

            // Optimize TCP settings
            if let Err(e) = stream.set_nodelay(true) {
                debug!("Failed to set TCP_NODELAY: {}", e);
            }

            let io = TokioIo::new(stream);
            let executor = Arc::clone(&self.executor);
            let document_root = Arc::clone(&self.config.document_root);

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
    let response = match *req.method() {
        Method::GET | Method::POST => {
            process_request(req, remote_addr, executor, document_root, skip_file_check).await
        }
        _ => Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header("Content-Type", "text/plain")
            .body(Full::new(METHOD_NOT_ALLOWED_BODY.clone()))
            .unwrap()
    };

    Ok(response)
}

#[inline]
fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");

        let key = percent_encoding::percent_decode_str(key)
            .decode_utf8_lossy()
            .into_owned();
        let value = percent_encoding::percent_decode_str(value)
            .decode_utf8_lossy()
            .into_owned();

        if !key.is_empty() {
            params.insert(key, value);
        }
    }

    params
}

#[inline]
fn parse_cookies(cookie_header: &str) -> HashMap<String, String> {
    let mut cookies = HashMap::new();

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if cookie.is_empty() {
            continue;
        }

        let mut parts = cookie.splitn(2, '=');
        let name = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();

        if !name.is_empty() {
            let value = percent_encoding::percent_decode_str(value)
                .decode_utf8_lossy()
                .into_owned();
            cookies.insert(name.to_string(), value);
        }
    }

    cookies
}

async fn parse_multipart(
    content_type: &str,
    body: Bytes,
) -> Result<(HashMap<String, String>, HashMap<String, Vec<UploadedFile>>), String> {
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

    let mut params = HashMap::new();
    let mut files: HashMap<String, Vec<UploadedFile>> = HashMap::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let field_name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().map(|s| s.to_string());
        let content_type = field.content_type().map(|m| m.to_string()).unwrap_or_default();

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

            if size > MAX_UPLOAD_SIZE {
                files.entry(normalized_name).or_default().push(UploadedFile {
                    name: original_name,
                    mime_type: content_type,
                    tmp_name: String::new(),
                    size,
                    error: 1,
                });
                continue;
            }

            let tmp_name = format!("/tmp/php{}", Uuid::new_v4().simple());

            let mut file = File::create(&tmp_name).await.map_err(|e| e.to_string())?;
            file.write_all(&data).await.map_err(|e| e.to_string())?;
            file.flush().await.map_err(|e| e.to_string())?;

            files.entry(normalized_name).or_default().push(UploadedFile {
                name: original_name,
                mime_type: content_type,
                tmp_name,
                size,
                error: 0,
            });
        } else {
            let value = field.text().await.map_err(|e| e.to_string())?;
            params.insert(field_name, value);
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
    let method = req.method().clone();
    let uri = req.uri().clone();
    let uri_path = uri.path();
    let query_string = uri.query().unwrap_or("");

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

    let cookies = if cookie_header_str.is_empty() {
        HashMap::new()
    } else {
        parse_cookies(&cookie_header_str)
    };

    let get_params = if query_string.is_empty() {
        HashMap::new()
    } else {
        parse_query_string(query_string)
    };

    let (post_params, files) = if method == Method::POST {
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

        if content_type_str.starts_with("application/x-www-form-urlencoded") {
            let body_str = String::from_utf8_lossy(&body_bytes);
            (parse_query_string(&body_str), HashMap::new())
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
            (HashMap::new(), HashMap::new())
        }
    } else {
        (HashMap::new(), HashMap::new())
    };

    // Build server variables
    let mut server_vars = HashMap::with_capacity(9);
    server_vars.insert("REQUEST_METHOD".into(), method.to_string());
    server_vars.insert("REQUEST_URI".into(), uri.to_string());
    server_vars.insert("QUERY_STRING".into(), query_string.to_string());
    server_vars.insert("REMOTE_ADDR".into(), remote_addr.ip().to_string());
    server_vars.insert("REMOTE_PORT".into(), remote_addr.port().to_string());
    server_vars.insert("SERVER_SOFTWARE".into(), "tokio_php/0.1.0".into());
    server_vars.insert("SERVER_PROTOCOL".into(), "HTTP/1.1".into());
    server_vars.insert("CONTENT_TYPE".into(), content_type_str);
    if !cookie_header_str.is_empty() {
        server_vars.insert("HTTP_COOKIE".into(), cookie_header_str);
    }

    // Decode URL path
    let decoded_path = percent_encoding::percent_decode_str(uri_path)
        .decode_utf8_lossy();

    // Sanitize path
    let clean_path = decoded_path
        .trim_start_matches('/')
        .replace("..", "");

    let file_path = if clean_path.is_empty() || clean_path.ends_with('/') {
        format!("{}/{}/index.php", document_root, clean_path)
    } else {
        format!("{}/{}", document_root, clean_path)
    };

    let file_path = Path::new(&file_path);

    // Check extension
    let extension = file_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Check if file exists
    if !skip_file_check && tokio::fs::metadata(&file_path).await.is_err() {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/html")
            .body(Full::new(NOT_FOUND_BODY.clone()))
            .unwrap();
    }

    if extension == "php" {
        let temp_files: Vec<String> = files.values()
            .flat_map(|file_vec| file_vec.iter().map(|f| f.tmp_name.clone()))
            .filter(|path| !path.is_empty())
            .collect();

        let script_request = ScriptRequest {
            script_path: file_path.to_string_lossy().into_owned(),
            get_params,
            post_params,
            cookies,
            server_vars,
            files,
        };

        let response = match executor.execute(script_request).await {
            Ok(resp) => create_script_response_fast(resp),
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
fn create_script_response_fast(script_response: ScriptResponse) -> Response<Full<Bytes>> {
    // Fast path: no headers to process
    if script_response.headers.is_empty() {
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
