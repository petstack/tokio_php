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

# Run with TLS/HTTPS (port 8443)
docker compose --profile tls up -d

# Benchmark
wrk -t4 -c100 -d10s http://localhost:8080/index.php
```

## Architecture

### Core Components

- `src/main.rs` - Entry point, runtime initialization, executor selection, TLS config
- `src/server/` - Hyper-based HTTP/1.1 + HTTP/2 server, TLS support, request parsing
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

Performance depends on script complexity:

| Script | PhpExecutor | ExtExecutor | Winner |
|--------|-------------|-------------|--------|
| bench.php (minimal) | **22,821** RPS | 20,420 RPS | PhpExecutor +12% |
| index.php (superglobals) | 17,119 RPS | **25,307** RPS | **ExtExecutor +48%** |

*Benchmark: 14 workers, OPcache+JIT, wrk -t4 -c100 -d10s, Apple M3 Pro*

**When to use which:**

| Use Case | Recommendation |
|----------|----------------|
| Real apps (Laravel, Symfony, WordPress) | **USE_EXT=1** — 48% faster with superglobals |
| Minimal scripts (health checks, APIs) | USE_EXT=0 — less extension overhead |
| Production | **USE_EXT=1** — most apps use superglobals |

**Why ExtExecutor is faster for real apps:**

1. **FFI batch API** — sets all `$_SERVER` vars in one C call vs building PHP string
2. **`php_execute_script()`** — native PHP execution, fully OPcache/JIT optimized
3. **No string parsing** — PhpExecutor builds and parses PHP code every request

**Why PhpExecutor is faster for minimal scripts:**

1. **No extension overhead** — tokio_sapi adds ~100µs per request init/shutdown
2. **Simple eval** — for tiny scripts, `zend_eval_string()` is very fast

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
- PHP functions: `tokio_request_id()`, `tokio_worker_id()`, `tokio_server_info()`, `tokio_request_heartbeat()`, `tokio_finish_request()`
- Build version tracking: `$_SERVER['TOKIO_SERVER_BUILD_VERSION']` and `tokio_server_info()['build']`
- C API for FFI superglobals optimization (no eval overhead)
- Built as both shared library (.so) and static library (.a)

#### tokio_request_heartbeat(int $time = 10): bool

Extends the request timeout deadline for long-running scripts. Useful for preventing 504 Gateway Timeout while processing large datasets or slow external APIs.

```php
<?php
// Long-running script that needs more time
foreach ($large_dataset as $item) {
    process_item($item);

    // Extend deadline by 30 seconds and PHP's time limit
    set_time_limit(30);
    tokio_request_heartbeat(30);
}
```

Returns `false` if:
- No timeout configured (`REQUEST_TIMEOUT=off`)
- `$time <= 0`
- `$time > REQUEST_TIMEOUT` limit (e.g., if `REQUEST_TIMEOUT=5m`, max is 300)

**Note**: Also call `set_time_limit()` to extend PHP's internal timeout.

#### tokio_finish_request(): bool

Sends the response to the client immediately and continues executing the script in the background. Analog of `fastcgi_finish_request()` in PHP-FPM.

```php
<?php
echo "Response sent to user\n";
header("X-Status: accepted");

tokio_finish_request();  // Client gets response NOW

