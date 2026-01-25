# Static File Serving

tokio_php efficiently serves static files with automatic optimization based on file size and MIME type.

## Overview

Static files are served using one of two methods:

1. **In-memory**: Small files loaded entirely into memory (fast, supports compression)
2. **Streaming**: Large files streamed from disk in chunks (constant memory usage)

The decision is automatic based on file size and whether the content type is compressible.

## Decision Flow

```
                    ┌─────────────────────┐
                    │   Static File       │
                    │   Request           │
                    └─────────┬───────────┘
                              │
                    ┌─────────▼───────────┐
                    │  Check MIME type    │
                    │  compressibility    │
                    └─────────┬───────────┘
                              │
            ┌─────────────────┴─────────────────┐
            │                                   │
   ┌────────▼────────┐                ┌────────▼────────┐
   │  Compressible   │                │ Non-compressible │
   │  (text, json)   │                │ (images, video)  │
   └────────┬────────┘                └────────┬────────┘
            │                                   │
    ┌───────┴───────┐                   ┌──────┴──────┐
    │               │                   │             │
┌───▼───┐      ┌────▼────┐         ┌────▼────┐   ┌────▼────┐
│< 256B │      │256B-3MB │         │  ≤ 1MB  │   │  > 1MB  │
└───┬───┘      └────┬────┘         └────┬────┘   └────┬────┘
    │               │                   │             │
┌───▼───┐      ┌────▼────┐         ┌────▼────┐   ┌────▼────┐
│Memory │      │ Memory  │         │ Memory  │   │Streaming│
│No comp│      │ Brotli  │         │         │   │         │
└───────┘      └─────────┘         └─────────┘   └─────────┘
                    │
            ┌───────┴───────┐
            │               │
       ┌────▼────┐     ┌────▼────┐
       │  > 3MB  │     │         │
       └────┬────┘     │         │
            │          │         │
       ┌────▼────┐     │         │
       │Streaming│     │         │
       │No comp  │     │         │
       └─────────┘     └─────────┘
```

## Size Thresholds

### Compressible Files

Text-based content types that benefit from compression.

| Size Range | Method | Compression | Memory |
|------------|--------|-------------|--------|
| < 256 bytes | In-memory | None | O(file) |
| 256 bytes - 3 MB | In-memory | Brotli | O(file) |
| > 3 MB | Streaming | None | O(64 KB) |

### Non-compressible Files

Binary formats that are already compressed or don't compress well.

| Size Range | Method | Compression | Memory |
|------------|--------|-------------|--------|
| ≤ 1 MB | In-memory | None | O(file) |
| > 1 MB | Streaming | None | O(64 KB) |

## Compressible MIME Types

These content types are considered compressible:

| Category | MIME Types |
|----------|------------|
| Text | `text/html`, `text/css`, `text/plain`, `text/xml`, `text/javascript` |
| Application | `application/javascript`, `application/json`, `application/xml` |
| Feeds | `application/rss+xml`, `application/atom+xml`, `application/xhtml+xml` |
| Structured | `application/manifest+json`, `application/ld+json` |
| Vector | `image/svg+xml` |
| Fonts | `font/ttf`, `font/otf`, `application/vnd.ms-fontobject` |

### Non-compressible Types

These are served without compression:

| Type | Reason |
|------|--------|
| `image/png`, `image/jpeg`, `image/webp` | Already compressed |
| `image/gif`, `image/avif` | Already compressed |
| `video/*`, `audio/*` | Already compressed |
| `font/woff`, `font/woff2` | Uses internal compression |
| `application/zip`, `application/gzip` | Already compressed |

## File Streaming

Large files are streamed directly from disk using async I/O.

### How It Works

```rust
// Files are read in 64 KB chunks
const FILE_CHUNK_SIZE: usize = 64 * 1024;

// Chunks are sent as HTTP body frames
impl Stream for FileFrameStream {
    fn poll_next(...) -> Poll<Option<Frame<Bytes>>> {
        // Read chunk from file
        // Convert to HTTP frame
        // Send to client
    }
}
```

### Benefits

