# tokio_php Documentation

> **Beta** — This project is experimental. The concept is being tested and validated. API and features may change. Not recommended for production use.
>
> Try it: [Docker Hub](https://hub.docker.com/r/diolektor/tokio_php) | Feedback: [GitHub Issues](https://github.com/petstack/tokio_php/issues/new)

Async PHP web server in Rust. Tokio + php-embed SAPI. HTTP/1.1, HTTP/2, HTTPS, worker pools, OPcache/JIT, Brotli compression.

**Supported PHP versions:** 8.5, 8.4 (ZTS)

## Features

| Feature | Description                                                |
|---------|------------------------------------------------------------|
| [PHP Support](php-support.md) | PHP 8.5/8.4, ZTS, embed SAPI, extensions                   |
| [Architecture](architecture.md) | System design, components, request flow                    |
| [Docker](docker.md) | Dockerfiles, compose, build targets, deployment            |
| [HTTP/2 & TLS](http2-tls.md) | HTTP/1.1, HTTP/2, HTTPS with TLS 1.3                       |
| [HTTP Methods](http-methods.md) | GET, POST, PUT, PATCH, DELETE, OPTIONS, QUERY              |
| [Middleware](middleware.md) | Rate limiting, compression, logging, error pages           |
| [Internal Server](internal-server.md) | Health checks, Prometheus metrics, monitoring              |
| [Superglobals](superglobals.md) | `$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`, `$_FILES`, `$_REQUEST` |
| [OPcache & JIT](opcache-jit.md) | Bytecode caching and JIT compilation                       |
| [OPcache Internals](opcache-internals.md) | Deep dive into OPcache architecture                        |
| [Worker Pool](worker-pool.md) | Multi-threaded PHP execution, scaling                      |
| [Profiling](profiling.md) | Request timing and performance analysis                    |
| [Compression](compression.md) | Brotli compression for responses                           |
| [Static Caching](static-caching.md) | Cache-Control, ETag, Last-Modified for static files        |
| [Single Entry Point](single-entry-point.md) | Laravel/Symfony routing mode                               |
| [Framework Compatibility](framework-compatibility.md) | Symfony, Laravel thread-safety guide                       |
| [Health Checks](health-checks.md) | Docker and Kubernetes probes                               |
| [Rate Limiting](rate-limiting.md) | Per-IP request throttling                                  |
| [Request Heartbeat](request-heartbeat.md) | Extend timeout for long-running scripts                    |
| [SSE Streaming](sse-streaming.md) | Server-Sent Events for real-time data streaming            |
| [Error Pages](error-pages.md) | Custom HTML error pages for 4xx/5xx                        |
| [Graceful Shutdown](graceful-shutdown.md) | Zero-downtime deployments with connection draining         |
| [Security](security.md) | Non-root execution, best practices                         |
| [Configuration](configuration.md) | Environment variables reference                            |
| [Distributed Tracing](distributed-tracing.md) | W3C Trace Context for request correlation                  |
| [Logging](logging.md) | JSON logs, Monolog integration, log aggregation            |
| [tokio_sapi Extension](tokio-sapi-extension.md) | PHP extension for FFI superglobals                         |

## Quick Start

```bash
# Run from Docker Hub
docker run -d -p 8080:8080 -v ./www:/var/www/html diolektor/tokio_php

# Or with specific version
docker run -d -p 8080:8080 -v ./www:/var/www/html diolektor/tokio_php:php8.5

# Test
curl http://localhost:8080/
```

### Build from Source

```bash
# Build and run (PHP 8.4 default)
docker compose build
docker compose up -d

# Build with PHP 8.5
PHP_VERSION=8.5 docker compose build
PHP_VERSION=8.5 docker compose up -d

# With TLS/HTTPS
docker compose --profile tls up -d

# View logs
docker compose logs -f
```

## Requirements

- Docker and Docker Compose
- PHP 8.4 or 8.5 ZTS (Thread Safe) — [included in Docker image](php-support.md)
- Rust 1.70+ (for development only)

## Project Structure

```
tokio_php/
├── src/
│   ├── main.rs              # Entry point, runtime init
│   ├── lib.rs               # Library entry point
│   ├── bridge.rs            # Bridge FFI bindings (libtokio_bridge)
│   ├── server/              # HTTP server (Hyper)
│   │   ├── mod.rs           # Server core
│   │   ├── config.rs        # Server configuration
│   │   ├── connection.rs    # Connection handling
│   │   ├── internal.rs      # Health/metrics server
│   │   ├── routing.rs       # URL routing
│   │   ├── request/         # Request parsing
│   │   └── response/        # Response building, compression
│   ├── listener/            # Connection listeners
│   │   ├── tcp.rs           # TCP listener
│   │   └── tls.rs           # TLS listener (rustls)
│   ├── middleware/          # Middleware system
│   │   ├── mod.rs           # Middleware trait
│   │   ├── chain.rs         # Middleware chain
│   │   ├── rate_limit.rs    # Rate limiting
│   │   ├── compression.rs   # Brotli compression
│   │   ├── access_log.rs    # Access logging
│   │   ├── error_pages.rs   # Custom error pages
│   │   └── static_cache.rs  # Static file caching
│   ├── executor/            # PHP execution backends
│   │   ├── mod.rs           # ScriptExecutor trait
│   │   ├── php.rs           # PhpExecutor (zend_eval_string)
│   │   ├── ext.rs           # ExtExecutor (php_execute_script)
│   │   ├── stub.rs          # StubExecutor (benchmarks)
│   │   ├── common.rs        # Shared worker pool
│   │   └── sapi.rs          # SAPI initialization
│   ├── core/                # Core types and context
│   ├── config/              # Configuration parsing
│   ├── logging.rs           # Structured JSON logging
│   ├── trace_context.rs     # W3C Trace Context
│   ├── profiler.rs          # Request timing
│   └── types.rs             # Request/Response types
├── ext/                     # PHP extensions
│   ├── bridge/              # libtokio_bridge.so (Rust ↔ PHP TLS)
│   │   ├── tokio_bridge.h   # Public API header
│   │   ├── tokio_bridge.c   # Thread-local storage implementation
│   │   └── Makefile         # Build rules
│   ├── tokio_sapi.c         # tokio_sapi PHP extension
│   └── tokio_sapi.h         # Extension headers
├── docs/                    # Documentation
├── www/                     # Document root
│   └── errors/              # Custom error pages
├── certs/                   # TLS certificates
├── Dockerfile               # Development build (with tests)
├── Dockerfile.release       # Release build (minimal, Alpine)
├── Dockerfile.debian        # Debian Bookworm build (glibc)
├── docker-compose.yml       # Service definitions
└── LICENSE                  # AGPL-3.0
```

## Links

- [Docker Hub](https://hub.docker.com/r/diolektor/tokio_php)
- [GitHub](https://github.com/petstack/tokio_php)

## License

[AGPL-3.0](../LICENSE)