// Background work (client doesn't wait):
send_email($user);
log_to_database($data);
sleep(10);  // This doesn't delay the response
```

**Behavior:**
- Output before `tokio_finish_request()` is sent to client
- Output after is discarded
- Headers set before are included; headers set after are excluded
- Script continues executing until completion
- Idempotent (multiple calls have no effect)

**Use cases:** Webhooks, async notifications, background logging, cleanup tasks.

### PHP Worker Pool

- Multi-threaded worker pool (threads = `PHP_WORKERS` or CPU count)
- Channel-based work distribution (`mpsc::channel` → workers)
- Each worker: `php_request_startup()` → execute → `php_request_shutdown()`
- Output captured via memfd + stdout redirection
- Superglobals injected via `zend_eval_string` before script execution

### TLS Implementation

- Uses `tokio-rustls` with `rustls` for TLS 1.3
- Certificates loaded from PEM files via `TLS_CERT` and `TLS_KEY` env vars
- Docker secrets support: certificates mounted at `/run/secrets/tls_cert` and `/run/secrets/tls_key`
- ALPN protocols: `h2`, `http/1.1` for automatic HTTP/2 negotiation
- Self-signed dev certificates in `certs/` directory

### Key Technical Details

- SAPI name set to "cli-server" before `php_embed_init` for OPcache/JIT compatibility
- PHP 8.5/8.4 ZTS (Thread Safe) build required
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
| `REQUEST_TIMEOUT` | `2m` | Request timeout (30s, 2m, 5m, off). Returns 504 on timeout. Use `tokio_request_heartbeat()` to extend |
| `ACCESS_LOG` | `0` | Enable access logs (target: `access`) |
| `RATE_LIMIT` | `0` | Max requests per IP per window (0 = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window in seconds |
| `USE_STUB` | `0` | Stub mode - disable PHP, return empty responses |
| `USE_EXT` | `1` | **Recommended.** Use ExtExecutor with tokio_sapi extension (2x faster) |
| `PROFILE` | `0` | Enable profiling (requires `X-Profile: 1` header) |
| `TLS_CERT` | _(empty)_ | Path to TLS certificate (PEM). In Docker: `/run/secrets/tls_cert` |
| `TLS_KEY` | _(empty)_ | Path to TLS private key (PEM). In Docker: `/run/secrets/tls_key` |
| `TLS_CERT_FILE` | `./certs/cert.pem` | Docker secrets: host path to certificate file |
| `TLS_KEY_FILE` | `./certs/key.pem` | Docker secrets: host path to private key file |
| `RUST_LOG` | `tokio_php=info` | Log level (trace, debug, info, warn, error) |

### Auto-calculated Defaults

- `PHP_WORKERS=0` → uses `num_cpus::get()` (all available CPU cores)
- `QUEUE_CAPACITY=0` → uses `workers × 100` (e.g., 8 workers = 800 queue capacity)

### Configuration Examples

```bash
# Minimal (all defaults, uses ExtExecutor)
docker compose up -d

# Production with tuning
PHP_WORKERS=8 docker compose up -d

# Benchmark mode (no PHP execution)
USE_STUB=1 docker compose up -d

# Laravel/Symfony single entry point
INDEX_FILE=index.php DOCUMENT_ROOT=/var/www/html/public docker compose up -d

# With TLS/HTTPS (uses Docker secrets, default: ./certs/)
docker compose --profile tls up -d

# With TLS/HTTPS (custom certificate files via secrets)
TLS_CERT_FILE=/path/to/cert.pem TLS_KEY_FILE=/path/to/key.pem docker compose --profile tls up -d

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

```json
{"ts":"2025-01-15T10:30:00.123Z","level":"info","type":"app","msg":"Server started","ctx":{"service":"tokio_php"},"data":{}}
```

### Quick Reference

| Field | Description |
|-------|-------------|
| `ts` | ISO 8601 timestamp (UTC) |
| `level` | `debug`, `info`, `warn`, `error` |
| `type` | `app`, `access`, `error` |
| `msg` | Human-readable message |
| `ctx` | Context: service, request_id, trace_id |
| `data` | Structured data |

### Filtering with jq

```bash
docker compose logs | jq -c 'select(.type == "access")'
docker compose logs | jq -c 'select(.level == "error")'
docker compose logs | jq -c 'select(.data.status >= 500)'
```

### Monolog Integration

Use `TokioPhpFormatter` for consistent log format in PHP apps:

```php
$handler = new StreamHandler('php://stderr');
$handler->setFormatter(new TokioPhpFormatter('myapp'));
```

See [docs/logging.md](docs/logging.md) for full documentation, Laravel/Symfony integration, and log aggregation setup.

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
Content-Type: text/plain

429 Too Many Requests
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
| `tokio_php` | 8080, 9090 | HTTP + internal server |
| `tokio_php_tls` | 8443, 9090 | HTTPS + internal server (profile: tls) |

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

## HTTP Methods

All standard HTTP methods are supported:

| Method | Body | `php://input` | `$_POST` parsing |
|--------|------|---------------|------------------|
| GET | No | — | — |
| HEAD | No | — | — |
| POST | Yes | ✓ | ✓ (urlencoded/multipart) |
| PUT | Yes | ✓ | — |
| PATCH | Yes | ✓ | — |
| DELETE | May | ✓ | — |
| OPTIONS | Yes | ✓ | — |
| QUERY | Yes | ✓ | ✓ (urlencoded/multipart) |

### Request Body Access

