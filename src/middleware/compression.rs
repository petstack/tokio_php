//! Brotli compression middleware.
//!
//! Compresses response bodies when:
//! - Client accepts Brotli encoding
//! - Response body is within size limits
//! - Content-Type is compressible

use crate::core::{Context, Request, Response};

use super::{Middleware, MiddlewareResult};

/// Minimum size to consider compression (smaller bodies don't benefit).
pub const MIN_COMPRESSION_SIZE: usize = 256;

/// Maximum size to compress (3 MB).
/// Bodies larger than this are NOT compressed (too CPU intensive).
pub const MAX_COMPRESSION_SIZE: usize = 3 * 1024 * 1024; // 3 MB

/// Brotli compression quality (0-11, higher = better but slower).
const BROTLI_QUALITY: u32 = 4;

/// Brotli compression window size.
const BROTLI_WINDOW: u32 = 20;

/// Check if the MIME type should be compressed.
#[inline]
pub fn should_compress_mime(content_type: &str) -> bool {
    let ct = content_type.split(';').next().unwrap_or("").trim();
    matches!(
        ct,
        // Text types
        "text/html"
            | "text/css"
            | "text/plain"
            | "text/xml"
            | "text/javascript"
            // Application types
            | "application/javascript"
            | "application/json"
            | "application/xml"
            | "application/xhtml+xml"
            | "application/rss+xml"
            | "application/atom+xml"
            | "application/manifest+json"
            | "application/ld+json"
            // SVG
            | "image/svg+xml"
            // Fonts (uncompressed formats)
            | "font/ttf"
            | "font/otf"
            | "application/x-font-ttf"
            | "application/x-font-opentype"
            | "application/vnd.ms-fontobject"
    )
}

