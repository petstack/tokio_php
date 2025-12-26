//! HTTP response building and utilities.

pub mod compression;
pub mod static_file;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use crate::types::ScriptResponse;
use compression::{compress_brotli, should_compress_mime, MIN_COMPRESSION_SIZE};

pub use compression::accepts_brotli;
pub use static_file::serve_static_file;

// Pre-allocated static bytes for common responses
pub static EMPTY_BODY: Bytes = Bytes::from_static(b"");
pub static NOT_FOUND_BODY: Bytes = Bytes::from_static(b"404 Not Found");
pub static METHOD_NOT_ALLOWED_BODY: Bytes = Bytes::from_static(b"Method Not Allowed");
pub static BAD_REQUEST_BODY: Bytes = Bytes::from_static(b"Failed to read request body");

const DEFAULT_CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// Build an empty response for stub mode.
/// Optimized: minimal allocations, pre-computed header values.
#[inline]
pub fn empty_stub_response() -> Response<Full<Bytes>> {
    use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, SERVER};
    use hyper::http::HeaderValue;

    // Pre-computed static header values (no allocation)
    static CT_VALUE: HeaderValue = HeaderValue::from_static("text/html; charset=utf-8");
    static SERVER_VALUE: HeaderValue = HeaderValue::from_static("tokio_php/0.1.0");
    static ZERO_VALUE: HeaderValue = HeaderValue::from_static("0");

    let mut resp = Response::new(Full::new(EMPTY_BODY.clone()));
    let headers = resp.headers_mut();
    headers.insert(CONTENT_TYPE, CT_VALUE.clone());
    headers.insert(SERVER, SERVER_VALUE.clone());
    headers.insert(CONTENT_LENGTH, ZERO_VALUE.clone());
    resp
}

/// Create an error response with the given status and body.
#[inline]
pub fn error_response(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain")
        .header("Server", "tokio_php/0.1.0")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

/// Create a Not Found response.
#[inline]
pub fn not_found_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/html")
        .body(Full::new(NOT_FOUND_BODY.clone()))
        .unwrap()
}

/// Create a response from a PHP script execution result.
#[inline]
pub fn from_script_response(
    script_response: ScriptResponse,
    profiling: bool,
    use_brotli: bool,
) -> Response<Full<Bytes>> {
    // Fast path: no headers to process, no profiling, no compression
    if script_response.headers.is_empty() && !profiling && !use_brotli {
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", DEFAULT_CONTENT_TYPE)
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
    let mut actual_content_type = DEFAULT_CONTENT_TYPE.to_string();
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
    let should_compress =
        use_brotli && body_bytes.len() >= MIN_COMPRESSION_SIZE && should_compress_mime(&actual_content_type);

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
        builder = builder.header("Content-Type", DEFAULT_CONTENT_TYPE);
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

    builder.body(Full::new(final_body)).unwrap()
}

/// Check if a header name is valid per HTTP spec.
#[inline]
fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            matches!(b, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
                    b'0'..=b'9' | b'A'..=b'Z' | b'^' | b'_' | b'`' | b'a'..=b'z' | b'|' | b'~')
        })
}
