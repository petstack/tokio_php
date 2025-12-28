# Static File Caching

tokio_php includes built-in HTTP caching support for static files (CSS, JS, images, fonts, etc.).

## Features

- **Cache-Control**: Browser caching with configurable max-age
- **Expires**: HTTP/1.0 compatible expiration header
- **ETag**: Weak entity tag for conditional requests
- **Last-Modified**: File modification timestamp
- **If-None-Match**: Conditional request support (returns 304)
- **If-Modified-Since**: Conditional request support (returns 304)

## Configuration

Set the `STATIC_CACHE_TTL` environment variable to control cache duration:

```bash
# Default: 1 day
STATIC_CACHE_TTL=1d docker compose up -d

# 1 week (recommended for production)
STATIC_CACHE_TTL=1w docker compose up -d

# 1 month
STATIC_CACHE_TTL=1m docker compose up -d

# 1 year (for versioned assets)
STATIC_CACHE_TTL=1y docker compose up -d

# Disable caching
STATIC_CACHE_TTL=off docker compose up -d
```

### Duration Format

| Format | Duration | Seconds |
|--------|----------|---------|
| `1s` | 1 second | 1 |
| `1h` | 1 hour | 3,600 |
| `1d` | 1 day | 86,400 |
| `1w` | 1 week | 604,800 |
| `1m` | ~1 month | 2,592,000 |
| `1y` | ~1 year | 31,536,000 |
| `off` | disabled | - |

Numbers can be any positive integer: `7d`, `2w`, `6m`, etc.

## Response Headers

When caching is enabled, static file responses include:

```http
HTTP/1.1 200 OK
Content-Type: text/css
Cache-Control: public, max-age=86400
Expires: Mon, 30 Dec 2024 12:00:00 GMT
ETag: "1a2b-65a51a2d"
Last-Modified: Sun, 29 Dec 2024 12:00:00 GMT
```

### Header Descriptions

| Header | Description |
|--------|-------------|
| `Cache-Control` | `public, max-age=N` where N is TTL in seconds |
| `Expires` | Absolute expiration date (HTTP-date format) |
| `ETag` | `"size-mtime"` in hex format |
| `Last-Modified` | File modification time (HTTP-date format) |

## Best Practices

### Development
```bash
STATIC_CACHE_TTL=off docker compose up -d
```
Disable caching during development to see changes immediately.

### Production
```bash
STATIC_CACHE_TTL=1w docker compose up -d
```
Use 1 week for most static assets.

### Versioned Assets
For assets with version hashes (e.g., `app.abc123.js`):
```bash
STATIC_CACHE_TTL=1y docker compose up -d
```
Long cache with immutable content.

## Conditional Requests (304 Not Modified)

When caching is enabled, tokio_php supports conditional requests to avoid resending unchanged files:

### If-None-Match (ETag validation)

```bash
# First request - returns 200 with ETag
curl -I http://localhost:8080/style.css
# ETag: "14-6951a459"

# Second request with ETag - returns 304 if unchanged
curl -I -H 'If-None-Match: "14-6951a459"' http://localhost:8080/style.css
# HTTP/1.1 304 Not Modified
```

### If-Modified-Since (Date validation)

```bash
# Request with date - returns 304 if file hasn't changed since
curl -I -H 'If-Modified-Since: Sun, 28 Dec 2025 21:42:49 GMT' http://localhost:8080/style.css
# HTTP/1.1 304 Not Modified
```

### Behavior

| Condition | Result |
|-----------|--------|
| ETag matches (`If-None-Match`) | 304 Not Modified |
| File not modified since date (`If-Modified-Since`) | 304 Not Modified |
| ETag doesn't match | 200 OK with full body |
| File modified after date | 200 OK with full body |

`If-None-Match` takes precedence over `If-Modified-Since` per RFC 7232.

## PHP Files

Caching headers are **only applied to static files**. PHP responses are not cached by this mechanism - use PHP's `header()` function to set caching for dynamic content.

## Compression

Static file caching works together with Brotli compression. Compressed responses include:
- `Content-Encoding: br`
- `Vary: Accept-Encoding`

The `Vary` header ensures proper cache key separation for compressed vs uncompressed versions.

### Compression Size Limits

| Constant | Value | Description |
|----------|-------|-------------|
| `MIN_COMPRESSION_SIZE` | 256 bytes | Files smaller than this are not compressed |
| `MAX_COMPRESSION_SIZE` | 3 MB | Files larger than this are not compressed |

Files outside this range are served without compression to avoid overhead (small files) or blocking (large files).
