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
- `src/server.rs` - Hyper-based async HTTP server, routes requests to PHP or static files
- `src/php.rs` - PHP embed FFI bindings and execution wrapper with dedicated PHP thread
- `build.rs` - Links against libphp
- `www/` - Document root for PHP files (mounted as volume in Docker)

## Key Design Decisions

- Dedicated PHP executor thread with 16MB stack (PHP 8.4 requires larger stack for zend.max_allowed_stack_size)
- Channel-based communication between async Tokio tasks and PHP thread
- Output captured by redirecting stdout via pipe during PHP execution
- Static files served directly via Tokio async I/O
- PHP files executed through embed SAPI in dedicated thread

## Docker Environment

Uses Alpine 3.21 with php84-embed. Multi-stage build:
1. Builder stage: Rust + PHP dev headers + all build dependencies
2. Runtime stage: Minimal Alpine with php84-embed only (~24MB)

## Limitations

- `$_GET`, `$_POST`, `$_SERVER` superglobals not populated (embed SAPI limitation)
- Single-threaded PHP execution (PHP is not thread-safe)
