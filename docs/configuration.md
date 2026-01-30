# Configuration

tokio_php is configured via environment variables.

## Environment Variables Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | HTTP server bind address |
| `PHP_WORKERS` | `0` | Worker count (0 = auto-detect CPU cores) |
| `QUEUE_CAPACITY` | `0` | Max pending requests (0 = workers × 100) |
| `DOCUMENT_ROOT` | `/var/www/html` | Web root directory |
| `INDEX_FILE` | _(empty)_ | Single entry point mode (e.g., `index.php`) |
| `INTERNAL_ADDR` | _(empty)_ | Internal server for /health and /metrics |
| `ERROR_PAGES_DIR` | _(empty)_ | Directory with custom HTML error pages |
| `DRAIN_TIMEOUT_SECS` | `30` | Graceful shutdown drain timeout (seconds) |
| `STATIC_CACHE_TTL` | `1d` | Static file cache duration (1d, 1w, 1m, 1y, off) |
| `REQUEST_TIMEOUT` | `2m` | Request timeout (30s, 2m, 5m, off). Returns 504 on timeout |
| `SSE_TIMEOUT` | `30m` | SSE connection timeout (30m, 1h, off). Separate from REQUEST_TIMEOUT |
| `ACCESS_LOG` | `0` | Enable access logs (target: `access`) |
| `RATE_LIMIT` | `0` | Max requests per IP per window (0 = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window in seconds |
| `EXECUTOR` | `ext` | Script executor: `ext` (recommended), `php` (legacy), `stub` (benchmark) |
| `TLS_CERT` | _(empty)_ | Path to TLS certificate (PEM) |
| `TLS_KEY` | _(empty)_ | Path to TLS private key (PEM) |
| `TLS_CERT_FILE` | `./certs/cert.pem` | Docker secrets: host path to certificate |
| `TLS_KEY_FILE` | `./certs/key.pem` | Docker secrets: host path to private key |
| `RUST_LOG` | `tokio_php=info` | Log level |
| `SERVICE_NAME` | `tokio_php` | Service name in structured logs |
| `PHP_VERSION` | `8.5` | Docker build: PHP version (8.4 or 8.5) |

### OpenTelemetry (requires `otel` feature)

| Variable | Default | Description |
|----------|---------|-------------|
| `OTEL_ENABLED` | `0` | Enable OpenTelemetry tracing (`1` = enabled) |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP gRPC endpoint |
| `OTEL_SERVICE_NAME` | `tokio_php` | Service name in traces |
| `OTEL_SERVICE_VERSION` | _(from Cargo)_ | Service version |
| `OTEL_ENVIRONMENT` | `development` | Deployment environment |
| `OTEL_SAMPLING_RATIO` | `1.0` | Sampling ratio (0.0-1.0) |

### Monitoring Stack (docker compose --profile monitoring)

| Variable | Default | Description |
|----------|---------|-------------|
| `GRAFANA_USER` | `admin` | Grafana admin username |
| `GRAFANA_PASSWORD` | `admin` | Grafana admin password |

## Detailed Configuration

### LISTEN_ADDR

HTTP server bind address and port.

```bash
# Default - all interfaces, port 8080
LISTEN_ADDR=0.0.0.0:8080

# Localhost only
LISTEN_ADDR=127.0.0.1:8080

# Custom port
LISTEN_ADDR=0.0.0.0:80
```

### PHP_WORKERS

Number of PHP worker threads.

```bash
# Auto-detect (recommended)
PHP_WORKERS=0

# Fixed count
PHP_WORKERS=4
PHP_WORKERS=8
PHP_WORKERS=16
```

| Value | Behavior |
|-------|----------|
| `0` | Auto-detect using `num_cpus::get()` |
| `N` | Use exactly N workers |

Recommendation: Start with auto-detect, adjust based on workload.

See [Worker Pool](worker-pool.md) for details on worker architecture.

### QUEUE_CAPACITY

Maximum pending requests in the worker queue.

```bash
# Auto-calculate (workers × 100)
QUEUE_CAPACITY=0

# Custom capacity
QUEUE_CAPACITY=500
QUEUE_CAPACITY=1000
```

| Value | Behavior |
|-------|----------|
| `0` | `workers × 100` (e.g., 8 workers = 800) |
| `N` | Fixed queue size |

When queue is full, new requests receive HTTP 503 with `Retry-After: 1`.

### DOCUMENT_ROOT

Web root directory for serving files.

```bash
# Default
DOCUMENT_ROOT=/var/www/html

# Laravel/Symfony
DOCUMENT_ROOT=/var/www/html/public

# Custom path
DOCUMENT_ROOT=/app/public
```

### INDEX_FILE

Enable single entry point mode for frameworks.

```bash
# Disabled (default)
INDEX_FILE=

# Laravel/Symfony
INDEX_FILE=index.php
```

When set:
- All requests route to the specified file
- Direct access to index file returns 404
- File existence validated at startup

See [Single Entry Point](single-entry-point.md) for framework integration details.

### INTERNAL_ADDR

Enable internal HTTP server for health checks and metrics.

```bash
# Disabled (default)
INTERNAL_ADDR=

# Enable on port 9090
INTERNAL_ADDR=0.0.0.0:9090

# Localhost only
INTERNAL_ADDR=127.0.0.1:9090
```

Provides endpoints:
- `/health` - JSON health check
- `/metrics` - Prometheus-compatible metrics
- `/config` - Current server configuration (JSON)

See [Internal Server](internal-server.md) for endpoint details and Prometheus integration.

### ERROR_PAGES_DIR

Directory containing custom HTML error pages for 4xx/5xx responses.

```bash
# Disabled (default)
ERROR_PAGES_DIR=

# Enable with custom directory
ERROR_PAGES_DIR=/var/www/html/errors
ERROR_PAGES_DIR=/app/errors
```

**File Naming**: Files must be named `{status_code}.html`:
- `404.html` - Not Found
- `500.html` - Internal Server Error
- `503.html` - Service Unavailable

**Behavior**:
- Files are cached in memory at server startup for performance
- Only served when client sends `Accept: text/html` header
- Only applied to 4xx/5xx responses with empty body
- Missing files fall back to default text response
- Files are served as-is (not processed through PHP)

**Example Setup**:

```bash
# Directory structure
www/
  errors/
    404.html
    500.html
    503.html

# Run with custom error pages
ERROR_PAGES_DIR=/var/www/html/errors docker compose up -d
```

**Example 404.html**:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>404 - Not Found</title>
</head>
<body>
    <h1>404</h1>
    <p>The page you're looking for doesn't exist.</p>
    <a href="/">Go Home</a>
</body>
</html>
```

See [Error Pages](error-pages.md) for more examples and best practices.

### DRAIN_TIMEOUT_SECS

Graceful shutdown drain timeout in seconds.

```bash
# Default: 30 seconds
DRAIN_TIMEOUT_SECS=30

# Quick shutdown for development
DRAIN_TIMEOUT_SECS=5

# Kubernetes: match terminationGracePeriodSeconds minus preStop
DRAIN_TIMEOUT_SECS=25
```

When the server receives SIGTERM/SIGINT:
1. Stops accepting new connections
2. Waits for in-flight requests to complete
3. Forces shutdown after timeout

| Value | Use Case |
|-------|----------|
| `5` | Development/testing |
| `25-30` | Production with Kubernetes |
| `60` | Long-running requests |

**Kubernetes example:**
```yaml
spec:
  terminationGracePeriodSeconds: 30
  containers:
    - lifecycle:
        preStop:
          exec:
            command: ["sleep", "5"]  # LB drain time
```

See [Graceful Shutdown](graceful-shutdown.md) for Kubernetes deployment details.

### STATIC_CACHE_TTL

Cache duration for static files (CSS, JS, images, fonts, etc.).

```bash
# Default: 1 day
STATIC_CACHE_TTL=1d

# 1 week (recommended for production)
STATIC_CACHE_TTL=1w

# ~1 month (30 days)
STATIC_CACHE_TTL=30d

# 1 year (for versioned assets)
STATIC_CACHE_TTL=1y

# Disable caching
STATIC_CACHE_TTL=off
```

| Format | Duration | Seconds |
|--------|----------|---------|
| `1s` | 1 second | 1 |
| `1m` | 1 minute | 60 |
| `1h` | 1 hour | 3,600 |
| `1d` | 1 day | 86,400 |
| `1w` | 1 week | 604,800 |
| `1y` | ~1 year | 31,536,000 |
| `off` | disabled | - |

**Note:** There is no month unit. Use `30d` for approximately one month.

**Response headers when enabled:**

```http
Cache-Control: public, max-age=86400
Expires: Mon, 30 Dec 2024 12:00:00 GMT
ETag: "1a2b-65a51a2d"
Last-Modified: Sun, 29 Dec 2024 12:00:00 GMT
```

**Note:** Only static files receive caching headers. PHP responses are not affected.

See [Static Caching](static-caching.md) for cache strategies and CDN integration.

### REQUEST_TIMEOUT

Maximum time for a request to complete before returning 504 Gateway Timeout.

```bash
# Default: 2 minutes
REQUEST_TIMEOUT=2m

# 30 seconds (strict)
REQUEST_TIMEOUT=30s

# 5 minutes (long-running scripts)
REQUEST_TIMEOUT=5m

# 10 minutes (batch processing)
REQUEST_TIMEOUT=10m

# Disable timeout (not recommended)
REQUEST_TIMEOUT=off
```

| Format | Duration |
|--------|----------|
| `30s` | 30 seconds |
| `2m` | 2 minutes |
| `5m` | 5 minutes |
| `1h` | 1 hour |
| `off` | No timeout |

**Behavior:**
- When timeout is reached, server returns HTTP 504 Gateway Timeout
- PHP script continues running until `max_execution_time` is reached
- Use `tokio_request_heartbeat()` to extend deadline for long-running scripts

**Heartbeat extension:**

```php
<?php

// Extend deadline by 30 seconds
set_time_limit(30);
tokio_request_heartbeat(30);
```

See [Request Heartbeat](request-heartbeat.md) for details.

### SSE_TIMEOUT

Timeout for Server-Sent Events (SSE) connections. Separate from `REQUEST_TIMEOUT` because SSE connections are typically long-lived.

```bash
# Default: 30 minutes
SSE_TIMEOUT=30m

# 1 hour
SSE_TIMEOUT=1h

# 2 hours (for long-running streams)
SSE_TIMEOUT=2h

# Disable timeout (not recommended)
SSE_TIMEOUT=off
```

| Format | Duration |
|--------|----------|
| `30m` | 30 minutes |
| `1h` | 1 hour |
| `2h` | 2 hours |
| `off` | No timeout |

**Behavior:**
- When timeout is reached, SSE connection is closed
- Use `tokio_request_heartbeat()` to extend deadline for active streams
- SSE connections bypass `REQUEST_TIMEOUT` and use this dedicated timeout

See [SSE Streaming](sse-streaming.md) for implementation details.

### ACCESS_LOG

Enable access logs.

```bash
# Disabled (default)
ACCESS_LOG=0

# Enabled
ACCESS_LOG=1
```

Access logs use unified JSON format:

```json
{"ts":"2025-01-15T10:30:00.123Z","level":"info","type":"access","msg":"GET /api/users 200","ctx":{"service":"tokio_php","request_id":"65bdbab40000","trace_id":"0af7651916cd43dd8448eb211c80319c","span_id":"b7ad6b7169203331"},"data":{"method":"GET","path":"/api/users","status":200,"bytes":1234,"duration_ms":5.25,"ip":"10.0.0.1"}}
```

**Context fields (`ctx`):**

| Field | Type | Description |
|-------|------|-------------|
| `service` | string | Service name (configurable via `SERVICE_NAME`) |
| `request_id` | string | Short request ID for logs |
| `trace_id` | string | W3C trace ID (32 hex chars) |
| `span_id` | string | Span ID (16 hex chars) |

**Data fields (`data`):**

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

See [Middleware](middleware.md) for access log middleware details.

### Request ID

Every request includes a unique ID for distributed tracing:

- **Response header**: `X-Request-ID` in every response
- **Log field**: `ctx.request_id` in access logs
- **Propagation**: Incoming `X-Request-ID` header is preserved

```bash
# Check response header
curl -sI http://localhost:8080/index.php | grep x-request-id
x-request-id: 65bdbab40000

# Propagate existing ID
curl -sI -H "X-Request-ID: trace-123" http://localhost:8080/ | grep x-request-id
x-request-id: trace-123

# Filter logs by request ID
docker compose logs | jq -c 'select(.ctx.request_id == "trace-123")'
```

**Docker/Kubernetes integration:**

```bash
# Filter by type
docker compose logs | jq -c 'select(.type == "access")'

# Filter errors (4xx/5xx)
docker compose logs | jq -c 'select(.data.status >= 400)'
```

See [Distributed Tracing](distributed-tracing.md) for W3C Trace Context integration.

### RATE_LIMIT / RATE_WINDOW

Per-IP rate limiting to prevent abuse.

```bash
# Disabled (default)
RATE_LIMIT=0

# 100 requests per minute per IP
RATE_LIMIT=100
RATE_WINDOW=60

# 1000 requests per hour
RATE_LIMIT=1000
RATE_WINDOW=3600

# Strict: 10 requests per 10 seconds
RATE_LIMIT=10
RATE_WINDOW=10
```

**Response when rate limited:**

```
HTTP/1.1 429 Too Many Requests
Content-Type: text/plain
Retry-After: 45
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 45

429 Too Many Requests
```

**Headers:**

| Header | Description |
|--------|-------------|
| `Retry-After` | Seconds until limit resets (RFC 7231) |
| `X-RateLimit-Limit` | Maximum requests per window |
| `X-RateLimit-Remaining` | Remaining requests in current window |
| `X-RateLimit-Reset` | Seconds until window resets |

**Algorithm:** Fixed window per IP address. Counter resets when window expires.

**vs QUEUE_CAPACITY:**

| Mechanism | Scope | Response | Purpose |
|-----------|-------|----------|---------|
| `RATE_LIMIT` | Per-IP | 429 | Fairness, abuse prevention |
| `QUEUE_CAPACITY` | Global | 503 | Server overload protection |

Rate limit is checked first. If passed, request enters queue.

See [Rate Limiting](rate-limiting.md) for algorithm details and best practices.

### EXECUTOR

Select the script execution backend. **Default: `ext` (recommended).**

```bash
# ExtExecutor - FFI superglobals + php_execute_script() (default, recommended)
EXECUTOR=ext

# PhpExecutor - eval-based superglobals (legacy)
EXECUTOR=php

# StubExecutor - no PHP execution (for benchmarking)
EXECUTOR=stub
```

| Value | Executor | Method | Use Case |
|-------|----------|--------|----------|
| `ext` | ExtExecutor | `php_execute_script()` + FFI | **Production (2x faster)** |
| `php` | PhpExecutor | `zend_eval_string()` | Legacy/debugging |
| `stub` | StubExecutor | No PHP | Benchmarking HTTP overhead |

ExtExecutor is **2x faster** than PhpExecutor:
- Uses native `php_execute_script()` - fully optimized for OPcache/JIT
- Sets superglobals via direct FFI calls (no eval parsing)
- ~36K RPS vs ~16K RPS for index.php

See [Architecture](architecture.md) for executor comparison and [tokio_sapi Extension](tokio-sapi-extension.md) for FFI details.

### Profiling (debug-profile feature)

Request profiling is enabled at **compile time** using the `debug-profile` Cargo feature.

```bash
# Build with profiling
cargo build --release --features debug-profile

# Docker build with profiling
CARGO_FEATURES=debug-profile docker compose build
```

When built with `debug-profile`:
- Server runs in **single-worker mode** for accurate timing
- All requests generate detailed reports to `/tmp/tokio_profile_request_{request_id}.md`

See [Profiling](profiling.md) for report format and detailed usage.

### TLS_CERT / TLS_KEY

Enable HTTPS with TLS.

```bash
# HTTP only (default)
TLS_CERT=
TLS_KEY=

# HTTPS
TLS_CERT=/certs/cert.pem
TLS_KEY=/certs/key.pem
```

Both variables must be set for TLS to be enabled.

**Docker Secrets:**

For Docker deployments, use `TLS_CERT_FILE` and `TLS_KEY_FILE` to specify host paths:

```bash
# Use default paths (./certs/cert.pem, ./certs/key.pem)
docker compose --profile tls up -d

# Custom certificate paths
TLS_CERT_FILE=/path/to/cert.pem TLS_KEY_FILE=/path/to/key.pem docker compose --profile tls up -d
```

See [HTTP/2 & TLS](http2-tls.md) for certificate setup and protocol configuration.

### OpenTelemetry

Enable distributed tracing with OpenTelemetry (requires `otel` feature).

```bash
# Build with OpenTelemetry support
CARGO_FEATURES=otel docker compose build

# Enable tracing
OTEL_ENABLED=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://jaeger:4317 \
OTEL_SERVICE_NAME=my-app \
docker compose up -d
```

| Variable | Default | Description |
|----------|---------|-------------|
| `OTEL_ENABLED` | `0` | Enable tracing (`1` = enabled) |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP gRPC endpoint (Jaeger, Tempo, etc.) |
| `OTEL_SERVICE_NAME` | `tokio_php` | Service name in traces |
| `OTEL_SERVICE_VERSION` | _(Cargo.toml)_ | Service version |
| `OTEL_ENVIRONMENT` | `development` | Environment: `development`, `staging`, `production` |
| `OTEL_SAMPLING_RATIO` | `1.0` | Sampling ratio (0.0 = none, 1.0 = all) |

**Sampling recommendations:**
- `1.0` - Development/staging (all requests)
- `0.1` - Production (10% of requests)
- `0.01` - High-traffic production (1% of requests)

See [Observability](observability.md) for full tracing documentation.

### Monitoring Stack

Enable Prometheus and Grafana with Docker Compose profile:

```bash
# Start with monitoring
docker compose --profile monitoring up -d

# Access:
# - Prometheus: http://localhost:9091
# - Grafana: http://localhost:3000 (admin/admin)
```

| Variable | Default | Description |
|----------|---------|-------------|
| `GRAFANA_USER` | `admin` | Grafana admin username |
| `GRAFANA_PASSWORD` | `admin` | Grafana admin password |

See [Observability](observability.md) for metrics and dashboard details.

### PHP_VERSION

Docker build argument for PHP version selection.

```bash
# PHP 8.5 (default)
docker compose build

# PHP 8.4
PHP_VERSION=8.4 docker compose build
```

Supported versions: `8.4`, `8.5`

### RUST_LOG

Configure log level and filtering. All logs use unified JSON format.

```bash
# Default - info level
RUST_LOG=tokio_php=info

# Debug mode
RUST_LOG=tokio_php=debug

# Trace level (very verbose)
RUST_LOG=tokio_php=trace

# Warning only (suppress info)
RUST_LOG=tokio_php=warn
```

Log levels: `trace`, `debug`, `info`, `warn`, `error`

**Note:** Access logs (when `ACCESS_LOG=1`) are always output regardless of RUST_LOG level. Use `jq` to filter by type.

## Configuration Examples

### Minimal (All Defaults)

```bash
docker compose up -d
```

### Production

```bash
PHP_WORKERS=8 \
QUEUE_CAPACITY=1000 \
INTERNAL_ADDR=0.0.0.0:9090 \
docker compose up -d  # EXECUTOR=ext by default
```

### Laravel/Symfony

```bash
INDEX_FILE=index.php \
DOCUMENT_ROOT=/var/www/html/public \
PHP_WORKERS=8 \
docker compose up -d
```

### Development

```bash
RUST_LOG=tokio_php=debug \
PHP_WORKERS=2 \
docker compose up -d
```

For profiling, build with `debug-profile` feature (see [Profiling](profiling.md)).

### Benchmark Mode

```bash
EXECUTOR=stub docker compose up -d
```

### With TLS

```bash
TLS_CERT=/certs/cert.pem \
TLS_KEY=/certs/key.pem \
docker compose --profile tls up -d
```

### Full Configuration

```bash
LISTEN_ADDR=0.0.0.0:8080 \
PHP_WORKERS=8 \
QUEUE_CAPACITY=1000 \
DOCUMENT_ROOT=/var/www/html/public \
INDEX_FILE=index.php \
INTERNAL_ADDR=0.0.0.0:9090 \
ERROR_PAGES_DIR=/var/www/html/errors \
STATIC_CACHE_TTL=1w \
ACCESS_LOG=1 \
EXECUTOR=ext \
RUST_LOG=tokio_php=info \
docker compose up -d
```

## docker-compose.yml

```yaml
services:
  tokio_php:
    build:
      context: .
      args:
        PHP_VERSION: ${PHP_VERSION:-8.5}
    ports:
      - "8080:8080"
      - "9090:9090"
    environment:
      - LISTEN_ADDR=0.0.0.0:8080
      - RUST_LOG=${RUST_LOG:-tokio_php=info}
      - SERVICE_NAME=${SERVICE_NAME:-tokio_php}
      - PHP_WORKERS=${PHP_WORKERS:-0}
      - QUEUE_CAPACITY=${QUEUE_CAPACITY:-0}
      - EXECUTOR=${EXECUTOR:-ext}  # ext (recommended), php, stub
      - INDEX_FILE=${INDEX_FILE:-}
      - DOCUMENT_ROOT=${DOCUMENT_ROOT:-/var/www/html}
      - INTERNAL_ADDR=0.0.0.0:9090
      - ERROR_PAGES_DIR=${ERROR_PAGES_DIR:-/var/www/html/errors}
      - DRAIN_TIMEOUT_SECS=${DRAIN_TIMEOUT_SECS:-30}
      - ACCESS_LOG=${ACCESS_LOG:-0}
      - RATE_LIMIT=${RATE_LIMIT:-0}
      - RATE_WINDOW=${RATE_WINDOW:-60}
      - STATIC_CACHE_TTL=${STATIC_CACHE_TTL:-1d}
      - REQUEST_TIMEOUT=${REQUEST_TIMEOUT:-2m}
    volumes:
      - ./www:/var/www/html:ro
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:9090/health"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s
```

## Validation

### Check Current Configuration

```bash
# View environment
docker compose exec app env | grep -E '^(PHP_|QUEUE_|DOCUMENT_|INDEX_|INTERNAL_|USE_|TLS_|RUST_|LISTEN_)'

# View startup logs
docker compose logs app | head -20
```

### Expected Startup Output

Logs are in JSON format. Key startup messages:

```bash
# View formatted startup logs
docker compose logs tokio_php | jq -r 'select(.type == "app") | .msg' | head -20
```

```
Configuration loaded:
  Listen: 0.0.0.0:8080
  Document root: "/var/www/html"
  Workers: 14
  Queue capacity: 1400
  Executor: Ext
  Internal server: 0.0.0.0:9090
  Static cache TTL: 86400s
  Request timeout: 120s
Starting tokio_php server...
Initializing EXT executor with 14 workers (FFI superglobals)...
PHP initialized with SAPI 'cli-server' (OPcache compatible, custom header handler)
ExtExecutor ready (14 workers, FFI mode)
Loaded 3 error pages: [404, 503, 500]
Server listening on http://0.0.0.0:8080 (executor: ext, workers: 14)
Internal server listening on http://0.0.0.0:9090
```

Raw JSON format:
```json
{"ctx":{"service":"tokio_php"},"data":{},"level":"info","msg":"Server listening on http://0.0.0.0:8080 (executor: ext, workers: 14)","ts":"2025-01-15T10:30:00.123Z","type":"app"}
```

## Troubleshooting

### Server Won't Start

Check for port conflicts:
```bash
lsof -i :8080
```

### Workers Not Starting

Check PHP ZTS is available:
```bash
docker compose exec app php -v
# Should show "ZTS" in output
```

### TLS Not Working

Verify certificate paths:
```bash
docker compose exec app ls -la /certs/
```

### Low Performance

Build with profiling to identify bottlenecks:
```bash
# Build with profiling (single-worker mode)
CARGO_FEATURES=debug-profile docker compose build
docker compose up -d

# Make request and check report
curl http://localhost:8080/index.php
docker compose exec tokio_php cat /tmp/tokio_profile_request_*.md
```

See [Profiling](profiling.md) for detailed analysis.

## See Also

- [Docker](docker.md) - Environment variables in Docker Compose
- [Architecture](architecture.md) - System design and components
- [Middleware](middleware.md) - Middleware system overview
- [Internal Server](internal-server.md) - Health checks and Prometheus metrics
- [Worker Pool](worker-pool.md) - PHP worker configuration
- [Profiling](profiling.md) - Request timing analysis
- [Compression](compression.md) - Brotli compression settings
- [Superglobals](superglobals.md) - PHP superglobals support
- [tokio_sapi Extension](tokio-sapi-extension.md) - ExtExecutor PHP functions

---

## For Developers

This section describes the configuration system internals for developers extending tokio_php.

### Configuration Module Structure

```
src/config/
├── mod.rs           # Config struct, aggregates all configs
├── server.rs        # ServerConfig (listen, document_root, TLS, etc.)
├── executor.rs      # ExecutorConfig (workers, queue, executor type)
├── middleware.rs    # MiddlewareConfig (rate limit, access log, profile)
├── logging.rs       # LoggingConfig (log level, format)
├── parse.rs         # Helper functions for parsing env vars
└── error.rs         # ConfigError enum
```

### Using Config in Code

```rust
use tokio_php::config::Config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load all configuration from environment
    let config = Config::from_env()?;

    // Access individual configs
    println!("Listen: {}", config.server.listen_addr);
    println!("Workers: {}", config.executor.worker_count());
    println!("Queue capacity: {}", config.executor.actual_queue_capacity());

    // Log configuration summary
    config.log_summary();

    Ok(())
}
```

### Config Struct

```rust
/// Complete application configuration.
pub struct Config {
    pub server: ServerConfig,       // Server settings
    pub executor: ExecutorConfig,   // PHP executor settings
    pub middleware: MiddlewareConfig, // Middleware settings
    pub logging: LoggingConfig,     // Logging settings
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError>;