1. **Constant memory**: Only ~64 KB buffer regardless of file size
2. **Fast TTFB**: First byte sent immediately, no waiting for full read
3. **No blocking**: Async I/O doesn't block the event loop
4. **Range support**: `Accept-Ranges: bytes` header for partial requests

### Response Headers

Streamed files include these headers:

```http
HTTP/1.1 200 OK
Content-Type: video/mp4
Content-Length: 104857600
Accept-Ranges: bytes
ETag: "6400000-696d454d"
Last-Modified: Sun, 18 Jan 2026 20:40:45 GMT
Cache-Control: public, max-age=86400
Server: tokio_php/0.1.0
```

## In-Memory Serving

Small files are loaded entirely into memory for fast serving.

### Benefits

1. **Compression**: Brotli compression for compressible types
2. **Fast response**: No disk I/O during request handling
3. **Caching**: Works with HTTP caching headers

### Response Headers

In-memory responses with compression:

```http
HTTP/1.1 200 OK
Content-Type: text/css
Content-Encoding: br
Vary: Accept-Encoding
ETag: "1a2b-65a51a2d"
Last-Modified: Sun, 18 Jan 2026 20:40:45 GMT
Cache-Control: public, max-age=86400
Server: tokio_php/0.1.0
```

## Configuration

Settings in `src/server/response/compression.rs`:

| Constant | Value | Description |
|----------|-------|-------------|
| `MIN_COMPRESSION_SIZE` | 256 bytes | Minimum size for compression |
| `MAX_COMPRESSION_SIZE` | 3 MB | Maximum size for compression |
| `STREAM_THRESHOLD_NON_COMPRESSIBLE` | 1 MB | Stream threshold for binary files |

Settings in `src/server/response/streaming.rs`:

| Constant | Value | Description |
|----------|-------|-------------|
| `FILE_CHUNK_SIZE` | 64 KB | Chunk size for streaming |

## HTTP Caching

All static files support HTTP caching via the `STATIC_CACHE_TTL` environment variable:

```bash
# 1 week caching (recommended for production)
STATIC_CACHE_TTL=1w docker compose up -d

# Disable caching (development)
STATIC_CACHE_TTL=off docker compose up -d
```

### Conditional Requests

Both streaming and in-memory responses support:

- `If-None-Match` (ETag validation)
- `If-Modified-Since` (date validation)

Returns `304 Not Modified` when content hasn't changed.

See [Static Caching](static-caching.md) for full documentation.

## Testing

### Check File Size Behavior

```bash
# Small file (in-memory, compressed if compressible)
curl -sI -H "Accept-Encoding: br" http://localhost:8080/small.css
# Content-Encoding: br

# Large compressible file (> 3 MB, streamed, no compression)
curl -sI -H "Accept-Encoding: br" http://localhost:8080/large.json
# No Content-Encoding header
# Accept-Ranges: bytes

# Large binary file (> 1 MB, streamed)
curl -sI http://localhost:8080/video.mp4
# Accept-Ranges: bytes
# Content-Length: 104857600
```

### Verify Streaming

```bash
# Watch chunks arrive in real-time
curl -N http://localhost:8080/large-file.bin > /dev/null

# Check memory usage during large file transfer
docker stats tokio_php
```

## Performance

### Benchmark Results

| File Type | Size | Method | Throughput |
|-----------|------|--------|------------|
| HTML | 10 KB | In-memory + Brotli | ~25,000 RPS |
| JSON | 100 KB | In-memory + Brotli | ~15,000 RPS |
| Image | 500 KB | In-memory | ~10,000 RPS |
| Video | 100 MB | Streaming | ~500 concurrent |

*Results vary based on hardware and network conditions.*

### Memory Usage

| File Size | In-Memory | Streaming |
|-----------|-----------|-----------|
| 100 KB | ~100 KB | ~64 KB |
| 1 MB | ~1 MB | ~64 KB |
| 10 MB | ~10 MB | ~64 KB |
| 100 MB | N/A (streamed) | ~64 KB |

## Implementation

### File Serving Logic

Located in `src/server/response/static_file.rs`:

