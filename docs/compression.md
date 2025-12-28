# Brotli Compression

tokio_php automatically compresses responses using Brotli for clients that support it.

## How It Works

Compression is applied when all conditions are met:

1. Client sends `Accept-Encoding: br` header
2. Response body is >= 256 bytes
3. Content-Type is compressible (text-based)

## Compression Results

Typical compression ratios:

| Content | Original | Compressed | Ratio |
|---------|----------|------------|-------|
| HTML | 2,822 bytes | 1,013 bytes | 64% |
| JSON | 5,000 bytes | 1,200 bytes | 76% |
| CSS | 10,000 bytes | 2,500 bytes | 75% |
| JavaScript | 50,000 bytes | 12,000 bytes | 76% |

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
?>
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

Compression adds CPU overhead but reduces bandwidth:

| Metric | Without Compression | With Compression |
|--------|---------------------|------------------|
| Response size | 2,822 bytes | 1,013 bytes |
| Transfer time | ~0.5ms | ~0.2ms |
| CPU time | 0ms | ~0.1ms |
| **Net benefit** | - | Faster for clients |

Brotli is slower than gzip but provides better compression ratios. For most web content, the bandwidth savings outweigh the CPU cost.

## Implementation

### Detection

```rust
fn accepts_brotli(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("br"))
        .unwrap_or(false)
}
```

### MIME Check

```rust
fn should_compress_mime(content_type: &str) -> bool {
    let compressible = [
        "text/html", "text/css", "text/plain", "text/xml",
        "application/javascript", "application/json",
        "image/svg+xml", "font/ttf", "font/otf",
        // ...
    ];

    compressible.iter().any(|m| content_type.starts_with(m))
}
```

### Compression

```rust
fn compress_brotli(data: &[u8]) -> Vec<u8> {
    let mut encoder = brotli::CompressorWriter::new(
        Vec::new(),
        4096,  // buffer size
        4,     // quality (0-11, 4 is good balance)
        22,    // window size
    );
    encoder.write_all(data).unwrap();
    encoder.into_inner()
}
```

## Configuration

Currently, compression settings are hardcoded:

| Setting | Value | Description |
|---------|-------|-------------|
| Min size | 256 bytes | Don't compress small responses |
| Quality | 4 | Brotli quality level (0-11) |
| Window | 22 | Brotli window size |

Future versions may expose these as environment variables.

## Limitations

- Only Brotli is supported (no gzip fallback)
- Pre-compressed files are not served directly
- Compression is not streaming (full response buffered)

## Best Practices

1. **Pre-compress static assets** for production
2. **Use CDN** for caching compressed responses
3. **Set appropriate Cache-Control** headers
4. **Monitor CPU** under high load with many large responses
