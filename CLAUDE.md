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

- Dedicated PHP executor thread with 16MB stack (PHP 8.4 requires larger stack)
- Channel-based communication between async Tokio tasks and PHP thread
- Output captured by redirecting stdout via pipe during PHP execution
- Superglobals (`$_GET`, `$_POST`, `$_SERVER`, `$_REQUEST`) injected via `zend_eval_string` before script execution
- Static files served directly via Tokio async I/O

## Superglobals Support

The server parses HTTP requests and injects PHP superglobals:
- `$_GET` - Query string parameters (URL decoded)
- `$_POST` - Form data (application/x-www-form-urlencoded)
- `$_SERVER` - REQUEST_METHOD, REQUEST_URI, QUERY_STRING, REMOTE_ADDR, etc.
- `$_REQUEST` - Merged GET + POST

## Docker Environment

Uses Alpine 3.21 with php84-embed. Multi-stage build:
1. Builder stage: Rust + PHP dev headers + all build dependencies
2. Runtime stage: Minimal Alpine with php84-embed only (~24MB)

## Limitations

- Single-threaded PHP execution (PHP is not thread-safe)
- No `$_FILES` support (multipart/form-data not implemented)
- No `$_COOKIE` / `$_SESSION` support yet