    /// Print configuration summary to log.
    pub fn log_summary(&self);
}
```

### ServerConfig

```rust
pub struct ServerConfig {
    pub listen_addr: SocketAddr,           // LISTEN_ADDR
    pub document_root: PathBuf,            // DOCUMENT_ROOT
    pub index_file: Option<String>,        // INDEX_FILE
    pub internal_addr: Option<SocketAddr>, // INTERNAL_ADDR
    pub error_pages_dir: Option<PathBuf>,  // ERROR_PAGES_DIR
    pub drain_timeout: Duration,           // DRAIN_TIMEOUT_SECS
    pub static_cache_ttl: StaticCacheTtl,  // STATIC_CACHE_TTL
    pub request_timeout: RequestTimeout,   // REQUEST_TIMEOUT
    pub tls: TlsConfig,                    // TLS_CERT, TLS_KEY
}
```

### ExecutorConfig

```rust
pub struct ExecutorConfig {
    pub executor_type: ExecutorType,  // EXECUTOR env var
    pub workers: usize,               // PHP_WORKERS (0 = auto)
    pub queue_capacity: usize,        // QUEUE_CAPACITY (0 = auto)
}

impl ExecutorConfig {
    /// Get actual worker count (resolves 0 to CPU count).
    pub fn worker_count(&self) -> usize;