```rust
pub async fn serve_static_file(
    file_path: &Path,
    use_brotli: bool,
    cache_ttl: &StaticCacheTtl,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
) -> Response<StaticFileBody> {
    // 1. Get file metadata
    let metadata = tokio::fs::metadata(file_path).await?;
    let size = metadata.len();
    let mime = mime_guess::from_path(file_path);

    // 2. Check if compressible
    let is_compressible = should_compress_mime(&mime);

    // 3. Decide: stream or in-memory
    if should_stream_file(size, is_compressible) {
        // Stream large files
        file_streaming_response(...)
    } else {
        // Load into memory, optionally compress
        let contents = tokio::fs::read(file_path).await?;
        if should_compress {
            compress_brotli(&contents)
        }
    }
}
```

### Streaming Decision

Located in `src/server/response/streaming.rs`:

```rust
pub fn should_stream_file(size: u64, is_compressible: bool) -> bool {
    if is_compressible {
        // Compressible: stream if > 3 MB
        size > MAX_COMPRESSION_SIZE as u64
    } else {
        // Non-compressible: stream if > 1 MB
        size > STREAM_THRESHOLD_NON_COMPRESSIBLE as u64
    }
}
```

## INDEX_FILE with HTML (SPA Mode)

When `INDEX_FILE` points to an HTML file, the server operates in SPA (Single Page Application) mode:

```bash
# SPA mode - all non-existent paths serve index.html
INDEX_FILE=index.html docker compose up -d
```

### Routing Behavior

| Request | File Exists | Result |
|---------|-------------|--------|
| `/` | — | Serve index.html |
| `/about` | No | Serve index.html |
| `/users/123` | No | Serve index.html |
| `/index.html` | — | **404** (direct access blocked) |
| `/style.css` | Yes | Serve style.css |
| `/api.php` | Yes | Execute PHP |
| `/api.php` | No | Serve index.html |

### Key Differences from Framework Mode

| Aspect | Framework (`index.php`) | SPA (`index.html`) |
|--------|-------------------------|---------------------|
| PHP files | ALL blocked with 404 | Execute if exists |
| Missing files | Fallback to index.php | Fallback to index.html |
| Index access | `/index.php` → 404 | `/index.html` → 404 |

### SPA Optimization

HTML index files benefit from static file optimizations:

| Feature | Applies to HTML |
|---------|-----------------|
| In-memory serving | ✓ (if < 3 MB) |
| Brotli compression | ✓ |
| ETag / Last-Modified | ✓ |
| Cache-Control headers | ✓ |
| 304 Not Modified | ✓ |

### Hybrid PHP API + SPA

SPA mode allows PHP endpoints alongside client-side routing:

```
/var/www/html/
├── api.php            # PHP API (executed)
├── webhook.php        # PHP webhook (executed)
├── index.html         # SPA entry point
├── app.js
└── style.css
```

```bash
INDEX_FILE=index.html docker compose up -d
```

- `/api.php` → Execute PHP (file exists)
- `/webhook.php` → Execute PHP (file exists)
- `/users/123` → Serve index.html (client-side route)

See [Single Entry Point](single-entry-point.md#spa-mode-index_fileindexhtml) for full SPA documentation.

## Limitations

- No range request support for streaming (Accept-Ranges header is informational)
- No pre-compressed file serving (`.br` files not served directly)
- Streaming files are not compressed (too CPU-intensive)
- No directory listing

## Best Practices

1. **Use CDN** for large static files in production
2. **Enable caching** with appropriate TTL (`STATIC_CACHE_TTL=1w`)
3. **Compress assets** at build time for best compression ratios
4. **Use versioned filenames** for long cache TTL (`app.abc123.js`)
5. **Serve large files** from object storage (S3, GCS) when possible

## See Also

- [Compression](compression.md) - Brotli compression details
- [Static Caching](static-caching.md) - HTTP caching headers
- [SSE Streaming](sse-streaming.md) - Server-Sent Events streaming
- [Single Entry Point](single-entry-point.md) - Static file handling with `INDEX_FILE`
- [Architecture](architecture.md) - System design overview