```php
<?php
// Method name
$_SERVER['REQUEST_METHOD'];  // "PUT", "PATCH", "QUERY", etc.

// Raw body via php://input (standard PHP way)
$body = file_get_contents('php://input');
$data = json_decode($body, true);

// Content info
$_SERVER['CONTENT_TYPE'];    // "application/json"
$_SERVER['CONTENT_LENGTH'];  // Body size in bytes

// For urlencoded POST, data is also in $_POST
$_POST['field'];
```

### QUERY Method

The [HTTP QUERY method](https://httpwg.org/http-extensions/draft-ietf-httpbis-safe-method-w-body.html) is a safe, idempotent method with body support (like POST, but cacheable like GET).

```bash
curl -X QUERY -H "Content-Type: application/json" \
  -d '{"search":"test"}' http://localhost:8080/api.php
```

## Distributed Tracing

W3C Trace Context support for request correlation across microservices.

### Headers

| Header | Direction | Description |
|--------|-----------|-------------|
| `traceparent` | Request | Incoming W3C trace context (optional) |
| `traceparent` | Response | Outgoing W3C trace context (always) |
| `x-request-id` | Response | Short ID: `{trace_id[0:12]}-{span_id[0:4]}` |

### PHP $_SERVER Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `TRACE_ID` | 32-char trace identifier | `0af7651916cd43dd8448eb211c80319c` |
| `SPAN_ID` | 16-char span identifier | `b7ad6b7169203331` |
| `PARENT_SPAN_ID` | Parent span (if propagated) | `a1b2c3d4e5f67890` |
| `HTTP_TRACEPARENT` | Full W3C traceparent header | `00-0af7...-b7ad...-01` |

### Usage in PHP

```php
<?php
// Access trace context
$traceId = $_SERVER['TRACE_ID'];
$spanId = $_SERVER['SPAN_ID'];

// Propagate to downstream services
$ch = curl_init('https://api.example.com');
curl_setopt($ch, CURLOPT_HTTPHEADER, [
    "traceparent: 00-{$traceId}-" . bin2hex(random_bytes(8)) . "-01"
]);
```

### Access Logs

With `ACCESS_LOG=1`, logs include trace context in `ctx`:

```json
{
  "ctx": {
    "service": "tokio_php",
    "request_id": "0af7651916cd-b7ad",
    "trace_id": "0af7651916cd43dd8448eb211c80319c",
    "span_id": "b7ad6b7169203331"
  }
}
```

See [docs/distributed-tracing.md](docs/distributed-tracing.md) for full documentation.

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

Behavior (nginx `try_files` equivalent):
- Static files served directly if they exist (e.g., `/style.css` -> served)
- Other requests route to index file (e.g., `/api/users` -> `index.php`)
- Direct access to the index file returns 404 (e.g., `/index.php` -> 404)
- Index file existence validated at startup (server exits if missing)

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
| `/config` | Current server configuration (JSON) |

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

# Configuration
curl http://localhost:9090/config
{"listen_addr":"0.0.0.0:8080","document_root":"/var/www/html","workers":14,"queue_capacity":1400,"executor":"ext","index_file":null,"internal_addr":"0.0.0.0:9090","tls_enabled":false,"drain_timeout_secs":30,"static_cache_ttl":"1d","request_timeout":"2m","profile_enabled":false,"access_log_enabled":false,"rate_limit":null,"rate_window_secs":60,"error_pages_enabled":true}

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

## Framework Compatibility

Symfony, Laravel, and other frameworks compile cache on-the-fly in development mode. These file operations are not thread-safe and cause segfaults with multiple workers.

| Mode | Workers | Setup |
|------|---------|-------|
| **Development** | `PHP_WORKERS=1` | Single worker (safe) |
| **Production** | `PHP_WORKERS=0` (auto) | Pre-warm cache first |

### Symfony

```bash
# Development
PHP_WORKERS=1 APP_ENV=dev docker compose up -d

# Production
docker compose exec app php bin/console cache:warmup --env=prod
APP_ENV=prod PHP_WORKERS=0 docker compose up -d
```

### Laravel

```bash
# Development
PHP_WORKERS=1 APP_ENV=local docker compose up -d

# Production
docker compose exec app php artisan optimize
APP_ENV=production PHP_WORKERS=0 docker compose up -d
```

See [docs/framework-compatibility.md](docs/framework-compatibility.md) for full documentation.

## Limitations

- No `$_SESSION` support (requires session handler implementation)
- HTTP/3 (QUIC) not yet implemented (h3 crate is experimental)
