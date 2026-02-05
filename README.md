# tokio_php

> **Beta** — This project is experimental. The concept is being tested and validated. API and features may change. Not recommended for production use.
>
> Try it: [Docker Hub](https://hub.docker.com/r/diolektor/tokio_php) | Feedback: [GitHub Issues](https://github.com/petstack/tokio_php/issues/new)

Async PHP web server in Rust. Tokio + php-embed SAPI. HTTP/1.1, HTTP/2, HTTPS, worker pools, OPcache/JIT, Brotli compression.

## Features

- **PHP 8.5 & 8.4** — Support for latest PHP versions (ZTS)
- **High Performance** — Async I/O via Tokio runtime, zero-copy architecture
- **HTTP/1.1 & HTTP/2** — Full protocol support with automatic negotiation
- **HTTPS/TLS 1.3** — Secure connections with ALPN for HTTP/2
- **All HTTP Methods** — GET, POST, PUT, PATCH, DELETE, OPTIONS, HEAD, QUERY
- **Worker Pool** — Multi-threaded PHP execution with configurable workers
- **OPcache + JIT** — Bytecode caching and tracing JIT compilation
- **Brotli Compression** — Automatic compression for text responses
- **Static File Serving** — Efficient caching with configurable TTL
- **Rate Limiting** — Per-IP request throttling with fixed window
- **Distributed Tracing** — W3C Trace Context propagation
- **Graceful Shutdown** — Connection draining for zero-downtime deployments

## Quick Start

```bash
# Run from Docker Hub
docker run -d -p 8080:8080 -v ./www:/var/www/html diolektor/tokio_php

# Or with specific PHP version
docker run -d -p 8080:8080 -v ./www:/var/www/html diolektor/tokio_php:php8.5

# Test
curl http://localhost:8080/
```

## Build from Source

```bash
# Clone repository
git clone https://github.com/petstack/tokio_php.git
cd tokio_php

# Build and run (PHP 8.4 default)
docker compose build
docker compose up -d

# Build with PHP 8.5
PHP_VERSION=8.5 docker compose build
PHP_VERSION=8.5 docker compose up -d
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PHP_VERSION` | `8.4` | PHP version (8.4 or 8.5) |
| `PHP_WORKERS` | `0` | Worker count (0 = auto-detect CPU cores) |
| `QUEUE_CAPACITY` | `0` | Max pending requests (0 = workers × 100) |
| `LISTEN_ADDR` | `0.0.0.0:8080` | Server bind address |
| `DOCUMENT_ROOT` | `/var/www/html` | Web root directory |
| `INDEX_FILE` | — | Routing mode: `index.php` (framework), `index.html` (SPA), empty (traditional) |
| `EXECUTOR` | `ext` | Script executor: `ext` (recommended), `php` (legacy), `stub` (benchmark) |
| `TLS_CERT` | — | Path to TLS certificate (PEM) |
| `TLS_KEY` | — | Path to TLS private key (PEM) |
| `STATIC_CACHE_TTL` | `1d` | Static file cache duration |
| `ERROR_PAGES_DIR` | — | Custom HTML error pages directory |
| `DRAIN_TIMEOUT_SECS` | `30` | Graceful shutdown timeout |
| `REQUEST_TIMEOUT` | `2m` | Request timeout (30s, 2m, 5m, off) |
| `SSE_TIMEOUT` | `30m` | SSE/streaming connection timeout (30s, 2m, 1h, off) |
| `HEADER_TIMEOUT_SECS` | `5` | Header read timeout (Slowloris protection) |
| `IDLE_TIMEOUT_SECS` | `60` | Idle connection timeout |
| `INTERNAL_ADDR` | — | Internal server for /health, /metrics |
| `ACCESS_LOG` | `0` | Enable access logs (0 = disabled) |
| `LOG_LEVEL` | `info` | Log level: trace, debug, info, warn, error |
| `RATE_LIMIT` | `0` | Max requests per IP (0 = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window (seconds) |

## Examples

```bash
# Production with tuning
PHP_WORKERS=8 docker compose up -d

# Laravel/Symfony (framework mode - all .php blocked, routes through index.php)
INDEX_FILE=index.php DOCUMENT_ROOT=/var/www/html/public docker compose up -d

# React/Vue SPA (all non-existent paths serve index.html, PHP still works)
INDEX_FILE=index.html docker compose up -d

# With TLS/HTTPS
docker compose --profile tls up -d

# Benchmark mode (no PHP)
EXECUTOR=stub docker compose up -d

# Build with profiling (single-worker, writes reports to /tmp/)
CARGO_FEATURES=debug-profile docker compose build
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Tokio Runtime                        │
├─────────────────────────────────────────────────────────┤
│  Hyper HTTP Server (HTTP/1.1, HTTP/2, TLS)              │
├─────────────────────────────────────────────────────────┤
│  Request Router                                         │
│  ├── Static Files (Brotli, Cache-Control)               │
│  └── PHP Scripts                                        │
├─────────────────────────────────────────────────────────┤
│  Worker Pool (N threads)                                │
│  ├── Worker 0: php-embed SAPI + OPcache                 │
│  ├── Worker 1: php-embed SAPI + OPcache                 │
│  └── Worker N: php-embed SAPI + OPcache                 │
└─────────────────────────────────────────────────────────┘
```

## Superglobals

Full PHP superglobals support:

- `$_GET` — Query parameters
- `$_POST` — POST data (form-urlencoded, JSON)
- `$_SERVER` — Server variables, headers
- `$_COOKIE` — Cookies
- `$_FILES` — File uploads (multipart/form-data)
- `$_REQUEST` — Merged GET + POST + COOKIE

## Extension Functions

When using `EXECUTOR=ext` (default), additional PHP functions are available:

```php
tokio_request_id();            // int - unique request ID
tokio_worker_id();             // int - worker thread ID (0..N-1)
tokio_server_info();           // array - server info including 'build' with git hash
tokio_request_heartbeat(30);   // bool - extend request timeout by N seconds
tokio_finish_request();        // bool - send response immediately, continue in background

$_SERVER['TOKIO_SERVER_BUILD_VERSION']; // "0.1.0 (abc12345)"
```

## Profiling

Enable profiling at compile time with `debug-profile` feature:

```bash
# Build with profiling
CARGO_FEATURES=debug-profile docker compose build
docker compose up -d

# Make a request
curl http://localhost:8080/index.php

# View profile report
docker compose exec tokio_php cat /tmp/tokio_profile_request_*.md
```

When built with profiling:
- Server runs in **single-worker mode** (accurate timing)
- All requests generate detailed markdown reports to `/tmp/`
- Reports include tree-structured timing breakdown

See [docs/profiling.md](docs/profiling.md) for full documentation.

## Compression

Automatic Brotli compression when:
- Client sends `Accept-Encoding: br`
- Response body >= 256 bytes and <= 3 MB
- Content-Type is compressible (text/html, application/json, etc.)

## Internal Server

Health checks and metrics endpoint:

```bash
INTERNAL_ADDR=0.0.0.0:9090 docker compose up -d

curl http://localhost:9090/health
curl http://localhost:9090/metrics
```

## Benchmark

```bash
# Install wrk
brew install wrk  # macOS

# Run benchmark
wrk -t4 -c100 -d10s http://localhost:8080/index.php
```

## Requirements

- Docker & Docker Compose
- Or: Rust 1.70+, PHP 8.5/8.4 ZTS with php-embed

## Docker Tags

| Tag | Description |
|-----|-------------|
| `latest` | PHP 8.5 on Alpine (default, multi-arch) |
| `php8.5`, `php8.5-alpine3.23` | PHP 8.5 on Alpine 3.23 |
| `php8.4`, `php8.4-alpine3.23` | PHP 8.4 on Alpine 3.23 |

For binary extraction, use `--target dist` with `Dockerfile.release` or `Dockerfile.debian`.

## Links

- [Docker Hub](https://hub.docker.com/r/diolektor/tokio_php)
- [GitHub](https://github.com/petstack/tokio_php)
- [Documentation](docs/)

## License

[AGPL-3.0](LICENSE)
