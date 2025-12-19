use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response, StatusCode, Method};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::php::{PhpRequest, PhpRuntime};

const DOCUMENT_ROOT: &str = "/var/www/html";

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
            let (stream, remote_addr) = listener.accept().await?;
            let io = TokioIo::new(stream);

            tokio::task::spawn(async move {
                let service = service_fn(move |req| handle_request(req, remote_addr));

                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                {
                    error!("Error serving connection: {:?}", err);
                }
            });
        }
    }
}

async fn handle_request(
    req: Request<IncomingBody>,
    remote_addr: SocketAddr,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let uri_path = uri.path().to_string();

    info!("{} {} - {}", method, uri_path, remote_addr);

    let response = match method {
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

async fn process_request(
    req: Request<IncomingBody>,
    remote_addr: SocketAddr,
) -> Response<Full<Bytes>> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let uri_path = uri.path().to_string();
    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Parse cookies from Cookie header
    let cookie_header = req
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let cookies = if cookie_header.is_empty() {
        HashMap::new()
    } else {
        parse_cookies(&cookie_header)
    };

    // Parse query string for GET parameters
    let get_params = uri
        .query()
        .map(|q| parse_query_string(q))
        .unwrap_or_default();

    // Read and parse POST body
    let post_params = if method == Method::POST {
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                error!("Failed to read request body: {}", e);
                return create_response(
                    StatusCode::BAD_REQUEST,
                    "text/plain",
                    "Failed to read request body",
                );
            }
        };

        if content_type.starts_with("application/x-www-form-urlencoded") {
            let body_str = String::from_utf8_lossy(&body_bytes);
            parse_form_urlencoded(&body_str)
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    // Build server variables
    let mut server_vars = HashMap::new();
    server_vars.insert("REQUEST_METHOD".to_string(), method.to_string());
    server_vars.insert("REQUEST_URI".to_string(), uri.to_string());
    server_vars.insert("QUERY_STRING".to_string(), uri.query().unwrap_or("").to_string());
    server_vars.insert("REMOTE_ADDR".to_string(), remote_addr.ip().to_string());
    server_vars.insert("REMOTE_PORT".to_string(), remote_addr.port().to_string());
    server_vars.insert("SERVER_SOFTWARE".to_string(), "tokio_php/0.1.0".to_string());
    server_vars.insert("SERVER_PROTOCOL".to_string(), "HTTP/1.1".to_string());
    server_vars.insert("CONTENT_TYPE".to_string(), content_type);
    if !cookie_header.is_empty() {
        server_vars.insert("HTTP_COOKIE".to_string(), cookie_header.clone());
    }

    // Decode URL path
    let decoded_path = percent_encoding::percent_decode_str(&uri_path)
        .decode_utf8_lossy()
        .to_string();

    // Sanitize path to prevent directory traversal
    let clean_path = decoded_path
        .trim_start_matches('/')
        .replace("..", "");

    let file_path = if clean_path.is_empty() || clean_path.ends_with('/') {
        PathBuf::from(DOCUMENT_ROOT).join(&clean_path).join("index.php")
    } else {
        PathBuf::from(DOCUMENT_ROOT).join(&clean_path)
    };

    // Check if file exists
    if !file_path.exists() {
        return create_response(StatusCode::NOT_FOUND, "text/html", "404 Not Found");
    }

    // Check file extension
    let extension = file_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if extension == "php" {
        let php_request = PhpRequest {
            script_path: file_path.to_string_lossy().to_string(),
            get_params,
            post_params,
            cookies,
            server_vars,
        };
        execute_php_file(php_request).await
    } else {
        serve_static_file(&file_path).await
    }
}

async fn execute_php_file(request: PhpRequest) -> Response<Full<Bytes>> {
    let result = tokio::task::spawn_blocking(move || {
        PhpRuntime::execute_request(request)
    }).await;

    match result {
        Ok(Ok(output)) => {
            create_response(StatusCode::OK, "text/html; charset=utf-8", &output)
        }
        Ok(Err(e)) => {
            error!("PHP execution error: {}", e);
            create_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "text/html",
                &format!("<h1>500 Internal Server Error</h1><pre>{}</pre>", e),
            )
        }
        Err(e) => {
            error!("Task join error: {}", e);
            create_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "text/plain",
                "Internal Server Error",
            )
        }
    }
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
