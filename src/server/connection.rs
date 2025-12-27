//! TCP/TLS connection handling.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming as IncomingBody;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error};

use super::config::TlsInfo;
use super::request::{parse_cookies, parse_multipart, parse_query_string};
use super::response::{
    accepts_brotli, empty_stub_response, from_script_response, not_found_response,
    serve_static_file, BAD_REQUEST_BODY, EMPTY_BODY, METHOD_NOT_ALLOWED_BODY,
};
use super::routing::{is_direct_index_access, is_php_uri, resolve_file_path};
use crate::executor::ScriptExecutor;
use crate::profiler;
use crate::types::ScriptRequest;

/// Check if an error is a common connection reset or timeout.
#[inline]
fn is_connection_error(err_str: &str) -> bool {
    err_str.contains("connection reset")
        || err_str.contains("broken pipe")
        || err_str.contains("Connection reset")
        || err_str.contains("os error 104")
        || err_str.contains("os error 32")
        || err_str.contains("timed out")
        || err_str.contains("deadline has elapsed")
}

use super::internal::RequestMetrics;

/// Connection handler context.
pub struct ConnectionContext<E: ScriptExecutor> {
    pub executor: Arc<E>,
    pub document_root: Arc<str>,
    pub skip_file_check: bool,
    pub is_stub_mode: bool,
    pub index_file_path: Option<Arc<str>>,
    pub index_file_name: Option<Arc<str>>,
    pub active_connections: Arc<AtomicUsize>,
    pub request_metrics: Arc<RequestMetrics>,
}

