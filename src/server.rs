use bytes::Bytes;
use futures_util::stream;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response, StatusCode, Method};
use hyper_util::rt::TokioIo;
use multer::Multipart;
use socket2::{SockRef, TcpKeepalive};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tracing::{error, info, debug};
use uuid::Uuid;

use crate::php::{PhpRequest, PhpResponse, PhpRuntime, UploadedFile};

const DOCUMENT_ROOT: &str = "/var/www/html";
const MAX_UPLOAD_SIZE: u64 = 10 * 1024 * 1024; // 10MB max file size

pub struct Server {
    addr: SocketAddr,
}

impl Server {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("Server listening on http://{}", self.addr);

        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Accept error: {}", e);
                    continue;
                }
            };

            // Optimize TCP settings
            let sock_ref = SockRef::from(&stream);
            let _ = sock_ref.set_nodelay(true);
            let keepalive = TcpKeepalive::new()
                .with_time(Duration::from_secs(60));
            let _ = sock_ref.set_tcp_keepalive(&keepalive);

            let io = TokioIo::new(stream);

            tokio::task::spawn(async move {
                let service = service_fn(move |req| handle_request(req, remote_addr));

                if let Err(err) = http1::Builder::new()
                    .keep_alive(true)
                    .serve_connection(io, service)
                    .await
                {
                    // Only log unexpected errors (not connection resets, broken pipes, etc.)
                    let err_str = format!("{:?}", err);
                    if !err_str.contains("connection reset")
                        && !err_str.contains("broken pipe")
                        && !err_str.contains("Connection reset")
                        && !err_str.contains("os error 104")  // ECONNRESET
                        && !err_str.contains("os error 32")   // EPIPE
                    {
                        debug!("Connection error: {:?}", err);
                    }
                }
            });
        }
    }
}

async fn handle_request(
    req: Request<IncomingBody>,
    remote_addr: SocketAddr,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let response = match *req.method() {
        Method::GET | Method::POST => {
            process_request(req, remote_addr).await
        }
        _ => {
            create_response(StatusCode::METHOD_NOT_ALLOWED, "text/plain", "Method Not Allowed")
        }
    };

    Ok(response)
}

fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");

        // URL decode
        let key = percent_encoding::percent_decode_str(key)
            .decode_utf8_lossy()
            .to_string();
        let value = percent_encoding::percent_decode_str(value)
            .decode_utf8_lossy()
            .to_string();

        if !key.is_empty() {
            params.insert(key, value);
        }
    }

    params
}

fn parse_form_urlencoded(body: &str) -> HashMap<String, String> {
    parse_query_string(body)
}

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
            // URL decode cookie value
            let value = percent_encoding::percent_decode_str(value)
                .decode_utf8_lossy()
                .to_string();
            cookies.insert(name.to_string(), value);
        }
    }

    cookies
}

async fn parse_multipart(
    content_type: &str,
    body: Bytes,
) -> Result<(HashMap<String, String>, HashMap<String, Vec<UploadedFile>>), String> {
    // Extract boundary from content-type header
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
            // This is a file upload
            if original_name.is_empty() {
                // Empty file field, skip
                continue;
            }

            let data = field.bytes().await.map_err(|e| e.to_string())?;
            let size = data.len() as u64;

            // Normalize field name: "files[]" -> "files"
            let normalized_name = if field_name.ends_with("[]") {
                field_name[..field_name.len() - 2].to_string()
            } else {
                field_name
            };

            // Check file size limit
            if size > MAX_UPLOAD_SIZE {
                // UPLOAD_ERR_INI_SIZE = 1: file exceeds upload_max_filesize
                files.entry(normalized_name).or_default().push(UploadedFile {
                    name: original_name,
                    mime_type: content_type,
                    tmp_name: String::new(),
                    size,
                    error: 1,
                });
                continue;
            }

            // Generate unique temp filename
            let tmp_name = format!("/tmp/php{}", Uuid::new_v4().to_string().replace("-", ""));

            // Write file to temp location
            let mut file = File::create(&tmp_name).await.map_err(|e| e.to_string())?;
            file.write_all(&data).await.map_err(|e| e.to_string())?;
            file.flush().await.map_err(|e| e.to_string())?;

            files.entry(normalized_name).or_default().push(UploadedFile {
                name: original_name,
                mime_type: content_type,
                tmp_name,
                size,
                error: 0, // UPLOAD_ERR_OK
            });
        } else {
            // This is a regular form field
            let value = field.text().await.map_err(|e| e.to_string())?;
            params.insert(field_name, value);
        }
    }

    Ok((params, files))
}

