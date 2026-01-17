# PHP Support

tokio_php executes PHP scripts via the **php-embed SAPI** — a C library that allows embedding PHP into other applications.

## Supported Versions

| Version | Status | Docker Tag |
|---------|--------|------------|
| PHP 8.5 | Supported (default) | `diolektor/tokio_php:php8.5` |
| PHP 8.4 | Supported | `diolektor/tokio_php:php8.4` |

PHP 8.5 is the default version in Docker images.

## Requirements

tokio_php requires a **custom PHP build** with specific configuration:

| Requirement | Description |
|-------------|-------------|
| **ZTS (Thread Safe)** | `--enable-zts` — required for multi-threaded worker pool |
| **embed SAPI** | `--enable-embed` — provides `libphp.so` shared library |
| **Shared library** | Builds PHP as `libphp.so` instead of CLI binary |

### Why ZTS?

tokio_php uses a multi-threaded worker pool where each thread executes PHP scripts concurrently. PHP's Thread Safe Runtime Manager (TSRM) ensures that global state is isolated per thread:

```
┌─────────────────────────────────────────────────────────────┐
│                      Shared Memory                          │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                     OPcache                           │  │
│  │  - Compiled bytecode (shared across all workers)      │  │
│  │  - JIT compiled code                                  │  │
│  │  - Interned strings                                   │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
              │              │              │
              ▼              ▼              ▼
       ┌───────────┐  ┌───────────┐  ┌───────────┐
       │ Worker 0  │  │ Worker 1  │  │ Worker N  │
       │  (TSRM)   │  │  (TSRM)   │  │  (TSRM)   │
       │           │  │           │  │           │
       │ $_GET     │  │ $_GET     │  │ $_GET     │
       │ $_POST    │  │ $_POST    │  │ $_POST    │
       │ $_SERVER  │  │ $_SERVER  │  │ $_SERVER  │
       └───────────┘  └───────────┘  └───────────┘
          Thread          Thread          Thread
         Isolated        Isolated        Isolated
```

Without ZTS, global PHP state would be shared between threads, causing data corruption.

### Why embed SAPI?

The embed SAPI provides C functions for:
- `php_embed_init()` — initialize PHP runtime
- `php_request_startup()` / `php_request_shutdown()` — request lifecycle
- `php_execute_script()` — execute PHP files
- `zend_eval_string()` — execute PHP code strings

This allows tokio_php (written in Rust) to call PHP directly via FFI, without spawning processes or using sockets.

## BYOP — Bring Your Own PHP

The Docker images include PHP, but if you're building from source or deploying without Docker, you need to provide a compatible PHP installation.

### Building PHP from Source

```bash
# Download PHP source
wget https://www.php.net/distributions/php-8.4.0.tar.gz
tar xzf php-8.4.0.tar.gz
cd php-8.4.0

# Configure with required options
./configure \
    --enable-zts \
    --enable-embed=shared \
    --enable-opcache \
    --with-zlib \
    --with-curl \
    --with-openssl \
    --enable-mbstring \
    --enable-sockets

# Build and install
make -j$(nproc)
sudo make install
```

Key configure options:
- `--enable-zts` — Thread Safety (required)
- `--enable-embed=shared` — Build as shared library (required)
- `--enable-opcache` — OPcache support (recommended)

### Verifying Your PHP Build

```bash
# Check ZTS is enabled
php -i | grep "Thread Safety"
# Output: Thread Safety => enabled

# Check embed SAPI is available
ls /usr/local/lib/libphp*.so
# Output: /usr/local/lib/libphp.so

# Check php-config paths
php-config --includes
php-config --ldflags
php-config --extension-dir
```

## Docker Images

### Available Tags

| Tag | Contents | Use Case |
|-----|----------|----------|
| `latest`, `php8.5` | Full image (PHP 8.5 + server) | Production, development |
| `php8.4` | Full image (PHP 8.4 + server) | Production, development |
| `php8.5-bin` | Binaries only | Custom PHP builds |
| `php8.4-bin` | Binaries only | Custom PHP builds |

### Binary-Only Images

For advanced users who want to use their own PHP build, binary-only images are available:

```bash
# Extract binaries from the image
docker create --name tmp diolektor/tokio_php:php8.5-bin
docker cp tmp:/tokio_php ./tokio_php
docker cp tmp:/tokio_sapi.so ./tokio_sapi.so
docker cp tmp:/libtokio_bridge.so ./libtokio_bridge.so
docker rm tmp
```

Contents:
- `/tokio_php` — the server binary
- `/tokio_sapi.so` — PHP extension for ExtExecutor
- `/libtokio_bridge.so` — shared library for Rust-PHP communication (required)

Use these when:
- Building a custom Docker image with specific PHP extensions
- Deploying to a system with an existing PHP ZTS installation
- Creating minimal production images

