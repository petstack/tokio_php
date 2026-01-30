//! HTTP response building and utilities.

pub mod compression;
pub mod static_file;
pub mod streaming;

use bytes::Bytes;
use http_body_util::{Either, Full};
use hyper::{Response, StatusCode};

use crate::types::ScriptResponse;
use compression::{
    compress_brotli, should_compress_mime, MAX_COMPRESSION_SIZE, MIN_COMPRESSION_SIZE,
};

pub use compression::{accepts_brotli, STREAM_THRESHOLD_NON_COMPRESSIBLE};
pub use static_file::serve_static_file;
pub use streaming::{
    // File streaming exports
    file_streaming_response,
    is_sse_accept,
    is_sse_content_type,
    metered_streaming_response,
    open_file_stream,
    sse_response,
    stream_channel,
    streaming_response,
    FileBody,
    FileResponse,
    MeteredStreamingBody,
    MeteredStreamingResponse,
    StreamChunk,
    StreamingBody,
    StreamingResponse,
    DEFAULT_STREAM_BUFFER_SIZE,
};

/// Inner Either type for streaming bodies (SSE/chunked, metered SSE, or file).
type StreamOrFileBody = Either<Either<StreamingBody, MeteredStreamingBody>, FileBody>;

/// Response body that can be full (buffered), streaming (SSE/chunked), or file streaming.
///
/// This type allows handlers to return:
/// - Complete responses loaded in memory
/// - Streaming responses for SSE and chunked transfer
/// - File streaming responses for large files
pub type FlexibleBody = Either<Full<Bytes>, StreamOrFileBody>;

/// HTTP response with flexible body (full, streaming, or file).
pub type FlexibleResponse = Response<FlexibleBody>;

/// Convert a full response to a flexible response.
#[inline]
pub fn full_to_flexible(resp: Response<Full<Bytes>>) -> FlexibleResponse {
    resp.map(Either::Left)
}

/// Convert a streaming response to a flexible response.
#[inline]
pub fn streaming_to_flexible(resp: StreamingResponse) -> FlexibleResponse {
    resp.map(|body| Either::Right(Either::Left(Either::Left(body))))
}

/// Convert a metered streaming response to a flexible response.
#[inline]
pub fn metered_streaming_to_flexible(resp: MeteredStreamingResponse) -> FlexibleResponse {
    resp.map(|body| Either::Right(Either::Left(Either::Right(body))))
}

/// Convert a file streaming response to a flexible response.
#[inline]
pub fn file_to_flexible(resp: FileResponse) -> FlexibleResponse {
    resp.map(|body| Either::Right(Either::Right(body)))
}

// Pre-allocated static bytes for common responses
pub static EMPTY_BODY: Bytes = Bytes::from_static(b"");
pub static METHOD_NOT_ALLOWED_BODY: Bytes = Bytes::from_static(b"Method Not Allowed");
pub static BAD_REQUEST_BODY: Bytes = Bytes::from_static(b"Failed to read request body");

const DEFAULT_CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// Build a pre-built empty response for stub mode.
#[inline]
pub fn empty_stub_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", DEFAULT_CONTENT_TYPE)
        .header("Server", "tokio_php/0.1.0")
        .header("Content-Length", "0")
        .body(Full::new(EMPTY_BODY.clone()))
        .unwrap()
}

/// Build stub response with profiling headers.
#[inline]
pub fn stub_response_with_profile(
    total_us: u64,
    http_version: &str,
    tls_handshake_us: u64,
    tls_protocol: &str,
    tls_alpn: &str,
) -> Response<Full<Bytes>> {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", DEFAULT_CONTENT_TYPE)
        .header("Server", "tokio_php/0.1.0")
        .header("Content-Length", "0")
        // Profile headers
        .header("X-Profile-Total-Us", total_us.to_string())
        .header("X-Profile-HTTP-Version", http_version)
        .header("X-Profile-Executor", "stub");

    // TLS headers (only if TLS was used)
    if tls_handshake_us > 0 {
        builder = builder.header("X-Profile-TLS-Handshake-Us", tls_handshake_us.to_string());
    }
    if !tls_protocol.is_empty() {
        builder = builder.header("X-Profile-TLS-Protocol", tls_protocol);
    }
    if !tls_alpn.is_empty() {
        builder = builder.header("X-Profile-TLS-ALPN", tls_alpn);
    }

    builder.body(Full::new(EMPTY_BODY.clone())).unwrap()
}

/// Create a Not Found response with empty body (for error page injection).
#[inline]
pub fn not_found_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/html")
        .body(Full::new(EMPTY_BODY.clone()))
        .unwrap()
}

/// Create a response from a PHP script execution result.
#[inline]
pub fn from_script_response(
    mut script_response: ScriptResponse,
    profiling: bool,
    use_brotli: bool,
) -> Response<Full<Bytes>> {
    use std::time::Instant;

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

    let response_build_start = Instant::now();

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
    let original_size = body_bytes.len();
    let should_compress = use_brotli
        && (MIN_COMPRESSION_SIZE..=MAX_COMPRESSION_SIZE).contains(&original_size)
        && should_compress_mime(&actual_content_type);

    let compression_start = Instant::now();
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
    let compression_us = if profiling && should_compress {
        compression_start.elapsed().as_micros() as u64
    } else {
        0
    };
    let compression_ratio = if is_compressed && original_size > 0 {
        final_body.len() as f32 / original_size as f32
    } else {
        0.0
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

    // Update profile data if profiling is enabled
    if profiling {
        if let Some(ref mut profile) = script_response.profile {
            // Add response building timing
            profile.response_build_us = response_build_start.elapsed().as_micros() as u64;
            profile.compression_us = compression_us;
            profile.compression_ratio = compression_ratio;

            // Recalculate total including response build time
            profile.total_us = profile.total_us.saturating_add(profile.response_build_us);

            // Note: With debug-profile feature, profile data is written to file
            // in connection.rs instead of being added as headers.
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
