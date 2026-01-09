# Framework Compatibility

This document describes how to use tokio_php with popular PHP frameworks.

## Thread Safety Considerations

tokio_php uses multiple PHP worker threads to handle concurrent requests. This requires PHP ZTS (Thread Safe) build and thread-safe application code.

**Important:** Many PHP frameworks perform file operations (cache compilation, container building) that are not thread-safe in development mode. This can cause segmentation faults when running with multiple workers.

## Symfony

### Development Mode

Symfony's development mode compiles cache, container, and routing on-the-fly. These file operations are not thread-safe and will cause crashes with multiple workers.

**Solution: Use single worker in development**

```bash
PHP_WORKERS=1 APP_ENV=dev docker compose up -d
```

### Production Mode

In production, pre-warm the cache before starting with multiple workers:

```bash
# Build and warm cache
docker compose exec app php bin/console cache:clear --env=prod
docker compose exec app php bin/console cache:warmup --env=prod

# Start with multiple workers
APP_ENV=prod APP_DEBUG=0 PHP_WORKERS=0 docker compose up -d
```

### docker-compose.yml Example

```yaml
services:
  app:
    image: tokio_php
    environment:
      - APP_ENV=${APP_ENV:-dev}
      - APP_DEBUG=${APP_DEBUG:-1}
      - PHP_WORKERS=${PHP_WORKERS:-1}  # Safe default for dev
      - DOCUMENT_ROOT=/var/www/html/public
      - INDEX_FILE=index.php
    volumes:
      - .:/var/www/html
```

**Development:**
```bash
docker compose up -d
```

**Production:**
```bash
APP_ENV=prod APP_DEBUG=0 PHP_WORKERS=0 docker compose up -d
```

## Laravel

Laravel has similar cache compilation behavior in development mode.

### Development Mode

```bash
PHP_WORKERS=1 APP_ENV=local docker compose up -d
```

### Production Mode

```bash
# Optimize for production
docker compose exec app php artisan config:cache
docker compose exec app php artisan route:cache
docker compose exec app php artisan view:cache

# Start with multiple workers
APP_ENV=production PHP_WORKERS=0 docker compose up -d
```

### docker-compose.yml Example

```yaml
services:
  app:
    image: tokio_php
    environment:
      - APP_ENV=${APP_ENV:-local}
      - APP_DEBUG=${APP_DEBUG:-true}
      - PHP_WORKERS=${PHP_WORKERS:-1}
      - DOCUMENT_ROOT=/var/www/html/public
      - INDEX_FILE=index.php
    volumes:
      - .:/var/www/html
```

## Other Frameworks

The same principle applies to any framework that performs file-based caching or compilation:

| Framework | Dev Mode | Prod Mode |
|-----------|----------|-----------|
| Symfony | `PHP_WORKERS=1` | `cache:warmup` + multiple workers |
| Laravel | `PHP_WORKERS=1` | `artisan optimize` + multiple workers |
| Laminas | `PHP_WORKERS=1` | Pre-warm config cache |
| Yii2 | `PHP_WORKERS=1` | Disable debug, enable caching |
| CodeIgniter | Usually safe | Multiple workers OK |
| Slim | Usually safe | Multiple workers OK |

Micro-frameworks (Slim, Lumen) typically don't have this issue as they don't perform heavy file-based caching.

## Diagnosing Thread Safety Issues

### Symptoms

- Segmentation fault (exit code 139) under load
- Works with 1 worker, crashes with 2+
- Debug logging makes the crash disappear (timing change masks race condition)

### Debugging Steps

1. **Isolate the framework:**
   ```bash
   # Test with simple PHP script
   echo '<?php echo "ok";' > public/test.php
   wrk -t2 -c100 -d10s http://localhost:8080/test.php
   ```
   If `test.php` works but your app crashes, it's a framework issue.

2. **Check exit code:**
   ```bash
   docker inspect --format='ExitCode: {{.State.ExitCode}}, OOMKilled: {{.State.OOMKilled}}' $(docker compose ps -aq)
   ```
   - Exit code 139 = SIGSEGV (segmentation fault)
   - Exit code 137 + OOMKilled = Out of memory

3. **Try single worker:**
   ```bash
   PHP_WORKERS=1 docker compose up -d
   ```
   If single worker works, it's a thread-safety issue.

## Why This Happens

PHP frameworks in development mode typically:

1. **Check file timestamps** on every request
2. **Recompile cache** when files change
3. **Rebuild DI container** dynamically
4. **Generate routing tables** on-the-fly

These operations involve:
- Reading/writing multiple files
- File locking (or lack thereof)
- Shared state between operations

When multiple workers do this simultaneously, race conditions occur:
- Worker 1 writes partial file
- Worker 2 reads incomplete file
- Segmentation fault

## Not a tokio_php Limitation

This is not specific to tokio_php. The same issues occur with:

- **Swoole** - Same thread-safety requirements
- **RoadRunner** - Worker-based, similar issues
- **FrankenPHP** - Thread-based like tokio_php
- **ReactPHP** - Event loop, same file race conditions

The solution is always the same: single worker for development, pre-warmed cache for production.