Example custom Dockerfile using [official PHP image](https://hub.docker.com/_/php) scripts:

```dockerfile
FROM php:8.4-zts-alpine

# Install build dependencies
RUN apk add --no-cache --virtual .build-deps \
    $PHPIZE_DEPS \
    linux-headers

# Install PHP extensions using official scripts
# See: https://github.com/docker-library/docs/blob/master/php/README.md#how-to-install-more-php-extensions
RUN docker-php-ext-install pdo_mysql opcache
RUN docker-php-ext-enable opcache

# For extensions requiring configure options
RUN docker-php-ext-configure gd --with-freetype --with-jpeg && \
    docker-php-ext-install gd

# For PECL extensions
RUN pecl install redis && \
    docker-php-ext-enable redis

# Cleanup build dependencies
RUN apk del .build-deps

# Copy tokio_php binaries
COPY --from=diolektor/tokio_php:php8.4-bin /tokio_php /usr/local/bin/
COPY --from=diolektor/tokio_php:php8.4-bin /tokio_sapi.so /tmp/
COPY --from=diolektor/tokio_php:php8.4-bin /libtokio_bridge.so /usr/local/lib/

# Install tokio_bridge shared library
RUN ldconfig 2>/dev/null || echo "/usr/local/lib" >> /etc/ld-musl-x86_64.path

# Install tokio_sapi extension
RUN EXT_DIR=$(php-config --extension-dir) && \
    cp /tmp/tokio_sapi.so "$EXT_DIR/" && \
    rm /tmp/tokio_sapi.so && \
    docker-php-ext-enable tokio_sapi

# Configure OPcache for production
RUN echo "opcache.enable=1" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.jit=tracing" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.jit_buffer_size=100M" >> /usr/local/etc/php/conf.d/opcache.ini

WORKDIR /var/www/html
CMD ["tokio_php"]
```

Official PHP image helper scripts:
- `docker-php-ext-configure` — configure extension before building
- `docker-php-ext-install` — compile and install bundled extensions
- `docker-php-ext-enable` — enable an already installed extension

### What's Included

| Component | Description |
|-----------|-------------|
| PHP 8.4/8.5 ZTS | Thread-safe PHP with embed SAPI |
| OPcache | Bytecode caching and JIT |
| tokio_sapi extension | FFI superglobals for ExtExecutor |
| libtokio_bridge | Shared library for Rust-PHP communication |
| tokio_php binary | The Rust server |
| Common extensions | curl, mbstring, openssl, zlib, etc. |

### What's NOT Included

| Component | How to Add |
|-----------|------------|
| Your PHP code | Mount via `-v ./www:/var/www/html` |
| Composer dependencies | Run `composer install` in your project |
| Additional PHP extensions | Build custom Docker image |
| Database drivers | Build custom Docker image |

### Custom Docker Image

To add PHP extensions to the full image:

```dockerfile
FROM diolektor/tokio_php:php8.4

# Install build dependencies
RUN apk add --no-cache --virtual .build-deps $PHPIZE_DEPS

# Install bundled extensions
RUN docker-php-ext-install pdo_mysql

# Install PECL extensions
RUN pecl install redis && \
    docker-php-ext-enable redis

# Cleanup
RUN apk del .build-deps
```

## PHP Configuration

### php.ini Settings

Configure PHP via environment or mounted config:

```bash
# Mount custom php.ini
docker run -v ./php.ini:/usr/local/etc/php/php.ini diolektor/tokio_php

# Or use PHP_INI_SCAN_DIR
docker run -e PHP_INI_SCAN_DIR=/app/config diolektor/tokio_php
```

### OPcache Configuration

The Docker image includes optimized OPcache settings:

```ini
; OPcache enabled
opcache.enable=1
opcache.enable_cli=1

; JIT enabled (tracing mode for best performance)
opcache.jit=tracing
opcache.jit_buffer_size=100M

; Production settings
opcache.validate_timestamps=0
opcache.revalidate_freq=0

; Memory settings
opcache.memory_consumption=256
opcache.interned_strings_buffer=64
opcache.max_accelerated_files=32531
```

See [OPcache & JIT](opcache-jit.md) for detailed configuration.

## Executors

tokio_php provides two PHP execution modes:

### ExtExecutor (Recommended)

Uses `php_execute_script()` + FFI for superglobals:

```bash
USE_EXT=1 docker compose up -d  # Default
```

- 48% faster for real applications
- Native PHP execution path
- Full OPcache/JIT optimization
- Requires tokio_sapi extension

### PhpExecutor

Uses `zend_eval_string()` for superglobals:

```bash
USE_EXT=0 docker compose up -d
```

- Simpler execution model
- No extension dependency
- Better for minimal scripts
- Slightly higher overhead for complex apps

See [Architecture](architecture.md#executor-system) for detailed comparison.

## Extensions

### Bundled Extensions

The Docker image includes these extensions:

| Extension | Purpose |
|-----------|---------|
| OPcache | Bytecode caching, JIT |
| tokio_sapi | FFI superglobals (for ExtExecutor) |
| curl | HTTP client |
| mbstring | Multibyte string support |
| openssl | Cryptography |
| zlib | Compression |
| json | JSON encoding/decoding |
| pdo | Database abstraction |
| pdo_sqlite | SQLite driver |

### tokio_sapi Extension

Custom extension providing:

- `tokio_request_id()` — current request ID
- `tokio_worker_id()` — current worker thread ID
- `tokio_server_info()` — server configuration including build version with git hash
- `tokio_request_heartbeat(int $time = 10)` — extend request timeout
- `tokio_finish_request()` — send response immediately, continue script in background
- `$_SERVER['TOKIO_SERVER_BUILD_VERSION']` — build version string (e.g., `"0.1.0 (abc12345)"`)

See [tokio_sapi Extension](tokio-sapi-extension.md) for details.

## Limitations

### Not Supported

| Feature | Reason | Alternative |
|---------|--------|-------------|
| `$_SESSION` | No session handler | Use Redis/database sessions |
| `pcntl_*` | No process control in embed | Not applicable |
| `readline` | No interactive input | Not applicable |

### Timeout Handling

`set_time_limit()` works for PHP's internal timeout, but does not affect the server's request timeout (`REQUEST_TIMEOUT`). For long-running scripts, use both:

```php
<?php
// Extend PHP's max_execution_time
set_time_limit(60);

// Extend server's request deadline
tokio_request_heartbeat(60);
```

### Compatibility Notes

1. **Output buffering** — Works normally, but final flush happens at request end
2. **Headers** — Must be sent before output (standard PHP behavior)
3. **Exit/die** — Properly terminates script, response is sent
4. **Errors** — Fatal errors return 500, notices/warnings go to stderr
5. **Memory limit** — Per-request, reset on each request

## Building Without PHP

The project can be compiled without PHP for specific use cases using Cargo feature flags.

### Feature Flags

```toml
# Cargo.toml
[features]
default = ["php"]
php = []
stub = []
```

### Build Variants

| Command | PHP Required | Available Executors |
|---------|--------------|---------------------|
| `cargo build` | Yes | PhpExecutor, ExtExecutor, StubExecutor |
| `cargo build --no-default-features --features stub` | No | StubExecutor only |
| `cargo build --no-default-features` | No | None (won't start) |

### Building Stub-Only Binary

```bash
# Build without PHP dependency
cargo build --release --no-default-features --features stub

# Run in stub mode
USE_STUB=1 ./target/release/tokio_php
```

The stub-only build:
- Does not link against `libphp.so`
- Does not require PHP headers or `php-config`
- Returns empty 200 OK for PHP requests (`.php` files)
- **Full static file server** — serves HTML, CSS, JS, images normally
- All middleware works: Brotli compression, caching, rate limiting, error pages

### Use Cases

| Use Case | Description |
|----------|-------------|
| HTTP benchmarks | Measure server overhead without PHP execution |
| CI/CD testing | Test infrastructure without PHP installation |
| Rust development | Develop HTTP/middleware code without PHP setup |
| Load testing | Stress test connection handling and routing |

### How It Works

1. **build.rs** — conditionally links PHP:
   ```rust
   if env::var("CARGO_FEATURE_PHP").is_err() {
       return;  // Skip PHP linking
   }
   ```

2. **Conditional compilation** — PHP code behind feature gates:
   ```rust
   #[cfg(feature = "php")]
   mod sapi;

   #[cfg(feature = "php")]
   pub use ext::ExtExecutor;
   ```

3. **Runtime selection** — executor chosen at startup:
   ```rust
   let executor: Box<dyn ScriptExecutor> = if use_stub {
       Box::new(StubExecutor::new())
   } else {
       // PHP executors...
   };
   ```

## Troubleshooting

### "PHP not found" Error

```
Error: libphp.so not found
```

Ensure PHP is built with `--enable-embed` and `libphp.so` is in the library path:

```bash
export LD_LIBRARY_PATH=/usr/local/lib:$LD_LIBRARY_PATH
```

### "Thread Safety disabled" Error

```
Error: PHP must be built with --enable-zts
```

Rebuild PHP with ZTS enabled:

```bash
./configure --enable-zts --enable-embed=shared ...
```

### Extension Not Loading

```
Warning: Module 'xxx' not found
```

Check extension directory and php.ini:

```bash
php-config --extension-dir
cat /usr/local/etc/php/conf.d/*.ini
```

## See Also

- [Architecture](architecture.md) — System design and request flow
- [OPcache & JIT](opcache-jit.md) — Performance optimization
- [Worker Pool](worker-pool.md) — Thread pool configuration
- [tokio_sapi Extension](tokio-sapi-extension.md) — Custom PHP functions
- [Configuration](configuration.md) — Environment variables
