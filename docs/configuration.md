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
| `USE_STUB` | `0` | Stub mode - disable PHP, return empty responses |
| `USE_EXT` | `0` | Use ExtExecutor with tokio_sapi extension |
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

Use FFI-based superglobals via tokio_sapi extension.

```bash
# Eval-based superglobals (default)
USE_EXT=0

# FFI-based superglobals
USE_EXT=1
```

FFI mode provides:
- Per-variable timing in profiler
- Slightly slower but more detailed

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

Configure log level and filtering.

```bash
# Default - info level for tokio_php
RUST_LOG=tokio_php=info

# Debug level
RUST_LOG=tokio_php=debug

# Trace level (very verbose)
RUST_LOG=tokio_php=trace

# Warning only
RUST_LOG=tokio_php=warn

# Multiple targets
RUST_LOG=tokio_php=debug,hyper=info
```

Log levels: `trace`, `debug`, `info`, `warn`, `error`

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
PROFILE=0 \
USE_EXT=0 \
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
      - USE_EXT=${USE_EXT:-0}
      - USE_STUB=${USE_STUB:-0}
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
