//! Internal HTTP server for health and metrics endpoints.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming as IncomingBody;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

/// Run the internal HTTP server for /health and /metrics endpoints.
pub async fn run_internal_server(
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
            let _ = http1::Builder::new().serve_connection(io, service).await;
        });
    }
}

/// Handle internal server requests (/health, /metrics).
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
                now.as_secs(),
                active_connections
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        "/metrics" => {
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
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/plain")
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap(),
    };

    Ok(response)
}
