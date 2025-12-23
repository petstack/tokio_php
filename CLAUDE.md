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
USE_SAPI=1 docker compose up -d          # Alternative SAPI executor
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

- `PhpExecutor` (`php.rs`) - Main PHP executor using php-embed with worker pool
- `PhpSapiExecutor` (`php_sapi.rs`) - Alternative PHP executor with SAPI module init
- `StubExecutor` (`stub.rs`) - Returns empty responses for benchmarking

Selection order in main.rs:
1. `USE_STUB=1` → StubExecutor
2. `USE_SAPI=1` → PhpSapiExecutor
3. Default → PhpExecutor

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

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | Server bind address |
| `PHP_WORKERS` | `0` (auto) | Worker count (0 = CPU cores) |
| `USE_STUB` | `0` | Disable PHP, return empty responses |
| `USE_SAPI` | `0` | Use alternative SAPI executor |
| `PROFILE` | `0` | Enable profiling (requires `X-Profile: 1` header) |
| `TLS_CERT` | - | Path to TLS certificate (PEM) |
| `TLS_KEY` | - | Path to TLS private key (PEM) |
| `INDEX_FILE` | - | Single entry point mode (e.g., `index.php`) |
| `DOCUMENT_ROOT` | `/var/www/html` | Web root directory |
| `RUST_LOG` | `tokio_php=info` | Log level |

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
- Response body >= 256 bytes
- Content-Type is compressible (text/html, text/css, application/json, etc.)

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

## Limitations

- No `$_SESSION` support (requires session handler implementation)
- HTTP/3 (QUIC) not yet implemented (h3 crate is experimental)
