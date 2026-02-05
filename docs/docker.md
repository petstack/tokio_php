# Docker

This document covers Docker setup for development and production deployments.

## Dockerfiles

tokio_php provides three Dockerfiles for different use cases:

| File | Base | Use Case |
|------|------|----------|
| `Dockerfile` | Alpine | Development and production (full runtime with tests) |
| `Dockerfile.release` | Alpine | Minimal release builds, binary extraction |
| `Dockerfile.debian` | Debian Bookworm | glibc-based builds, better compatibility |

## Quick Start

```bash
# Build and run (development)
docker compose build
docker compose up -d

# With TLS/HTTPS
docker compose --profile tls up -d

# Production with custom PHP version
PHP_VERSION=8.5 docker compose up -d
```

## Dockerfile (Development/Production)

Multi-stage build with test validation:

```
┌─────────────────────────────────────────────────────────────┐
│ Stage: builder (php:8.x-zts-alpine)                         │
├─────────────────────────────────────────────────────────────┤
│ 1. Install build dependencies (rust, cargo, musl-dev, etc.) │
│ 2. Build libtokio_bridge.so (shared TLS library)            │
│ 3. Build tokio_sapi.so (PHP extension)                      │
│ 4. Build libtokio_sapi.a (static library for Rust)          │
│ 5. Run unit tests (cargo test)                              │
│ 6. Build tokio_php binary (cargo build --release)           │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│ Stage: runtime (php:8.x-zts-alpine)                         │
├─────────────────────────────────────────────────────────────┤
│ - Minimal runtime dependencies (libgcc, curl)               │
│ - libtokio_bridge.so in /usr/local/lib/                     │
│ - tokio_sapi.so in PHP extension directory                  │
│ - tokio_php binary in /usr/local/bin/                       │
│ - Runs as www-data user (UID 82)                            │
└─────────────────────────────────────────────────────────────┘
```

### Build

```bash
# Default (PHP 8.4)
docker compose build

# Specific PHP version
PHP_VERSION=8.5 docker compose build

# Without cache
docker compose build --no-cache
```

### Run

```bash
# Start
docker compose up -d

# View logs
docker compose logs -f

# Stop
docker compose down
```

## Dockerfile.release (Minimal Release)

Optimized for production and binary distribution. Two targets:

### Default Target (Runtime Image)

Minimal runtime image without test dependencies:

```bash
docker build -f Dockerfile.release \
  --build-arg PHP_VERSION=8.4 \
  --build-arg ALPINE_VERSION=3.23 \
  -t tokio_php:php8.4-alpine3.23 .
```

### Dist Target (Binary Extraction)

Extracts binaries with proper directory structure for installation on host systems:

```bash
docker build -f Dockerfile.release \
  --build-arg PHP_VERSION=8.4 \
  --build-arg ALPINE_VERSION=3.23 \
  --target dist \
  --output type=local,dest=./dist .
```

Output structure:

```
./dist/
├── usr/
│   └── local/
│       ├── bin/
│       │   └── tokio_php              # Main binary
│       └── lib/
│           ├── libtokio_bridge.so     # Shared library
│           └── php/
│               └── extensions/
│                   └── no-debug-zts-20240924/   # PHP 8.4 API version
│                       └── tokio_sapi.so        # PHP extension
```

PHP extension directory varies by version:
- PHP 8.4: `no-debug-zts-20240924`
- PHP 8.5: `no-debug-zts-20250925`

### Build Args

| Arg | Default | Description |
|-----|---------|-------------|
| `PHP_VERSION` | `8.4` | PHP version (8.4 or 8.5) |
| `ALPINE_VERSION` | `3.23` | Alpine Linux version |

## Dockerfile.debian (Debian Bookworm)

glibc-based build for environments requiring Debian compatibility:

```bash
# Runtime image
docker build -f Dockerfile.debian \
  --build-arg PHP_VERSION=8.4 \
  -t tokio_php:php8.4-bookworm .

# Binary extraction
docker build -f Dockerfile.debian \
  --build-arg PHP_VERSION=8.4 \
  --target dist \
  --output type=local,dest=./dist-debian .
```