impl<E: ScriptExecutor + 'static> ConnectionContext<E> {
    /// Handle an incoming TCP connection (with optional TLS).
    pub async fn handle_connection(
        self: Arc<Self>,
        stream: TcpStream,
        remote_addr: SocketAddr,
        tls_acceptor: Option<TlsAcceptor>,
    ) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);

        if let Some(acceptor) = tls_acceptor {
            self.clone()
                .handle_tls_connection(stream, remote_addr, acceptor)
                .await;
        } else {
            self.clone()
                .handle_plain_connection(stream, remote_addr)
                .await;
        }

        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    async fn handle_tls_connection(
        self: Arc<Self>,
        stream: TcpStream,
        remote_addr: SocketAddr,
        acceptor: TlsAcceptor,
    ) {
        let tls_start = Instant::now();

        // TLS handshake with timeout
        let tls_stream = match tokio::time::timeout(
            Duration::from_secs(10),
            acceptor.accept(stream),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                debug!("TLS handshake failed: {:?}", e);
                return;
            }
            Err(_) => {
                debug!("TLS handshake timeout: {:?}", remote_addr);
                return;
            }
        };

        let handshake_us = tls_start.elapsed().as_micros() as u64;

        // Extract TLS info from the connection
        let (_, server_conn) = tls_stream.get_ref();
        let tls_info = TlsInfo {
            handshake_us,
            protocol: server_conn
                .protocol_version()
                .map(|v| format!("{:?}", v))
                .unwrap_or_default(),
            alpn: server_conn
                .alpn_protocol()
                .map(|p| String::from_utf8_lossy(p).to_string())
                .unwrap_or_default(),
        };

        let ctx = Arc::clone(&self);
        let service = service_fn(move |req| {
            let ctx = Arc::clone(&ctx);
            let tls = tls_info.clone();
            async move { ctx.handle_request(req, remote_addr, Some(tls)).await }
        });

        let io = TokioIo::new(tls_stream);
        if let Err(err) = auto::Builder::new(TokioExecutor::new())
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(Some(Duration::from_secs(5)))
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

    async fn handle_plain_connection(self: Arc<Self>, stream: TcpStream, remote_addr: SocketAddr) {
        // Wait for first byte with timeout to detect idle connections (skip for stub mode)
        if !self.is_stub_mode {
            let mut peek_buf = [0u8; 1];
            match tokio::time::timeout(Duration::from_secs(10), stream.peek(&mut peek_buf)).await {
                Ok(Ok(0)) | Err(_) => {
                    // Connection closed or timeout - client connected but sent nothing
                    debug!("Connection idle timeout or closed: {:?}", remote_addr);
                    return;
                }
                Ok(Err(e)) => {
                    debug!("Peek error: {:?}", e);
                    return;
                }
                Ok(Ok(_)) => {
                    // Data available, proceed
                }
            }
        }

        let ctx = Arc::clone(&self);
        let service = service_fn(move |req| {
            let ctx = Arc::clone(&ctx);
            async move { ctx.handle_request(req, remote_addr, None).await }
        });

        let io = TokioIo::new(stream);
        if let Err(err) = auto::Builder::new(TokioExecutor::new())
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(Some(Duration::from_secs(5)))
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

    async fn handle_request(
        &self,
        req: Request<IncomingBody>,
        remote_addr: SocketAddr,
        tls_info: Option<TlsInfo>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        let request_start = Instant::now();

        // Increment request method metrics
        self.request_metrics.increment_method(req.method());

        let is_head = *req.method() == Method::HEAD;

        let response = match *req.method() {
            Method::GET | Method::POST | Method::HEAD => {
                let mut resp = self.process_request(req, remote_addr, tls_info).await;

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
                .unwrap(),
        };

        // Record response time and status metrics
        let response_time_us = request_start.elapsed().as_micros() as u64;
        self.request_metrics.record_response_time(response_time_us);
        self.request_metrics.increment_status(response.status().as_u16());

        Ok(response)
    }

    async fn process_request(
        &self,
        req: Request<IncomingBody>,
        remote_addr: SocketAddr,
        tls_info: Option<TlsInfo>,
    ) -> Response<Full<Bytes>> {
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
        }
        .to_string();
        let uri = req.uri().clone();
        let uri_path = uri.path();
        let query_string = uri.query().unwrap_or("");

        // Block direct access to index file in single entry point mode
        if is_direct_index_access(uri_path, self.index_file_name.as_ref()) {
            return not_found_response();
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

        // Fast path for stub mode only
        if self.is_stub_mode && is_php_uri(uri_path) {
            return empty_stub_response();
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
                            .body(Full::new(Bytes::from(format!(
                                "Failed to parse multipart form: {}",
                                e
                            ))))
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

        // Resolve file path
        let path_start = Instant::now();
        let file_path_string =
            resolve_file_path(uri_path, &self.document_root, self.index_file_path.as_ref());
        let file_path = Path::new(&file_path_string);

        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if profiling_enabled {
            path_resolve_us = path_start.elapsed().as_micros() as u64;
        }

        // Check if file exists (sync - fast for stat syscall)
        let file_check_start = Instant::now();
        if !self.skip_file_check && !file_path.exists() {
            return not_found_response();
        }
        if profiling_enabled {
            file_check_us = file_check_start.elapsed().as_micros() as u64;
        }

        // Build server variables
        let server_vars_start = Instant::now();

        // Parse Host header for SERVER_NAME and SERVER_PORT
        let (server_name, server_port) = if !host_header.is_empty() {
            if let Some(colon_pos) = host_header.rfind(':') {
                if host_header.starts_with('[') && !host_header.contains("]:") {
                    (
                        host_header.clone(),
                        if tls_info.is_some() { "443" } else { "80" }.to_string(),
                    )
                } else {
                    (
                        host_header[..colon_pos].to_string(),
                        host_header[colon_pos + 1..].to_string(),
                    )
                }
            } else {
                (
                    host_header.clone(),
                    if tls_info.is_some() { "443" } else { "80" }.to_string(),
                )
            }
        } else {
            (
                "localhost".to_string(),
                if tls_info.is_some() { "443" } else { "80" }.to_string(),
            )
        };

        // Calculate SCRIPT_NAME and PHP_SELF
        let script_name = file_path_string
            .strip_prefix(self.document_root.as_ref())
            .unwrap_or(&file_path_string)
            .to_string();
        let script_name = if script_name.starts_with('/') {
            script_name
        } else {
            format!("/{}", script_name)
        };

        let path_info = String::new();

        let mut server_vars = Vec::with_capacity(32);

        // Request timing
        server_vars.push(("REQUEST_TIME".into(), request_time_secs.to_string()));
        server_vars.push((
            "REQUEST_TIME_FLOAT".into(),
            format!("{:.6}", request_time_float),
        ));

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
        server_vars.push(("SERVER_ADDR".into(), "0.0.0.0".into()));
        server_vars.push(("SERVER_SOFTWARE".into(), "tokio_php/0.1.0".into()));
        server_vars.push(("SERVER_PROTOCOL".into(), http_version.clone()));
        server_vars.push(("DOCUMENT_ROOT".into(), self.document_root.to_string()));
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

        // HTTP headers
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
            let temp_files: Vec<String> = files
                .iter()
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

            // Track pending requests for metrics (guard ensures cleanup on cancel)
            let _pending_guard = RequestMetrics::pending_guard(&self.request_metrics);
            let execute_result = self.executor.execute(script_request).await;

            let response = match execute_result {
                Ok(mut resp) => {
                    // Add parse breakdown to profile data if profiling
                    if profiling_enabled {
                        if let Some(ref mut profile) = resp.profile {
                            profile.http_version = http_version.clone();
                            if let Some(ref tls) = tls_info {
                                profile.tls_handshake_us = tls.handshake_us;
                                profile.tls_protocol = tls.protocol.clone();
                                profile.tls_alpn = tls.alpn.clone();
                            }

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
                    from_script_response(resp, profiling_enabled, use_brotli)
                }
                Err(e) => {
                    if e.is_queue_full() {
                        // Queue is full - server overloaded
                        self.request_metrics.inc_dropped();
                        Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .header("Content-Type", "text/plain")
                            .header("Retry-After", "1")
                            .body(Full::new(Bytes::from_static(b"503 Service Unavailable - Server overloaded")))
                            .unwrap()
                    } else {
                        error!("Script execution error: {}", e);
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header("Content-Type", "text/html")
                            .body(Full::new(Bytes::from(format!(
                                "<h1>500 Internal Server Error</h1><pre>{}</pre>",
                                e
                            ))))
                            .unwrap()
                    }
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
}