async fn process_request(
    req: Request<IncomingBody>,
    remote_addr: SocketAddr,
) -> Response<Full<Bytes>> {
    // Extract all needed values before consuming req
    let method = req.method().clone();
    let uri_path = req.uri().path().to_string();
    let query_string = req.uri().query().unwrap_or("").to_string();
    let uri_str = req.uri().to_string();
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

    // Parse cookies
    let cookies = if cookie_header_str.is_empty() {
        HashMap::new()
    } else {
        parse_cookies(&cookie_header_str)
    };

    // Parse query string for GET parameters
    let get_params = if query_string.is_empty() {
        HashMap::new()
    } else {
        parse_query_string(&query_string)
    };

    // Read and parse POST body
    let (post_params, files) = if method == Method::POST {
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => {
                return create_response(
                    StatusCode::BAD_REQUEST,
                    "text/plain",
                    "Failed to read request body",
                );
            }
        };

        if content_type_str.starts_with("application/x-www-form-urlencoded") {
            let body_str = String::from_utf8_lossy(&body_bytes);
            (parse_form_urlencoded(&body_str), HashMap::new())
        } else if content_type_str.starts_with("multipart/form-data") {
            match parse_multipart(&content_type_str, body_bytes).await {
                Ok((params, uploaded_files)) => (params, uploaded_files),
                Err(e) => {
                    return create_response(
                        StatusCode::BAD_REQUEST,
                        "text/plain",
                        &format!("Failed to parse multipart form: {}", e),
                    );
                }
            }
        } else {
            (HashMap::new(), HashMap::new())
        }
    } else {
        (HashMap::new(), HashMap::new())
    };

    // Build server variables with capacity hint
    let mut server_vars = HashMap::with_capacity(9);
    server_vars.insert("REQUEST_METHOD".into(), method.to_string());
    server_vars.insert("REQUEST_URI".into(), uri_str);
    server_vars.insert("QUERY_STRING".into(), query_string);
    server_vars.insert("REMOTE_ADDR".into(), remote_addr.ip().to_string());
    server_vars.insert("REMOTE_PORT".into(), remote_addr.port().to_string());
    server_vars.insert("SERVER_SOFTWARE".into(), "tokio_php/0.1.0".into());
    server_vars.insert("SERVER_PROTOCOL".into(), "HTTP/1.1".into());
    server_vars.insert("CONTENT_TYPE".into(), content_type_str);
    if !cookie_header_str.is_empty() {
        server_vars.insert("HTTP_COOKIE".into(), cookie_header_str);
    }

    // Decode URL path
    let decoded_path = percent_encoding::percent_decode_str(&uri_path)
        .decode_utf8_lossy();

    // Sanitize path to prevent directory traversal
    let clean_path = decoded_path
        .trim_start_matches('/')
        .replace("..", "");

    let file_path = if clean_path.is_empty() || clean_path.ends_with('/') {
        PathBuf::from(DOCUMENT_ROOT).join(&clean_path).join("index.php")
    } else {
        PathBuf::from(DOCUMENT_ROOT).join(&clean_path)
    };

    // Check file extension first (fast path)
    let extension = file_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Check if file exists (async)
    if tokio::fs::metadata(&file_path).await.is_err() {
        return create_response(StatusCode::NOT_FOUND, "text/html", "404 Not Found");
    }

    if extension == "php" {
        // Collect temp file paths for cleanup
        let temp_files: Vec<String> = files.values()
            .flat_map(|file_vec| file_vec.iter().map(|f| f.tmp_name.clone()))
            .filter(|path| !path.is_empty())
            .collect();

        let php_request = PhpRequest {
            script_path: file_path.to_string_lossy().to_string(),
            get_params,
            post_params,
            cookies,
            server_vars,
            files,
        };
        let response = execute_php_file(php_request).await;

        // Clean up temp files after request completes
        for temp_file in temp_files {
            if let Err(e) = tokio::fs::remove_file(&temp_file).await {
                // Only log if file exists but couldn't be deleted
                // (PHP script might have moved/deleted it)
                if e.kind() != std::io::ErrorKind::NotFound {
                    error!("Failed to clean up temp file {}: {}", temp_file, e);
                }
            }
        }

        response
    } else {
        serve_static_file(&file_path).await
    }
}

