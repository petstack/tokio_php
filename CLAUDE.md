# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

tokio_php is an async web server written in Rust that executes PHP scripts via php-embed SAPI. It uses Tokio for async I/O and Hyper for HTTP handling.

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

# Benchmark
wrk -t4 -c100 -d10s http://localhost:8080/index.php
```

## Architecture

### Core Components

- `src/main.rs` - Entry point, runtime initialization, executor selection based on env vars
- `src/server.rs` - Hyper-based HTTP server, request parsing (GET/POST/multipart), routing
- `src/executor/` - Script execution backends (trait-based, pluggable)
- `src/types.rs` - ScriptRequest/ScriptResponse data structures
- `src/profiler.rs` - Request timing profiler (enabled via `PROFILE=1` + `X-Profile: 1` header)

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
| `RUST_LOG` | `tokio_php=info` | Log level |

## Profiling

With `PROFILE=1`, requests with `X-Profile: 1` header return timing data:
```bash
curl -sI -H "X-Profile: 1" http://localhost:8080/index.php | grep X-Profile
```

Returns headers: `X-Profile-Total-Us`, `X-Profile-PHP-Startup-Us`, `X-Profile-Script-Us`, etc.

## Superglobals Support

Full superglobals: `$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`, `$_FILES`, `$_REQUEST`

## Limitations

- No `$_SESSION` support (requires session handler implementation)
