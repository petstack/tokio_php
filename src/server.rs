use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response, StatusCode, Method};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::php::PhpRuntime;

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
    let uri_path = req.uri().path().to_string();

    info!("{} {} - {}", method, uri_path, remote_addr);

    let response = match method {
        Method::GET | Method::POST => {
            process_request(&uri_path).await
        }
        _ => {
            create_response(StatusCode::METHOD_NOT_ALLOWED, "text/plain", "Method Not Allowed")
        }
    };

    Ok(response)
}

async fn process_request(uri_path: &str) -> Response<Full<Bytes>> {
    // Decode URL path
    let decoded_path = percent_encoding::percent_decode_str(uri_path)
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
        // Execute PHP file in blocking task
        execute_php_file(&file_path).await
    } else {
        // Serve static file
        serve_static_file(&file_path).await
    }
}

async fn execute_php_file(file_path: &Path) -> Response<Full<Bytes>> {
    let path_str = file_path.to_string_lossy().to_string();

    // PHP execution is blocking, run in spawn_blocking
    let result = tokio::task::spawn_blocking(move || {
        PhpRuntime::execute_file(&path_str)
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
