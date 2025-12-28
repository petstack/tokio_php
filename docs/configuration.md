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
| `ACCESS_LOG` | `0` | Enable access logs (target: `access`) |
| `RATE_LIMIT` | `0` | Max requests per IP per window (0 = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window in seconds |
| `USE_STUB` | `0` | Stub mode - disable PHP, return empty responses |
| `USE_EXT` | `0` | **Recommended.** Use ExtExecutor (2x faster) |
| `PROFILE` | `0` | Enable request profiling |
| `TLS_CERT` | _(empty)_ | Path to TLS certificate (PEM) |
| `TLS_KEY` | _(empty)_ | Path to TLS private key (PEM) |
| `RUST_LOG` | `tokio_php=info` | Log level |

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
{"ts":"2025-01-15T10:30:00.123Z","level":"info","type":"access","msg":"GET /api/users 200","ctx":{"service":"tokio_php","request_id":"65bdbab40000"},"data":{"method":"GET","path":"/api/users","status":200,"bytes":1234,"duration_ms":5.25,"ip":"10.0.0.1"}}
```

**Context fields (`ctx`):**

| Field | Type | Description |
|-------|------|-------------|
| `service` | string | Service name (`tokio_php`) |
| `request_id` | string | Unique request ID for tracing |

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

### USE_STUB

Enable stub mode for benchmarking without PHP.

```bash
# Normal mode (default)
USE_STUB=0

# Stub mode
USE_STUB=1
```

Stub mode returns empty 200 responses without executing PHP. Useful for measuring HTTP overhead.

### USE_EXT

**Recommended for production.** Use ExtExecutor with FFI-based superglobals.

```bash
# PhpExecutor - eval-based superglobals (default)
USE_EXT=0

# ExtExecutor - FFI superglobals + php_execute_script() (recommended)
USE_EXT=1
```

ExtExecutor is **2x faster** than PhpExecutor:
- Uses native `php_execute_script()` - fully optimized for OPcache/JIT
- Sets superglobals via direct FFI calls (no eval parsing)
- ~34K RPS vs ~16K RPS for index.php

### PROFILE

Enable request profiling.

```bash
# Disabled (default)
PROFILE=0

# Enabled
PROFILE=1
```

When enabled, requests with `X-Profile: 1` header return timing information.

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
docker compose up -d
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
PROFILE=1 \
PHP_WORKERS=2 \
docker compose up -d
```

### Benchmark Mode

```bash
USE_STUB=1 docker compose up -d
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
ACCESS_LOG=1 \
PROFILE=0 \
USE_EXT=1 \
RUST_LOG=tokio_php=info \
docker compose up -d
```

## docker-compose.yml

```yaml
version: '3.8'

services:
  app:
    build: .
    ports:
      - "8080:8080"
      - "9090:9090"
    environment:
      - LISTEN_ADDR=0.0.0.0:8080
      - PHP_WORKERS=${PHP_WORKERS:-0}
      - QUEUE_CAPACITY=${QUEUE_CAPACITY:-0}
      - DOCUMENT_ROOT=/var/www/html
      - INDEX_FILE=${INDEX_FILE:-}
      - INTERNAL_ADDR=0.0.0.0:9090
      - ERROR_PAGES_DIR=${ERROR_PAGES_DIR:-}
      - PROFILE=${PROFILE:-0}
      - USE_EXT=${USE_EXT:-1}  # Recommended: 2x faster
      - USE_STUB=${USE_STUB:-0}
      - ACCESS_LOG=${ACCESS_LOG:-0}
      - RUST_LOG=${RUST_LOG:-tokio_php=info}
    volumes:
      - ./www:/var/www/html:ro
```

## Validation

### Check Current Configuration

```bash
# View environment
docker compose exec app env | grep -E '^(PHP_|QUEUE_|DOCUMENT_|INDEX_|INTERNAL_|USE_|PROFILE|TLS_|RUST_|LISTEN_)'

# View startup logs
docker compose logs app | head -20
```

### Expected Startup Output

```
[INFO] tokio_php v0.1.0
[INFO] Listen address: 0.0.0.0:8080
[INFO] Document root: /var/www/html
[INFO] PHP workers: 8
[INFO] Queue capacity: 800
[INFO] Internal server: 0.0.0.0:9090
[INFO] OPcache: enabled
[INFO] JIT: tracing
[INFO] Server started
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

Enable profiling to identify bottlenecks:
```bash
PROFILE=1 docker compose up -d
curl -H "X-Profile: 1" http://localhost:8080/index.php
```