### Build Args

| Arg | Default | Description |
|-----|---------|-------------|
| `PHP_VERSION` | `8.4` | PHP version (8.4 or 8.5) |

### Image Size Comparison

| Base | Compressed | Uncompressed |
|------|------------|--------------|
| Alpine | ~53MB | ~209MB |
| Debian Bookworm | ~182MB | ~759MB |

### When to Use Debian

- **glibc compatibility**: Some PHP extensions require glibc (not musl)
- **Debugging**: Standard tools like gdb, valgrind work better
- **Production policy**: Organization requires Debian-based images
- **Native extensions**: Extensions with C dependencies may need glibc

## docker-compose.yml

### Services

| Service | Port | Profile | Description |
|---------|------|---------|-------------|
| `tokio_php` | 8080, 9090 | default | HTTP server |
| `tokio_php_tls` | 8443, 9090 | tls | HTTPS server |

### HTTP Service

```bash
# Start HTTP service
docker compose up -d

# With environment variables
PHP_WORKERS=4 ACCESS_LOG=1 docker compose up -d
```

### HTTPS/TLS Service

```bash
# Start TLS service (uses Docker secrets)
docker compose --profile tls up -d

# Custom certificate files
TLS_CERT_FILE=/path/to/cert.pem TLS_KEY_FILE=/path/to/key.pem \
  docker compose --profile tls up -d
```

TLS certificates are mounted as Docker secrets:
- Mounted at `/run/secrets/tls_cert` and `/run/secrets/tls_key`
- Files in tmpfs (memory only)
- Not visible in `docker inspect`

### Volumes

| Volume | Container Path | Mode | Description |
|--------|----------------|------|-------------|
| `./www` | `/var/www/html` | ro | Application files |
| `./php.ini` | `/usr/local/etc/php/conf.d/tokio_php.ini` | ro | PHP configuration |

### Health Check

Built-in health check via internal server:

```yaml
healthcheck:
  test: ["CMD", "curl", "-sf", "http://localhost:9090/health"]
  interval: 10s
  timeout: 5s
  retries: 3
  start_period: 10s
```

## Environment Variables

### Server Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | Server bind address |
| `INTERNAL_ADDR` | _(empty)_ | Internal server for /health, /metrics |
| `DOCUMENT_ROOT` | `/var/www/html` | Web root directory |
| `INDEX_FILE` | _(empty)_ | Single entry point (e.g., `index.php`) |

### Worker Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PHP_WORKERS` | `0` | Worker count (0 = auto-detect CPU cores) |
| `QUEUE_CAPACITY` | `0` | Max pending requests (0 = workers × 100) |
| `REQUEST_TIMEOUT` | `2m` | Request timeout (30s, 2m, 5m, off) |

### Executor Selection

| Variable | Default | Description |
|----------|---------|-------------|
| `EXECUTOR` | `ext` | Script executor: `ext` (recommended), `php` (legacy), `stub` (benchmark) |

### Middleware

