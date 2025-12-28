# tokio_php Documentation

Async PHP web server written in Rust using Tokio runtime and php-embed SAPI.

## Features

| Feature | Description                                                |
|---------|------------------------------------------------------------|
| [Architecture](architecture.md) | System design, components, request flow                    |
| [HTTP/2 & TLS](http2-tls.md) | HTTP/1.1, HTTP/2, HTTPS with TLS 1.3                       |
| [Superglobals](superglobals.md) | `$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`, `$_FILES`, `$_REQUEST` |
| [OPcache & JIT](opcache-jit.md) | Bytecode caching and JIT compilation                       |
| [Worker Pool](worker-pool.md) | Multi-threaded PHP execution, scaling                      |
| [Profiler](profiler.md) | Request timing and performance analysis                    |
| [Compression](compression.md) | Brotli compression for responses                           |
| [Single Entry Point](single-entry-point.md) | Laravel/Symfony routing mode                               |
| [Metrics](metrics.md) | Prometheus metrics for monitoring                          |
| [Health Checks](health-checks.md) | Docker and Kubernetes probes                               |
| [Rate Limiting](rate-limiting.md) | Per-IP request throttling                                  |
| [Error Pages](error-pages.md) | Custom HTML error pages for 4xx/5xx                        |
| [Graceful Shutdown](graceful-shutdown.md) | Zero-downtime deployments with connection draining         |
| [Configuration](configuration.md) | Environment variables reference                            |
| [tokio_sapi Extension](tokio-sapi-extension.md) | PHP extension for FFI superglobals                         |

## Quick Start

```bash
# Build and run
docker compose build
docker compose up -d

# Test
curl http://localhost:8080/index.php

# View logs
docker compose logs -f

# Stop
docker compose down
```

## Performance

Benchmark results on 14-core CPU:

| Configuration | Requests/sec | Latency |
|---------------|--------------|---------|
| No OPcache | ~12,400 | 8.27ms |
| OPcache | ~22,760 | 5.40ms |
| OPcache + JIT | ~23,650 | 4.46ms |
| Production tuned | ~40,000+ | ~2.5ms |

## Requirements

- Docker and Docker Compose
- PHP 8.4 ZTS (Thread Safe) - included in Docker image
- Rust 1.75+ (for development only)

## Project Structure

```
tokio_php/
├── src/
│   ├── main.rs           # Entry point
│   ├── server/           # HTTP server (Hyper)
│   │   ├── mod.rs        # Server initialization
│   │   ├── connection.rs # Connection handling
│   │   ├── internal.rs   # Health/metrics server
│   │   ├── request/      # Request parsing
│   │   ├── response/     # Response building
│   │   └── routing.rs    # URL routing
│   ├── executor/         # PHP execution backends
│   │   ├── mod.rs        # ScriptExecutor trait
│   │   ├── php.rs        # Main PHP executor
│   │   ├── ext.rs        # FFI-based executor
│   │   ├── stub.rs       # Benchmark stub
│   │   ├── common.rs     # Worker pool
│   │   └── sapi.rs       # SAPI initialization
│   ├── types.rs          # Request/Response types
│   └── profiler.rs       # Timing profiler
├── ext/                  # tokio_sapi PHP extension
├── www/                  # Document root
│   └── errors/           # Custom error pages
├── certs/                # TLS certificates
├── Dockerfile            # Multi-stage build
└── docker-compose.yml    # Service definitions
```

## License

proprietary
