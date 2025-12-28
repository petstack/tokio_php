# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

tokio_php is an async web server written in Rust that executes PHP scripts via php-embed SAPI. It uses Tokio for async I/O and Hyper for HTTP handling. Supports HTTP/1.1, HTTP/2, and HTTPS with TLS 1.3.

## Build Commands

```bash
# Build and run with Docker (recommended)
docker compose build
docker compose up -d

# Rebuild without cache
docker compose build --no-cache

# View logs
docker compose logs -f

# Stop and remove volumes
docker compose down -v

# Run with environment variables
PHP_WORKERS=4 docker compose up -d      # Set worker count
USE_STUB=1 docker compose up -d          # Stub mode (no PHP, for benchmarks)
USE_EXT=1 docker compose up -d           # ExtExecutor with tokio_sapi extension
PROFILE=1 docker compose up -d           # Enable profiling

# Run with TLS/HTTPS (ports 8443, 8444)
docker compose --profile tls up -d

# Benchmark
wrk -t4 -c100 -d10s http://localhost:8080/index.php
```

## Architecture

### Core Components

- `src/main.rs` - Entry point, runtime initialization, executor selection, TLS config
- `src/server.rs` - Hyper-based HTTP/1.1 + HTTP/2 server, TLS support, request parsing
- `src/executor/` - Script execution backends (trait-based, pluggable)
- `src/types.rs` - ScriptRequest/ScriptResponse data structures
- `src/profiler.rs` - Request timing profiler with TLS metrics

### Protocol Support

- **HTTP/1.1** - Default on port 8080
- **HTTP/2 h2c** - Cleartext HTTP/2 via `--http2-prior-knowledge`
- **HTTPS + HTTP/2** - TLS 1.3 with ALPN on port 8443 (requires `--profile tls`)

Auto-detection via `hyper_util::server::conn::auto::Builder`.

### Executor System

The `ScriptExecutor` trait (`src/executor/mod.rs`) defines the interface for script execution:

- `ExtExecutor` (`ext.rs`) - **Recommended for production.** Uses `php_execute_script()` + FFI superglobals
- `PhpExecutor` (`php.rs`) - Legacy executor using `zend_eval_string()` for superglobals
- `StubExecutor` (`stub.rs`) - Returns empty responses for benchmarking

Selection order in main.rs:
1. `USE_STUB=1` → StubExecutor
2. `USE_EXT=1` → ExtExecutor (with tokio_sapi PHP extension) **← recommended**
3. Default → PhpExecutor

### Executor Performance Comparison

ExtExecutor is **2x faster** than PhpExecutor due to different script execution methods:

| Executor | Method | RPS (index.php) | RPS (bench.php) |
|----------|--------|-----------------|-----------------|
| **ExtExecutor** | `php_execute_script()` | **33,677** | **37,911** |
| PhpExecutor | `zend_eval_string()` | 16,208 | 19,555 |

**Why ExtExecutor is faster:**

1. **`php_execute_script()`** - Native PHP file execution, fully optimized for OPcache/JIT
2. **FFI superglobals** - Direct C calls to set `$_GET`, `$_POST`, `$_SERVER`, etc.
3. **No parsing overhead** - PhpExecutor re-parses wrapper code on every request

**Production recommendation:**
```bash
USE_EXT=1 docker compose up -d
```

### Performance vs PHP-FPM

tokio_php is **2.5x faster** than nginx + PHP-FPM:

| Server | RPS (bench.php) | RPS (index.php) | Latency |
|--------|-----------------|-----------------|---------|
| **tokio_php** | **35,350** | **32,913** | 2.8ms |
| nginx + PHP-FPM | 13,890 | 12,471 | 7.2ms |

*Benchmark: 14 workers each, OPcache+JIT enabled, wrk -t4 -c100 -d5s*

**Why tokio_php is faster:**
1. No network hop (nginx → FastCGI socket → FPM)
2. Threads vs processes (no context switch overhead)
3. No FastCGI protocol encode/decode (~1ms saved)
4. Direct OPcache access via TSRM
5. Single binary (no reverse proxy)