| Variable | Default | Description |
|----------|---------|-------------|
| `ACCESS_LOG` | `0` | Enable access logs |
| `RATE_LIMIT` | `0` | Max requests per IP per window (0 = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window in seconds |
| `STATIC_CACHE_TTL` | `1d` | Static file cache duration |
| `ERROR_PAGES_DIR` | _(empty)_ | Custom error pages directory |

### TLS Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `TLS_CERT` | _(empty)_ | Path to TLS certificate (PEM) |
| `TLS_KEY` | _(empty)_ | Path to TLS private key (PEM) |
| `TLS_CERT_FILE` | `./certs/cert.pem` | Host path for Docker secret |
| `TLS_KEY_FILE` | `./certs/key.pem` | Host path for Docker secret |

### Logging

| Variable | Default | Description |
|----------|---------|-------------|
| `LOG_LEVEL` | `info` | Log level: trace, debug, info, warn, error (simple filter, takes priority over RUST_LOG) |
| `RUST_LOG` | _(fallback)_ | Advanced log filter (e.g., `tokio_php=debug,hyper=warn`). Used only if LOG_LEVEL not set |

**Profiling:** Build with `CARGO_FEATURES=debug-profile` for compile-time profiling. See [Profiling](profiling.md).

## PHP Configuration

PHP settings via `php.ini`:

```ini
; OPcache
opcache.enable=1
opcache.enable_cli=1
opcache.jit=tracing
opcache.jit_buffer_size=128M
opcache.validate_timestamps=0

; Memory
memory_limit=256M

; Preloading (optional)
; opcache.preload=/var/www/html/preload.php
; opcache.preload_user=www-data
```

## Production Configuration

### Recommended Settings

```bash
# Production with tuning
PHP_WORKERS=8 \
QUEUE_CAPACITY=1000 \
ACCESS_LOG=1 \
RATE_LIMIT=100 \
RATE_WINDOW=60 \
STATIC_CACHE_TTL=1w \
RUST_LOG=tokio_php=warn \
docker compose up -d
```

### Resource Limits

```yaml
# docker-compose.override.yml
services:
  tokio_php:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 1G
        reservations:
          cpus: '0.5'
          memory: 256M
```

### Read-only Application

```yaml
# docker-compose.override.yml
services:
  tokio_php:
    volumes:
      - ./www:/var/www/html:ro  # Read-only
```

## Framework Integration

### Symfony

```yaml
services:
  app:
    image: tokio_php
    environment:
      - APP_ENV=${APP_ENV:-dev}
      - PHP_WORKERS=${PHP_WORKERS:-1}  # Single worker for dev
      - DOCUMENT_ROOT=/var/www/html/public
      - INDEX_FILE=index.php
    volumes:
      - .:/var/www/html
```

### Laravel

```yaml
services:
  app:
    image: tokio_php
    environment:
      - APP_ENV=${APP_ENV:-local}
      - PHP_WORKERS=${PHP_WORKERS:-1}
      - DOCUMENT_ROOT=/var/www/html/public
      - INDEX_FILE=index.php
    volumes:
      - .:/var/www/html
```

See [Framework Compatibility](framework-compatibility.md) for thread-safety considerations.

## Troubleshooting

### Container Won't Start

```bash
# Check logs
docker compose logs

# Check if port is in use
lsof -i :8080
```

### Permission Errors

```bash
# Fix ownership
sudo chown -R 82:82 ./www

# Or run with correct permissions
docker compose exec tokio_php ls -la /var/www/html/
```

### TLS Certificate Issues

```bash
# Check certificate paths
docker compose exec tokio_php_tls ls -la /run/secrets/

# Verify certificate
openssl x509 -in ./certs/cert.pem -text -noout
```

### Build Failures

```bash
# Clean build
docker compose build --no-cache

# Check build logs
docker compose build 2>&1 | tee build.log
```

## CI/CD Integration

### GitHub Actions

```yaml
- name: Build
  run: |
    docker build -f Dockerfile.release \
      --build-arg PHP_VERSION=8.4 \
      -t tokio_php:${{ github.sha }} .

- name: Extract binaries
  run: |
    docker build -f Dockerfile.release \
      --target dist \
      --output type=local,dest=./dist .
```

### Binary Distribution

```bash
# Build for multiple PHP versions
for PHP_VERSION in 8.4 8.5; do
  docker build -f Dockerfile.release \
    --build-arg PHP_VERSION=$PHP_VERSION \
    --target dist \
    --output type=local,dest=./dist-php$PHP_VERSION .
done
```

## See Also

- [Configuration](configuration.md) - Full environment variable reference
- [Security](security.md) - Non-root execution and security context
- [HTTP/2 & TLS](http2-tls.md) - TLS configuration details
- [Framework Compatibility](framework-compatibility.md) - Laravel, Symfony setup
- [OPcache & JIT](opcache-jit.md) - PHP performance tuning
- [Architecture](architecture.md) - System design overview
