# tokio_php Documentation

> **Beta** — This project is experimental. The concept is being tested and validated. API and features may change. Not recommended for production use.
>
> Try it: [Docker Hub](https://hub.docker.com/r/diolektor/tokio_php) | Feedback: [GitHub Issues](https://github.com/petstack/tokio_php/issues/new)

Async PHP web server in Rust. Tokio + php-embed SAPI. HTTP/1.1, HTTP/2, HTTPS, worker pools, OPcache/JIT, Brotli compression.

**Supported PHP versions:** 8.4, 8.5 (ZTS)

## Features

| Feature | Description                                                |
|---------|------------------------------------------------------------|
| [Architecture](architecture.md) | System design, components, request flow                    |
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
| [Health Checks](health-checks.md) | Docker and Kubernetes probes                               |
| [Rate Limiting](rate-limiting.md) | Per-IP request throttling                                  |
| [Request Heartbeat](request-heartbeat.md) | Extend timeout for long-running scripts                    |
| [Error Pages](error-pages.md) | Custom HTML error pages for 4xx/5xx                        |
| [Graceful Shutdown](graceful-shutdown.md) | Zero-downtime deployments with connection draining         |
| [Security](security.md) | Non-root execution, best practices                         |
| [Configuration](configuration.md) | Environment variables reference                            |
| [Distributed Tracing](distributed-tracing.md) | W3C Trace Context for request correlation                  |
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
# Build and run (PHP 8.5 default)
docker compose build
docker compose up -d

# Build with PHP 8.4
PHP_VERSION=8.4 docker compose build
PHP_VERSION=8.4 docker compose up -d

# With TLS/HTTPS
docker compose --profile tls up -d

# View logs
docker compose logs -f
```

## Performance

### tokio_php vs nginx + PHP-FPM

| Server | RPS (bench.php) | RPS (index.php) | Latency |
|--------|-----------------|-----------------|---------|
| **tokio_php (ExtExecutor)** | **35,350** | **32,913** | 2.8ms |
| nginx + PHP-FPM | 13,890 | 12,471 | 7.2ms |

**tokio_php is 2.5x faster** — no FastCGI overhead, threads instead of processes.

### Executor Comparison

| Executor | Method | RPS (bench.php) |
|----------|--------|-----------------|
| **ExtExecutor** | `php_execute_script()` | **37,911** |
| PhpExecutor | `zend_eval_string()` | 19,555 |

Use `USE_EXT=1` for production (2x faster).

### tokio_php vs FrankenPHP

| Server | RPS (bench.php) | RPS (index.php) | Latency |
|--------|-----------------|-----------------|---------|
| **tokio_php** | **32,600** | **30,250** | 3.1ms |
| FrankenPHP | 18,350 | 17,530 | 5.5ms |

**tokio_php is 1.8x faster** — zero-cost Rust FFI vs Go CGO overhead.

### OPcache Impact

| Configuration | Requests/sec | Latency |
|---------------|--------------|---------|
| No OPcache | ~12,400 | 8.27ms |
| OPcache | ~22,760 | 5.40ms |
| OPcache + JIT | ~23,650 | 4.46ms |
| ExtExecutor + JIT | ~35,000+ | ~2.8ms |

## Requirements

- Docker and Docker Compose
- PHP 8.4 or 8.5 ZTS (Thread Safe) — included in Docker image
- Rust 1.70+ (for development only)

## Project Structure

```
tokio_php/
├── src/
│   ├── main.rs              # Entry point, runtime init
│   ├── server/              # HTTP server (Hyper)
│   │   ├── mod.rs           # Server core
│   │   ├── builder.rs       # Server builder pattern
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
│   │   ├── sapi.rs          # SAPI initialization
│   │   └── pool/            # Generic thread pool
│   ├── core/                # Core types and context
│   ├── config/              # Configuration parsing
│   ├── logging.rs           # Structured JSON logging
│   ├── trace_context.rs     # W3C Trace Context
│   ├── profiler.rs          # Request timing
│   └── types.rs             # Request/Response types
├── ext/                     # tokio_sapi PHP extension
├── docs/                    # Documentation
├── www/                     # Document root
│   └── errors/              # Custom error pages
├── certs/                   # TLS certificates
├── Dockerfile               # Multi-stage build
├── docker-compose.yml       # Service definitions
└── LICENSE                  # AGPL-3.0
```

## Links

- [Docker Hub](https://hub.docker.com/r/diolektor/tokio_php)
- [GitHub](https://github.com/petstack/tokio_php)

## License

[AGPL-3.0](../LICENSE)
