# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

tokio_php is an async web server written in Rust that executes PHP scripts via php-embed SAPI. It uses Tokio for async I/O and Hyper for HTTP handling.

## Build Commands

```bash
# Build with Docker (recommended)
docker compose build
docker compose up

# Run in background
docker compose up -d

# View logs
docker compose logs -f

# Stop
docker compose down
```

## Architecture

- `src/main.rs` - Entry point, initializes PHP runtime and starts HTTP server
- `src/server.rs` - Hyper-based async HTTP server, parses GET/POST, routes requests
- `src/php.rs` - PHP embed FFI bindings, superglobals injection, dedicated executor thread
- `build.rs` - Links against libphp
- `www/` - Document root for PHP files (mounted as volume in Docker)

## Key Design Decisions

- Uses PHP 8.4 ZTS (Thread Safe) build with php-embed SAPI
- OPcache + JIT enabled by overriding SAPI name to "cli-server" before php_embed_init
  - OPcache: ~2x performance boost for I/O workloads
  - JIT (tracing mode): up to 4x for CPU-intensive code (Fibonacci, math, etc.)
- Multi-threaded PHP worker pool (configurable via `PHP_WORKERS` env var, defaults to CPU count)
- Single-threaded Tokio runtime (PHP workers handle blocking work)
- Channel-based communication between async Tokio tasks and PHP worker threads
- Output captured by redirecting stdout to temp files during PHP execution
- Superglobals (`$_GET`, `$_POST`, `$_SERVER`, `$_REQUEST`, `$_COOKIE`, `$_FILES`) injected via `zend_eval_string` before script execution
- Static files served directly via Tokio async I/O

## Superglobals Support

The server parses HTTP requests and injects PHP superglobals:
- `$_GET` - Query string parameters (URL decoded)
- `$_POST` - Form data (application/x-www-form-urlencoded and multipart/form-data)
- `$_SERVER` - REQUEST_METHOD, REQUEST_URI, QUERY_STRING, REMOTE_ADDR, HTTP headers, etc.
- `$_COOKIE` - Parsed from Cookie header
- `$_FILES` - Uploaded files from multipart/form-data
- `$_REQUEST` - Merged GET + POST

## Docker Environment

Uses `php:8.4-zts-alpine` (official PHP ZTS image). Multi-stage build:
1. Builder stage: Rust + PHP dev headers + build dependencies
2. Runtime stage: Minimal Alpine with PHP ZTS + libgcc

## Limitations

- No `$_SESSION` support (requires session handler implementation)
