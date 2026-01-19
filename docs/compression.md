# Brotli Compression

tokio_php automatically compresses responses using Brotli for clients that support it.

## How It Works

Compression is applied when all conditions are met:

1. Client sends `Accept-Encoding: br` header
2. Response body is >= 256 bytes and <= 3 MB
3. Content-Type is compressible (text-based)

Files larger than 3 MB are [streamed from disk](static-files.md#file-streaming) without compression to avoid blocking.

## Compression Results

Typical compression ratios (approximate values):

| Content | Original | Compressed | Ratio |
|---------|----------|------------|-------|
| HTML | ~3 KB | ~1 KB | 60-70% |
| JSON | ~5 KB | ~1.2 KB | 70-80% |
| CSS | ~10 KB | ~2.5 KB | 70-80% |
| JavaScript | ~50 KB | ~12 KB | 70-80% |

*Note: Actual ratios depend on content structure and repetition. Brotli typically achieves 15-25% better compression than gzip.*

## Testing

### With Compression

```bash
curl -H "Accept-Encoding: br" http://localhost:8080/index.php \
  --output - | brotli -d
```

### Check Headers

```bash
curl -sI -H "Accept-Encoding: br" http://localhost:8080/index.php

# Response:
# Content-Encoding: br
# Vary: Accept-Encoding
# Content-Length: 1013
```

### Without Compression

```bash
# Small response (< 256 bytes) - not compressed
curl -sI http://localhost:8080/small.php

# No Accept-Encoding header - not compressed
curl -sI http://localhost:8080/index.php
```

## Supported MIME Types

### Text Content

| MIME Type | Description |
|-----------|-------------|
| `text/html` | HTML pages |
| `text/css` | Stylesheets |
| `text/plain` | Plain text |
| `text/xml` | XML documents |
| `text/javascript` | JavaScript (legacy) |

### Application Content

| MIME Type | Description |
|-----------|-------------|
| `application/javascript` | JavaScript |
| `application/json` | JSON data |
| `application/xml` | XML data |
| `application/xhtml+xml` | XHTML |
| `application/rss+xml` | RSS feeds |
| `application/atom+xml` | Atom feeds |
| `application/manifest+json` | Web manifests |
| `application/ld+json` | JSON-LD |

### Other

| MIME Type | Description |
|-----------|-------------|
| `image/svg+xml` | SVG images |
| `font/ttf` | TrueType fonts |
| `font/otf` | OpenType fonts |
| `application/vnd.ms-fontobject` | EOT fonts |
| `application/x-font-ttf` | TrueType (legacy) |
| `application/x-font-opentype` | OpenType (legacy) |

### Not Compressed

| MIME Type | Reason |
|-----------|--------|
| `font/woff` | Already compressed |
| `font/woff2` | Uses Brotli internally |
| `image/png` | Already compressed |
| `image/jpeg` | Already compressed |
| `application/zip` | Already compressed |

## Response Headers

When compression is applied:

```http
Content-Encoding: br
Vary: Accept-Encoding
Content-Length: <compressed-size>
```

The `Vary: Accept-Encoding` header ensures caches store separate versions for different encodings.

## PHP Script Compression

Compression works with PHP output:

```php
<?php

header('Content-Type: application/json');
echo json_encode(['data' => str_repeat('x', 1000)]);
```

```bash
curl -sI -H "Accept-Encoding: br" http://localhost:8080/api.php
# Content-Encoding: br
# Content-Type: application/json
```

## Static File Compression

Static files (CSS, JS, HTML) are also compressed:

```bash
curl -sI -H "Accept-Encoding: br" http://localhost:8080/style.css
# Content-Encoding: br
# Content-Type: text/css
```

## Performance Impact

Compression adds CPU overhead but reduces bandwidth.

**Trade-offs:**
- **CPU cost**: Brotli quality 4 adds ~0.05-0.2ms per response (depends on size)
- **Bandwidth savings**: 60-80% smaller responses
- **Network latency**: Reduced transfer time, especially on slow connections

**When compression helps:**
- Slow network connections (mobile, high latency)
- Large text responses (HTML, JSON, JS)
- CDN edge caching (compress once, serve many)

**When to skip compression:**
- Very small responses (< 256 bytes)
- Already compressed content (images, video, woff2)
- CPU-constrained environments under high load

Brotli is slower than gzip but provides 15-25% better compression ratios. For most web content, the bandwidth savings outweigh the CPU cost.

## Implementation

### Detection

```rust
/// Check if the client accepts Brotli encoding
pub fn accepts_brotli(accept_encoding: &str) -> bool {
    accept_encoding
        .split(',')
        .any(|enc| enc.trim().starts_with("br"))
}
```

### MIME Check

```rust
/// Check if the MIME type should be compressed.
pub fn should_compress_mime(content_type: &str) -> bool {
    let ct = content_type.split(';').next().unwrap_or("").trim();
    matches!(
        ct,
        "text/html" | "text/css" | "text/plain" | "text/xml" | "text/javascript"
        | "application/javascript" | "application/json" | "application/xml"
        | "application/xhtml+xml" | "application/rss+xml" | "application/atom+xml"
        | "application/manifest+json" | "application/ld+json"
        | "image/svg+xml"
        | "font/ttf" | "font/otf"
        | "application/x-font-ttf" | "application/x-font-opentype"
        | "application/vnd.ms-fontobject"
    )
}
```

### Compression

```rust
/// Compress data using Brotli.
/// Returns None if compression would not reduce size.
pub fn compress_brotli(data: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(data.len() / 2);
    let mut input = std::io::Cursor::new(data);
    let params = brotli::enc::BrotliEncoderParams {
        quality: BROTLI_QUALITY as i32,  // 4
        lgwin: BROTLI_WINDOW as i32,     // 20
        ..Default::default()
    };

    match brotli::BrotliCompress(&mut input, &mut output, &params) {
        Ok(_) if output.len() < data.len() => Some(output),
        _ => None, // Compression didn't help
    }
}
```

## Configuration

Compression settings are defined in `src/server/response/compression.rs`:

| Setting | Value | Description |
|---------|-------|-------------|
| `MIN_COMPRESSION_SIZE` | 256 bytes | Don't compress small responses |
| `MAX_COMPRESSION_SIZE` | 3 MB | Compress up to this size |
| `STREAM_THRESHOLD_NON_COMPRESSIBLE` | 1 MB | Stream non-compressible files above this |
| `BROTLI_QUALITY` | 4 | Brotli quality level (0-11) |
| `BROTLI_WINDOW` | 20 | Brotli window size |

### Size Thresholds

The server uses different thresholds based on file compressibility:

**Compressible files** (text/html, application/json, etc.):
| Size | Behavior |
|------|----------|
| < 256 bytes | In-memory, no compression |
| 256 bytes - 3 MB | In-memory, Brotli compressed |
| > 3 MB | [Streamed from disk](static-files.md), no compression |

**Non-compressible files** (images, videos, archives):
| Size | Behavior |
|------|----------|
| <= 1 MB | In-memory |
| > 1 MB | [Streamed from disk](static-files.md) |

See [Static Files](static-files.md) for details on file streaming.

## Limitations

- Only Brotli is supported (no gzip fallback)
- Pre-compressed files (`.br`) are not served directly
- Compression requires full response in memory
- Files > 3 MB are [streamed](static-files.md) without compression

## Best Practices

1. **Pre-compress static assets** for production
2. **Use CDN** for caching compressed responses
3. **Set appropriate Cache-Control** headers
4. **Monitor CPU** under high load with many large responses

## See Also

- [Static Files](static-files.md) - Static file serving and streaming
- [Static Caching](static-caching.md) - Cache-Control headers for static files
- [Middleware](middleware.md) - Middleware system overview
- [Configuration](configuration.md) - Environment variables reference
- [Profiling](profiling.md) - Request timing analysis