/// Compress data using Brotli.
/// Returns None if compression would not reduce size.
#[inline]
pub fn compress_brotli(data: &[u8]) -> Option<Vec<u8>> {
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

/// Brotli compression middleware.
///
/// Compresses response bodies when appropriate, adding:
/// - `Content-Encoding: br` header
/// - `Vary: Accept-Encoding` header for caching
pub struct CompressionMiddleware {
    min_size: usize,
    max_size: usize,
}

impl CompressionMiddleware {
    /// Create a new compression middleware with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom size limits.
    pub fn with_limits(min_size: usize, max_size: usize) -> Self {
        Self { min_size, max_size }
    }

    /// Check if response should be compressed.
    fn should_compress(&self, res: &Response, ctx: &Context) -> bool {
        // Check if client accepts brotli
        if !ctx.accepts_brotli {
            return false;
        }

        // Check body size
        let body_len = res.body_len();
        if body_len < self.min_size || body_len > self.max_size {
            return false;
        }

        // Check if already compressed
        if res.header("content-encoding").is_some() {
            return false;
        }

        // Check content type
        if let Some(ct) = res.content_type() {
            should_compress_mime(ct)
        } else {
            // Default to compressing if no content-type (PHP output)
            true
        }
    }
}

impl Default for CompressionMiddleware {
    fn default() -> Self {
        Self {
            min_size: MIN_COMPRESSION_SIZE,
            max_size: MAX_COMPRESSION_SIZE,
        }
    }
}

impl Middleware for CompressionMiddleware {
    fn name(&self) -> &'static str {
        "compression"
    }

    fn priority(&self) -> i32 {
        100 // Run late, after other response modifications
    }

    fn on_request(&self, req: Request, ctx: &mut Context) -> MiddlewareResult {
        // Store accept-encoding info in context for response processing
        ctx.accepts_brotli = req.accepts_brotli();
        MiddlewareResult::Next(req)
    }

    fn on_response(&self, res: Response, ctx: &Context) -> Response {
        if !self.should_compress(&res, ctx) {
            return res;
        }

        // Attempt compression
        match compress_brotli(res.body()) {
            Some(compressed) => {
                tracing::trace!(
                    original = res.body_len(),
                    compressed = compressed.len(),
                    "compressed response"
                );

                res.with_body(compressed)
                    .with_header("Content-Encoding", "br")
                    .with_header("Vary", "Accept-Encoding")
            }
            None => {
                // Compression didn't help, return original
                res
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn create_context(accepts_brotli: bool) -> Context {
        Context::builder(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
            .trace_id("trace")
            .span_id("span")
            .accepts_brotli(accepts_brotli)
            .build()
    }

    #[test]
    fn test_should_compress_mime() {
        assert!(should_compress_mime("text/html"));
        assert!(should_compress_mime("text/html; charset=utf-8"));
        assert!(should_compress_mime("application/json"));
        assert!(should_compress_mime("application/javascript"));
        assert!(should_compress_mime("image/svg+xml"));

        // Already compressed formats
        assert!(!should_compress_mime("image/png"));
        assert!(!should_compress_mime("image/jpeg"));
        assert!(!should_compress_mime("font/woff2"));
    }

    #[test]
    fn test_compress_brotli() {
        // Create compressible data (repetitive text compresses well)
        let data = "Hello, World! ".repeat(100);
        let result = compress_brotli(data.as_bytes());

        assert!(result.is_some());
        let compressed = result.unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_middleware_skips_small_response() {
        let mw = CompressionMiddleware::new();
        let ctx = create_context(true);

        let small_body = "Hi";
        let res = Response::builder()
            .status(http::StatusCode::OK)
            .header("content-type", "text/html")
            .body(small_body)
            .build();

        let res = mw.on_response(res, &ctx);

        // Should not be compressed (too small)
        assert!(res.header("content-encoding").is_none());
    }

    #[test]
    fn test_middleware_skips_without_accept() {
        let mw = CompressionMiddleware::new();
        let ctx = create_context(false); // Does not accept brotli

        let large_body = "Hello, World! ".repeat(100);
        let res = Response::builder()
            .status(http::StatusCode::OK)
            .header("content-type", "text/html")
            .body(large_body)
            .build();

        let res = mw.on_response(res, &ctx);

        // Should not be compressed (client doesn't accept)
        assert!(res.header("content-encoding").is_none());
    }

    #[test]
    fn test_middleware_compresses_large_response() {
        let mw = CompressionMiddleware::new();
        let ctx = create_context(true);

        let large_body = "Hello, World! ".repeat(100);
        let original_len = large_body.len();

        let res = Response::builder()
            .status(http::StatusCode::OK)
            .header("content-type", "text/html")
            .body(large_body)
            .build();

        let res = mw.on_response(res, &ctx);

        // Should be compressed
        assert_eq!(res.header("content-encoding"), Some("br"));
        assert_eq!(res.header("vary"), Some("Accept-Encoding"));
        assert!(res.body_len() < original_len);
    }

    #[test]
    fn test_middleware_skips_already_compressed() {
        let mw = CompressionMiddleware::new();
        let ctx = create_context(true);

        let body = "Already compressed data".repeat(50);
        let res = Response::builder()
            .status(http::StatusCode::OK)
            .header("content-type", "text/html")
            .header("content-encoding", "gzip")
            .body(body.clone())
            .build();

        let res = mw.on_response(res, &ctx);

        // Should not re-compress
        assert_eq!(res.header("content-encoding"), Some("gzip"));
    }

    #[test]
    fn test_extracts_accept_encoding_from_request() {
        let mw = CompressionMiddleware::new();

        let mut headers = http::HeaderMap::new();
        headers.insert("accept-encoding", "br, gzip".parse().unwrap());

        let req = Request::new(
            http::Method::GET,
            "/test".parse().unwrap(),
            headers,
            bytes::Bytes::new(),
        );

        let mut ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        );

        mw.on_request(req, &mut ctx);
        assert!(ctx.accepts_brotli);
    }
}