    /// Get actual queue capacity (resolves 0 to workers * 100).
    pub fn actual_queue_capacity(&self) -> usize;
}

pub enum ExecutorType {
    Stub,  // EXECUTOR=stub
    Php,   // EXECUTOR=php (legacy)
    Ext,   // EXECUTOR=ext (default, recommended)
}
```

### MiddlewareConfig

```rust
pub struct MiddlewareConfig {
    pub rate_limit: Option<u64>,  // RATE_LIMIT (None if 0)
    pub rate_window: u64,         // RATE_WINDOW
    pub access_log: bool,         // ACCESS_LOG
}
```

Note: Profiling is controlled at compile-time via `debug-profile` feature, not at runtime.

### LoggingConfig

```rust
pub struct LoggingConfig {
    pub filter: String,       // RUST_LOG
    pub service_name: String, // SERVICE_NAME
}
```

### Helper Functions (parse.rs)

```rust
/// Get environment variable or default.
pub fn env_or(key: &str, default: &str) -> String;

/// Get optional environment variable.
pub fn env_opt(key: &str) -> Option<String>;

/// Parse boolean from environment (1 or "true" = true, else default).
pub fn env_bool(key: &str, default: bool) -> bool;

/// Parse duration string (e.g., "30s", "2m", "1h", "1d", "off").
pub fn parse_duration(s: &str) -> Result<Option<Duration>, ParseError>;
```

### Duration Parsing

The `parse_duration` function supports:

| Suffix | Unit | Example |
|--------|------|---------|
| `s` | Seconds | `30s` → 30 seconds |
| `m` | Minutes | `2m` → 120 seconds |
| `h` | Hours | `1h` → 3600 seconds |
| `d` | Days | `1d` → 86400 seconds |
| `w` | Weeks | `1w` → 604800 seconds |
| `y` | Years | `1y` → 31536000 seconds |
| `off` | None | Disabled |
| `0` | None | Disabled |

### Adding New Configuration

1. **Add to appropriate config struct** (`server.rs`, `executor.rs`, etc.):

```rust
// In server.rs
pub struct ServerConfig {
    // ... existing fields
    pub my_new_option: Option<String>,  // MY_NEW_OPTION
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            // ... existing fields
            my_new_option: env_opt("MY_NEW_OPTION"),
        })
    }
}
```

2. **Update log_summary in mod.rs** (optional):

```rust
impl Config {
    pub fn log_summary(&self) {
        // ... existing logs
        if let Some(ref opt) = self.server.my_new_option {
            info!("  My new option: {}", opt);
        }
    }
}
```

3. **Add tests**:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_my_new_option_default() {
        std::env::remove_var("MY_NEW_OPTION");
        let config = ServerConfig::from_env().unwrap();
        assert!(config.my_new_option.is_none());
    }

    #[test]
    fn test_my_new_option_set() {
        std::env::set_var("MY_NEW_OPTION", "value");
        let config = ServerConfig::from_env().unwrap();
        assert_eq!(config.my_new_option, Some("value".to_string()));
        std::env::remove_var("MY_NEW_OPTION");
    }
}
```

### ConfigError

```rust
pub enum ConfigError {
    /// Failed to parse environment variable.
    Parse {
        key: String,
        value: String,
        error: String,
    },
    /// Missing required environment variable.
    Missing { key: String },
    /// Invalid value for environment variable.
    Invalid { key: String, message: String },
    /// IO error (e.g., reading TLS certificates).
    Io { path: String, error: std::io::Error },
}
```

### Testing Configuration

Run configuration tests:

```bash
cargo test config::

# Run specific test
cargo test config::tests::test_config_defaults
```

### Environment Variable Precedence

1. Environment variables take highest priority
2. `.env` file (if using docker-compose)
3. Default values in code

```bash
# Override for single command
PHP_WORKERS=4 cargo run

# Export for session
export PHP_WORKERS=4
cargo run

# Use .env file with docker-compose
echo "PHP_WORKERS=4" >> .env
docker compose up -d
```
