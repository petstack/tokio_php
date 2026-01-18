//! Brotli compression utilities.

/// Minimum size to consider compression (smaller bodies don't benefit)
pub const MIN_COMPRESSION_SIZE: usize = 256;

/// Threshold for large bodies (2 MB).
///
/// Bodies larger than this:
/// - Are NOT compressed (too CPU intensive)
/// - Static files are streamed from disk (not loaded into memory)
///
/// Bodies smaller than or equal to this:
/// - May be compressed if client supports it
/// - Static files are loaded into memory
pub const LARGE_BODY_THRESHOLD: usize = 2 * 1024 * 1024; // 2 MB

/// Brotli compression quality (0-11, higher = better compression but slower)
const BROTLI_QUALITY: u32 = 4;

/// Brotli compression window size (10-24, affects memory usage)
const BROTLI_WINDOW: u32 = 20;

/// Check if the client accepts Brotli encoding
#[inline]
pub fn accepts_brotli(accept_encoding: &str) -> bool {
    accept_encoding
        .split(',')
        .any(|enc| enc.trim().starts_with("br"))
}

/// Check if the MIME type should be compressed
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
            // Fonts (uncompressed formats - WOFF/WOFF2 are already compressed)
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
