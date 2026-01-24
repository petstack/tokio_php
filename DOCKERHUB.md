# tokio_php

> **Beta** — This project is experimental. The concept is being tested and validated. API and features may change. Not recommended for production use.
>
> Feedback welcome: [GitHub Issues](https://github.com/petstack/tokio_php/issues/new)

High-performance async PHP server written in Rust. Uses Tokio for async I/O and php-embed SAPI for PHP execution.

**Supported Architectures:** `linux/amd64`, `linux/arm64`

## Features

- **HTTP/1.1 & HTTP/2** - Full protocol support with automatic detection
- **HTTPS/TLS 1.3** - Secure connections with ALPN negotiation
- **All HTTP Methods** - GET, POST, PUT, PATCH, DELETE, OPTIONS, HEAD, QUERY
- **Worker Pool** - Multi-threaded PHP execution with configurable workers
- **OPcache & JIT** - PHP 8.4/8.5 with tracing JIT enabled
- **Brotli Compression** - Automatic response compression
- **Rate Limiting** - Per-IP request throttling
- **Distributed Tracing** - W3C Trace Context support
- **Graceful Shutdown** - Connection draining for zero-downtime deployments
- **Custom Error Pages** - HTML error pages for 4xx/5xx responses
- **Prometheus Metrics** - Built-in `/health` and `/metrics` endpoints
- **Background Processing** - `tokio_finish_request()` for async tasks

## Quick Start

```bash
# Pull the image
docker pull diolektor/tokio_php:php8.5-alpine3.23

# Run with default settings
docker run -d -p 8080:8080 -v $(pwd)/www:/var/www/html diolektor/tokio_php:php8.5-alpine3.23

# Run with custom configuration
docker run -d -p 8080:8080 \
  -e PHP_WORKERS=8 \
  -e INDEX_FILE=index.php \
  -v $(pwd)/www:/var/www/html \
  diolektor/tokio_php:php8.5-alpine3.23
```

## Available Tags

All tags are multi-arch (`amd64` + `arm64`).

| Tag | PHP | Alpine |
|-----|-----|--------|
| `latest` | 8.5 | 3.23 |
| `php8.5` | 8.5 | 3.23 |
| `php8.4` | 8.4 | 3.23 |
| `php8.5-alpine3.23` | 8.5 | 3.23 |
| `php8.4-alpine3.23` | 8.4 | 3.23 |

## Environment Variables

### Server Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | Server bind address and port |
| `DOCUMENT_ROOT` | `/var/www/html` | Web root directory |
| `INDEX_FILE` | _(empty)_ | Single entry point file (e.g., `index.php` for Laravel/Symfony) |
| `INTERNAL_ADDR` | _(empty)_ | Internal server address for `/health` and `/metrics` (e.g., `0.0.0.0:9090`) |

### Worker Pool

| Variable | Default | Description |
|----------|---------|-------------|
| `PHP_WORKERS` | `0` | Number of PHP worker threads. `0` = auto-detect (CPU cores) |
| `QUEUE_CAPACITY` | `0` | Max pending requests in queue. `0` = workers × 100 |

### TLS/HTTPS

| Variable | Default | Description |
|----------|---------|-------------|
| `TLS_CERT` | _(empty)_ | Path to TLS certificate file (PEM format) |
| `TLS_KEY` | _(empty)_ | Path to TLS private key file (PEM format) |

### Features

| Variable | Default | Description |
|----------|---------|-------------|
| `ERROR_PAGES_DIR` | _(empty)_ | Directory with custom HTML error pages (e.g., `404.html`, `500.html`) |
| `STATIC_CACHE_TTL` | `1d` | Cache-Control max-age for static files. Values: `off`, `30s`, `2m`, `1h`, `1d`, `1w`, `1y` |
| `REQUEST_TIMEOUT` | `2m` | Request timeout. Values: `30s`, `2m`, `5m`, `off`. Returns 504 on timeout |
| `DRAIN_TIMEOUT_SECS` | `30` | Graceful shutdown timeout in seconds |

### Rate Limiting

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT` | `0` | Max requests per IP per window. `0` = disabled |
| `RATE_WINDOW` | `60` | Rate limit window in seconds |

### Logging & Debugging

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `tokio_php=info` | Log level: `error`, `warn`, `info`, `debug`, `trace` |
| `ACCESS_LOG` | `0` | Enable access logging. `1` = enabled |

**Profiling:** Build from source with `CARGO_FEATURES=debug-profile` for detailed timing reports. See [GitHub](https://github.com/petstack/tokio_php/blob/master/docs/profiling.md).

### Executor Mode

| Variable | Default | Description |
|----------|---------|-------------|
| `USE_STUB` | `0` | Stub mode (no PHP execution, for benchmarks). `1` = enabled |
| `USE_EXT` | `1` | Use FFI extension for superglobals (recommended, 2x faster). `0` = legacy mode |

## Usage Examples

### Laravel / Symfony

```bash
docker run -d -p 8080:8080 \
  -e INDEX_FILE=index.php \
  -e DOCUMENT_ROOT=/var/www/html/public \
  -e PHP_WORKERS=1 \
  -v $(pwd):/var/www/html \
  diolektor/tokio_php:php8.5-alpine3.23
```

**Note:** Use `PHP_WORKERS=1` in development mode to avoid thread-safety issues with framework cache compilation.

### Production with Metrics

```bash
docker run -d -p 8080:8080 -p 9090:9090 \
  -e PHP_WORKERS=16 \
  -e QUEUE_CAPACITY=1000 \
  -e INTERNAL_ADDR=0.0.0.0:9090 \
  -e ACCESS_LOG=1 \
  -e RATE_LIMIT=100 \
  -e DRAIN_TIMEOUT_SECS=60 \
  -v $(pwd)/www:/var/www/html:ro \
  diolektor/tokio_php:php8.5-alpine3.23
```

### With HTTPS

```bash
docker run -d -p 8443:8443 \
  -e LISTEN_ADDR=0.0.0.0:8443 \
  -e TLS_CERT=/certs/cert.pem \
  -e TLS_KEY=/certs/key.pem \
  -v $(pwd)/www:/var/www/html \
  -v $(pwd)/certs:/certs:ro \
  diolektor/tokio_php:php8.5-alpine3.23
```

### With Rate Limiting

```bash
docker run -d -p 8080:8080 \
  -e RATE_LIMIT=100 \
  -e RATE_WINDOW=60 \
  -v $(pwd)/www:/var/www/html \
  diolektor/tokio_php:php8.5-alpine3.23
```

### Custom Error Pages

```bash
docker run -d -p 8080:8080 \
  -e ERROR_PAGES_DIR=/var/www/html/errors \
  -v $(pwd)/www:/var/www/html \
  diolektor/tokio_php:php8.5-alpine3.23
```

Create error pages: `errors/404.html`, `errors/500.html`, `errors/503.html`

## Health Check & Metrics

Enable internal server with `INTERNAL_ADDR`:

```bash
# Health check
curl http://localhost:9090/health
{"status":"ok","timestamp":1703361234,"active_connections":5,"total_requests":1000}

# Prometheus metrics
curl http://localhost:9090/metrics

# Server configuration
curl http://localhost:9090/config
```

### Available Metrics

- `tokio_php_uptime_seconds` - Server uptime
- `tokio_php_requests_per_second` - Average RPS
- `tokio_php_response_time_avg_seconds` - Average response time
- `tokio_php_active_connections` - Current connections
- `tokio_php_pending_requests` - Queue size
- `tokio_php_dropped_requests` - Requests dropped (queue full)
- `tokio_php_requests_total{method}` - Requests by HTTP method
- `tokio_php_responses_total{status}` - Responses by status code
- `node_load1`, `node_load5`, `node_load15` - System load averages
- `node_memory_*` - Memory statistics
- `tokio_php_memory_usage_percent` - Memory usage percentage

## Profiling

Enable profiling and send requests with `X-Profile: 1` header:

```bash
docker run -d -p 8080:8080 -e PROFILE=1 ...

curl -H "X-Profile: 1" http://localhost:8080/index.php -I
```

Response headers include timing data:
- `X-Profile-Total-Us` - Total request time (microseconds)
- `X-Profile-Queue-Us` - Worker queue wait time
- `X-Profile-PHP-Startup-Us` - PHP request startup time
- `X-Profile-Script-Us` - PHP script execution time
- `X-Profile-PHP-Shutdown-Us` - PHP request shutdown time
- `X-Profile-TLS-Handshake-Us` - TLS handshake time (HTTPS only)
- `X-Profile-TLS-Protocol` - TLS version (TLSv1_2, TLSv1_3)

## Kubernetes

```yaml
apiVersion: v1
kind: Pod
spec:
  terminationGracePeriodSeconds: 35
  containers:
    - name: tokio-php
      image: diolektor/tokio_php:php8.5-alpine3.23
      ports:
        - containerPort: 8080
        - containerPort: 9090
      env:
        - name: PHP_WORKERS
          value: "8"
        - name: INTERNAL_ADDR
          value: "0.0.0.0:9090"
        - name: DRAIN_TIMEOUT_SECS
          value: "30"
      livenessProbe:
        httpGet:
          path: /health
          port: 9090
      readinessProbe:
        httpGet:
          path: /health
          port: 9090
      lifecycle:
        preStop:
          exec:
            command: ["sleep", "5"]
```

## Docker Compose

```yaml
services:
  app:
    image: diolektor/tokio_php:php8.5-alpine3.23
    ports:
      - "8080:8080"
      - "9090:9090"
    environment:
      PHP_WORKERS: 8
      INDEX_FILE: index.php
      DOCUMENT_ROOT: /var/www/html/public
      INTERNAL_ADDR: 0.0.0.0:9090
      ACCESS_LOG: 1
    volumes:
      - ./:/var/www/html:ro
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:9090/health"]
      interval: 10s
      timeout: 5s
      retries: 3
```

## PHP Functions

tokio_php provides special PHP functions via the `tokio_sapi` extension:

### tokio_request_id(): string

Returns the unique request ID for distributed tracing:

```php
<?php
$requestId = tokio_request_id();
// "65bdbab40000"
```

### tokio_worker_id(): int

Returns the current worker thread ID:

```php
<?php
$workerId = tokio_worker_id();
// 0, 1, 2, ...
```

### tokio_server_info(): array

Returns server information:

```php
<?php
$info = tokio_server_info();
// ['name' => 'tokio_php', 'version' => '0.1.0', 'build' => 'abc1234', 'php_sapi' => 'embed']
```

### tokio_request_heartbeat(int $time = 10): bool

Extends the request timeout deadline for long-running scripts:

```php
<?php
foreach ($large_dataset as $item) {
    process_item($item);
    set_time_limit(30);
    tokio_request_heartbeat(30);  // Extend by 30 seconds
}
```

### tokio_finish_request(): bool

Sends response immediately and continues script execution in background (like `fastcgi_finish_request()`):

```php
<?php
echo "Response sent to user";
tokio_finish_request();  // Client gets response NOW

// Background work (client doesn't wait):
send_email($user);
log_to_database($data);
```

## Supported PHP Features

- Full superglobals: `$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`, `$_FILES`, `$_REQUEST`
- OPcache with JIT (tracing mode)
- Preloading support via `opcache.preload`
- All standard PHP extensions
- W3C Trace Context: `$_SERVER['TRACE_ID']`, `$_SERVER['SPAN_ID']`
- Build version: `$_SERVER['TOKIO_SERVER_BUILD_VERSION']`

## Image Contents

The image includes:

| Component | Path |
|-----------|------|
| Server binary | `/usr/local/bin/tokio_php` |
| Bridge library | `/usr/local/lib/libtokio_bridge.so` |
| PHP extension | `/usr/local/lib/php/extensions/no-debug-zts-{API}/tokio_sapi.so` |

PHP extension directory varies by version:
- PHP 8.4: `no-debug-zts-20240924`
- PHP 8.5: `no-debug-zts-20250925`

## Security

- Runs as non-root user (`www-data`, UID 82)
- Read-only volumes supported
- TLS 1.3 with strong cipher suites
- Rate limiting for abuse prevention

## Links

- **GitHub**: [https://github.com/petstack/tokio_php](https://github.com/petstack/tokio_php)
- **Documentation**: [https://github.com/petstack/tokio_php/tree/master/docs](https://github.com/petstack/tokio_php/tree/master/docs)
- **Issues**: [https://github.com/petstack/tokio_php/issues](https://github.com/petstack/tokio_php/issues)

## License

AGPL-3.0