async fn execute_php_file(request: PhpRequest) -> Response<Full<Bytes>> {
    match PhpRuntime::execute_request(request).await {
        Ok(response) => {
            create_php_response(StatusCode::OK, response)
        }
        Err(e) => {
            error!("PHP execution error: {}", e);
            create_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "text/html",
                &format!("<h1>500 Internal Server Error</h1><pre>{}</pre>", e),
            )
        }
    }
}

/// Validate HTTP header name according to RFC 7230
fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| {
        matches!(b, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
                    b'0'..=b'9' | b'A'..=b'Z' | b'^' | b'_' | b'`' | b'a'..=b'z' | b'|' | b'~')
    })
}

/// Sanitize header value (remove control characters)
fn sanitize_header_value(value: &str) -> String {
    value.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}

fn create_php_response(_default_status: StatusCode, php_response: PhpResponse) -> Response<Full<Bytes>> {
    let mut status = StatusCode::OK;
    let mut content_type = "text/html; charset=utf-8".to_string();
    let mut custom_headers: Vec<(String, String)> = Vec::new();

    // Process PHP headers
    for (name, value) in &php_response.headers {
        let name_lower = name.to_lowercase();

        // Check for HTTP status line (e.g., "HTTP/1.1 302 Found")
        if name_lower.starts_with("http/") {
            // This is a status line, parse the status code
            if let Some(code_str) = value.split_whitespace().next() {
                if let Ok(code) = code_str.parse::<u16>() {
                    // Skip 1xx informational status codes (not supported in HTTP/1.1 by hyper)
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
                content_type = sanitize_header_value(value);
            }
            "location" => {
                // Redirect - set status to 302 if not already set to a 3xx
                if !status.is_redirection() {
                    status = StatusCode::FOUND;
                }
                custom_headers.push((name.clone(), sanitize_header_value(value)));
            }
            "status" => {
                // CGI-style status header (e.g., "Status: 404 Not Found")
                if let Some(code_str) = value.split_whitespace().next() {
                    if let Ok(code) = code_str.parse::<u16>() {
                        // Skip 1xx informational status codes (not supported in HTTP/1.1 by hyper)
                        if code >= 200 {
                            if let Ok(s) = StatusCode::from_u16(code) {
                                status = s;
                            }
                        }
                    }
                }
            }
            _ => {
                // Validate header name before adding
                if is_valid_header_name(name) {
                    custom_headers.push((name.clone(), sanitize_header_value(value)));
                }
            }
        }
    }

    let mut builder = Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .header("Server", "tokio_php/0.1.0");

    // Add custom headers from PHP (already validated)
    for (name, value) in custom_headers {
        builder = builder.header(name, value);
    }

    builder
        .body(Full::new(Bytes::from(php_response.body)))
        .unwrap_or_else(|e| {
            error!("Failed to build response: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "text/plain")
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap()
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
            create_response(StatusCode::NOT_FOUND, "text/plain", "File not found")
        }
    }
}

fn create_response(status: StatusCode, content_type: &str, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .header("Server", "tokio_php/0.1.0")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}