See [docs/architecture.md](docs/architecture.md#comparison-with-php-fpm) for detailed comparison.

### tokio_sapi PHP Extension

Located in `ext/` directory. Provides:
- PHP functions: `tokio_request_id()`, `tokio_worker_id()`, `tokio_server_info()`, `tokio_async_call()`
- C API for future FFI superglobals optimization
- Built as both shared library (.so) and static library (.a)

### PHP Worker Pool

- Multi-threaded worker pool (threads = `PHP_WORKERS` or CPU count)
- Channel-based work distribution (`mpsc::channel` → workers)
- Each worker: `php_request_startup()` → execute → `php_request_shutdown()`
- Output captured via memfd + stdout redirection
- Superglobals injected via `zend_eval_string` before script execution

### TLS Implementation

- Uses `tokio-rustls` with `rustls` for TLS 1.3
- Certificates loaded from PEM files via `TLS_CERT` and `TLS_KEY` env vars
- ALPN protocols: `h2`, `http/1.1` for automatic HTTP/2 negotiation
- Self-signed dev certificates in `certs/` directory

### Key Technical Details

- SAPI name set to "cli-server" before `php_embed_init` for OPcache/JIT compatibility
- PHP 8.4 ZTS (Thread Safe) build required
- Single-threaded Tokio runtime (PHP workers handle blocking work)
- OPcache settings in Dockerfile: `opcache.jit=tracing`, `opcache.validate_timestamps=0`
- Preloading enabled via `opcache.preload` for framework optimization

### OPcache Preloading

Preloading runs `preload.php` at server startup to cache framework classes:

```php
// www/preload.php
<?php
require __DIR__ . '/vendor/autoload.php';
opcache_compile_file(__DIR__ . '/src/Kernel.php');
```

Benefits:
- Eliminates compilation time for preloaded files
- Classes are "linked" at startup
- +30-60% performance for frameworks

Check status: `curl http://localhost:8080/opcache_status.php`

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | Server bind address |
| `PHP_WORKERS` | `0` | Worker count (0 = auto-detect CPU cores) |
| `QUEUE_CAPACITY` | `0` | Max pending requests (0 = workers × 100) |
| `DOCUMENT_ROOT` | `/var/www/html` | Web root directory |
| `INDEX_FILE` | _(empty)_ | Single entry point mode (e.g., `index.php`) |
| `INTERNAL_ADDR` | _(empty)_ | Internal server for /health and /metrics |
| `ERROR_PAGES_DIR` | _(empty)_ | Directory with custom HTML error pages (e.g., 404.html) |
| `DRAIN_TIMEOUT_SECS` | `30` | Graceful shutdown drain timeout (seconds) |
| `STATIC_CACHE_TTL` | `1d` | Static file cache duration (1d, 1w, 1m, 1y, off) |
| `ACCESS_LOG` | `0` | Enable access logs (target: `access`) |
| `RATE_LIMIT` | `0` | Max requests per IP per window (0 = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window in seconds |
| `USE_STUB` | `0` | Stub mode - disable PHP, return empty responses |
| `USE_EXT` | `0` | **Recommended.** Use ExtExecutor with tokio_sapi extension (2x faster) |
| `PROFILE` | `0` | Enable profiling (requires `X-Profile: 1` header) |
| `TLS_CERT` | _(empty)_ | Path to TLS certificate (PEM) |
| `TLS_KEY` | _(empty)_ | Path to TLS private key (PEM) |
| `RUST_LOG` | `tokio_php=info` | Log level (trace, debug, info, warn, error) |

### Auto-calculated Defaults

- `PHP_WORKERS=0` → uses `num_cpus::get()` (all available CPU cores)
- `QUEUE_CAPACITY=0` → uses `workers × 100` (e.g., 8 workers = 800 queue capacity)

### Configuration Examples

```bash
# Minimal (all defaults, uses PhpExecutor)
docker compose up -d

# Production (recommended - 2x faster with ExtExecutor)
USE_EXT=1 PHP_WORKERS=8 INTERNAL_ADDR=0.0.0.0:9090 docker compose up -d

# Benchmark mode (no PHP execution)
USE_STUB=1 docker compose up -d

# Laravel/Symfony single entry point
INDEX_FILE=index.php DOCUMENT_ROOT=/var/www/html/public docker compose up -d

# With TLS/HTTPS
TLS_CERT=/certs/cert.pem TLS_KEY=/certs/key.pem docker compose up -d

# Custom error pages
ERROR_PAGES_DIR=/var/www/html/errors docker compose up -d

# Enable access logs
ACCESS_LOG=1 docker compose up -d

# Debug logging
RUST_LOG=tokio_php=debug docker compose up -d
```

## Profiling

With `PROFILE=1`, requests with `X-Profile: 1` header return timing data:

```bash
# HTTP profiling
curl -sI -H "X-Profile: 1" http://localhost:8080/index.php | grep X-Profile

# HTTPS profiling (includes TLS metrics)
curl -sIk -H "X-Profile: 1" https://localhost:8443/index.php | grep X-Profile
```

### Profile Headers

| Header | Description |
|--------|-------------|
| `X-Profile-Total-Us` | Total request time (microseconds) |
| `X-Profile-HTTP-Version` | HTTP/1.0, HTTP/1.1, HTTP/2.0 |
| `X-Profile-TLS-Handshake-Us` | TLS handshake time (HTTPS only) |
| `X-Profile-TLS-Protocol` | TLS version (TLSv1_2, TLSv1_3) |
| `X-Profile-TLS-ALPN` | ALPN negotiated protocol (h2, http/1.1) |
| `X-Profile-Parse-Us` | Request parsing time |
| `X-Profile-Queue-Us` | Worker queue wait time |
| `X-Profile-PHP-Startup-Us` | php_request_startup() time |
| `X-Profile-Script-Us` | PHP script execution time |
| `X-Profile-Output-Us` | Output capture time |
| `X-Profile-PHP-Shutdown-Us` | php_request_shutdown() time |

## Logging

All logs use unified JSON format. Enable access logs with `ACCESS_LOG=1`.

### Log Format

```json
{"ts":"2025-01-15T10:30:00.123Z","level":"info","type":"app","msg":"Server listening on http://0.0.0.0:8080","ctx":{"service":"tokio_php"},"data":{}}
```

| Field | Description |
|-------|-------------|
| `ts` | ISO 8601 timestamp with milliseconds, UTC |
| `level` | `debug`, `info`, `warn`, `error` |
| `type` | `app`, `access`, `error` — log type |
| `msg` | Short human-readable message |
| `ctx` | Context: service, request_id, etc. |
| `data` | Type-specific data |

### Access Log Example

```json
{"ts":"2025-01-15T10:30:00.456Z","level":"info","type":"access","msg":"GET /api/users 200","ctx":{"service":"tokio_php","request_id":"65bdbab40000"},"data":{"method":"GET","path":"/api/users","status":200,"bytes":1234,"duration_ms":5.25,"ip":"10.0.0.1","ua":"curl/8.0"}}
```

### Access Log Data Fields

| Field | Type | Description |
|-------|------|-------------|
| `method` | string | HTTP method |
| `path` | string | Request path |
| `query` | string? | Query string |
| `http` | string | HTTP version |
| `status` | number | Response status code |
| `bytes` | number | Response body size |
| `duration_ms` | number | Request duration (ms) |
| `ip` | string | Client IP |
| `ua` | string? | User-Agent |
| `referer` | string? | Referer |
| `xff` | string? | X-Forwarded-For |
| `tls` | string? | TLS protocol |

### Filtering

```bash
# All logs
RUST_LOG=tokio_php=info

# Debug mode
RUST_LOG=tokio_php=debug

# Filter by type with jq
docker compose logs | jq -c 'select(.type == "access")'
docker compose logs | jq -c 'select(.type == "app")'
```

Compatible with: Fluentd, Fluent Bit, Logstash, CloudWatch, Loki, Vector.

## Request ID

Every request gets a unique ID for tracing and correlation across services.

### Generation

Request IDs are 12-character hex strings: 8 chars timestamp + 4 chars counter.

```
65bdbab40000
^^^^^^^^----
    |     |
    |     +-- Counter (4 hex chars)
    +-------- Timestamp ms (8 hex chars)
```

### Propagation

- Incoming `X-Request-ID` header → used as-is
- No header → server generates new ID

### Response Header

Every response includes `X-Request-ID`:

```bash
curl -sI http://localhost:8080/index.php | grep x-request-id
x-request-id: 65bdbab40000

# Propagate existing ID
curl -sI -H "X-Request-ID: my-trace-123" http://localhost:8080/ | grep x-request-id
x-request-id: my-trace-123
```

### Log Correlation

Request ID appears in `ctx.request_id` in access logs:

```json
{"ts":"...","level":"info","type":"access","msg":"GET / 200","ctx":{"service":"tokio_php","request_id":"65bdbab40000"},...}
```

Use for distributed tracing across microservices.

## Rate Limiting

Per-IP rate limiting with fixed window algorithm. See [docs/rate-limiting.md](docs/rate-limiting.md) for full documentation.

```bash
# 100 requests per minute per IP
RATE_LIMIT=100 RATE_WINDOW=60 docker compose up -d
```

Response when limited:
```
HTTP/1.1 429 Too Many Requests
Retry-After: 45
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 45
```

### Rate Limit vs Queue Capacity

```
Request → Rate Limit (per-IP) → Queue (global) → Worker
              │ 429                  │ 503
              ▼                      ▼
         Too Many Requests    Service Unavailable
```

| Mechanism | Scope | Response | Purpose |
|-----------|-------|----------|---------|
| `RATE_LIMIT` | Per-IP | 429 | Fairness, abuse prevention |
| `QUEUE_CAPACITY` | Global | 503 | Server overload protection |

## Security

### Non-root Execution

The server runs as `www-data` user (UID 82) for security:

```dockerfile
USER www-data
CMD ["tokio_php"]
```

- No root privileges at runtime
- Standard UID 82 (compatible with nginx/apache)
- OPcache preload runs as `www-data`
- All files in `/var/www/html` owned by `www-data`

### Verify

```bash
docker compose exec tokio_php whoami
# www-data

docker compose exec tokio_php ps aux
# PID   USER     COMMAND
#   1   www-data tokio_php
```

## Docker Services

| Service | Port | Description |
|---------|------|-------------|
| `tokio_php_embed` | 8080 | HTTP, PhpExecutor |
| `tokio_php_sapi` | 8081 | HTTP, PhpSapiExecutor |
| `tokio_php_embed_tls` | 8443 | HTTPS, PhpExecutor (profile: tls) |
| `tokio_php_sapi_tls` | 8444 | HTTPS, PhpSapiExecutor (profile: tls) |

## Testing Protocols

```bash
# HTTP/1.1
curl http://localhost:8080/index.php

# HTTP/2 cleartext (h2c)
curl --http2-prior-knowledge http://localhost:8080/index.php

# HTTPS with HTTP/2 (auto-negotiated via ALPN)
curl -k https://localhost:8443/index.php

# Check protocol version in PHP
curl -k https://localhost:8443/index.php  # $_SERVER['SERVER_PROTOCOL'] = HTTP/2.0
```

## Superglobals Support

Full superglobals: `$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`, `$_FILES`, `$_REQUEST`

## Compression

Brotli compression is automatically applied when:
- Client sends `Accept-Encoding: br` header
- Response body >= 256 bytes and <= 3 MB
- Content-Type is compressible (text/html, text/css, application/json, etc.)

Size limits (defined in `src/server/response/compression.rs`):
- `MIN_COMPRESSION_SIZE` = 256 bytes (smaller files don't benefit)
- `MAX_COMPRESSION_SIZE` = 3 MB (larger files take too long)

Compressed responses include:
- `Content-Encoding: br` header
- `Vary: Accept-Encoding` header for proper caching

Supported MIME types:
- `text/html`, `text/css`, `text/plain`, `text/xml`, `text/javascript`
- `application/javascript`, `application/json`, `application/xml`
- `application/xhtml+xml`, `application/rss+xml`, `application/atom+xml`
- `application/manifest+json`, `application/ld+json`, `image/svg+xml`
- `font/ttf`, `font/otf`, `application/vnd.ms-fontobject` (EOT)

Note: WOFF/WOFF2 fonts are not compressed (already use internal compression).

## Single Entry Point Mode

For Laravel/Symfony-style routing, set `INDEX_FILE` to route all requests through a single script:

```bash
INDEX_FILE=index.php docker compose up -d
```

Behavior:
- All requests route to the specified file (e.g., `/api/users` -> `index.php`)
- Direct access to the index file returns 404 (e.g., `/index.php` -> 404)
- File existence validated at startup (server exits if missing)
- Skips per-request file existence checks (performance optimization)

## Internal Server

Optional internal HTTP server for health checks and metrics. Enable by setting `INTERNAL_ADDR`:

```bash
INTERNAL_ADDR=0.0.0.0:9090 docker compose up -d
```

### Endpoints

| Endpoint | Description |
|----------|-------------|
| `/health` | Health check with timestamp and active connections (JSON) |
| `/metrics` | Prometheus-compatible metrics |

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_uptime_seconds` | gauge | Server uptime in seconds |
| `tokio_php_requests_per_second` | gauge | Lifetime average RPS |
| `tokio_php_response_time_avg_seconds` | gauge | Average response time |
| `tokio_php_active_connections` | gauge | Current active connections |
| `tokio_php_pending_requests` | gauge | Requests waiting in queue |
| `tokio_php_dropped_requests` | counter | Requests dropped (queue full) |
| `tokio_php_requests_total{method}` | counter | Requests by HTTP method |
| `tokio_php_responses_total{status}` | counter | Responses by status class |
| `node_load1` | gauge | 1-minute load average |
| `node_load5` | gauge | 5-minute load average |
| `node_load15` | gauge | 15-minute load average |
| `node_memory_MemTotal_bytes` | gauge | Total memory in bytes |
| `node_memory_MemAvailable_bytes` | gauge | Available memory in bytes |
| `node_memory_MemUsed_bytes` | gauge | Used memory in bytes |
| `tokio_php_memory_usage_percent` | gauge | Memory usage percentage |

### Example responses

```bash
# Health check
curl http://localhost:9090/health
{"status":"ok","timestamp":1703361234,"active_connections":5,"total_requests":1000}

# Metrics
curl http://localhost:9090/metrics
# HELP tokio_php_uptime_seconds Server uptime in seconds
tokio_php_uptime_seconds 3600.000
# HELP node_load1 1-minute load average
node_load1 1.50
# HELP tokio_php_memory_usage_percent Memory usage percentage
tokio_php_memory_usage_percent 45.32
```

## Custom Error Pages

Serve custom HTML error pages for 4xx/5xx responses. Enable by setting `ERROR_PAGES_DIR`:

```bash
ERROR_PAGES_DIR=/var/www/html/errors docker compose up -d
```

### File Naming

Files must be named `{status_code}.html`:
- `404.html` - Not Found
- `500.html` - Internal Server Error
- `503.html` - Service Unavailable

### Behavior

- Files cached in memory at startup (high performance)
- Only served when client sends `Accept: text/html` header
- Only applied to 4xx/5xx responses with empty body
- Missing files fall back to default text response
- Files served as-is (not processed through PHP)

Example error pages are provided in `www/errors/`.

## Graceful Shutdown

tokio_php supports graceful shutdown with connection draining for zero-downtime deployments.

### Behavior

1. Server receives SIGTERM/SIGINT (Ctrl+C)
2. Stops accepting new connections
3. Waits for in-flight requests to complete (up to `DRAIN_TIMEOUT_SECS`)
4. Shuts down cleanly

### Configuration

```bash
# Default: 30 seconds
DRAIN_TIMEOUT_SECS=30 docker compose up -d

# Kubernetes: match terminationGracePeriodSeconds
DRAIN_TIMEOUT_SECS=25 docker compose up -d  # 5s buffer for preStop hook
```

### Kubernetes Integration

```yaml
spec:
  terminationGracePeriodSeconds: 30
  containers:
    - name: app
      lifecycle:
        preStop:
          exec:
            command: ["sleep", "5"]  # Allow LB to remove pod
```

Timeline:
- 0s: SIGTERM sent
- 0-5s: preStop hook (LB removes pod from rotation)
- 5-30s: App drains connections
- 30s: SIGKILL if still running

## Limitations

- No `$_SESSION` support (requires session handler implementation)
- HTTP/3 (QUIC) not yet implemented (h3 crate is experimental)
