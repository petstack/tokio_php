# Routing System

tokio_php supports three routing modes based on the `INDEX_FILE` environment variable.

## Overview

The routing system determines how requests are mapped to files:

| Mode | INDEX_FILE | Use Case |
|------|------------|----------|
| **Traditional** | _(empty)_ | Classic PHP sites, direct file access |
| **Framework** | `index.php` | Laravel, Symfony, WordPress |
| **SPA** | `index.html` | React, Vue, Angular applications |

## Routing Components

| Component | Location | Description |
|-----------|----------|-------------|
| `RouteConfig` | `src/server/routing.rs` | Configuration struct |
| `RouteResult` | `src/server/routing.rs` | `Execute(path)`, `Serve(path)`, `NotFound` |
| `FileCache` | `src/server/file_cache.rs` | LRU cache for file metadata |

## Routing Logic

```
1. Decode URI and sanitize (remove "..")
2. Direct access to INDEX_FILE? → 404
3. INDEX_FILE=*.php and uri=*.php? → 404 (blocks all PHP)
4. Root path "/"? → resolve with index file
5. Trailing slash? → directory mode
6. File exists? → serve/execute based on extension
7. INDEX_FILE set? → fallback to index file
8. → 404
```

## Traditional Mode (INDEX_FILE='')

Default mode when `INDEX_FILE` is not set. Direct file-to-URL mapping.

```bash
docker compose up -d  # No INDEX_FILE
```

### Behavior

| Request | File Exists | Result |
|---------|-------------|--------|
| `/` | — | Try index.php → index.html → 404 |
| `/about/` | — | Try about/index.php → about/index.html → 404 |
| `/script.php` | Yes | Execute PHP |
| `/script.php` | No | 404 |
| `/style.css` | Yes | Serve static |
| `/style.css` | No | 404 |
| `/admin` | Directory | 404 (no redirect to /admin/) |

### Root Resolution

For root path `/` and directories with trailing slash:
1. Check `index.php` → Execute if exists
2. Check `index.html` → Serve if exists
3. Return 404

## Framework Mode (INDEX_FILE=index.php)

For Laravel, Symfony, and other PHP frameworks.

```bash
INDEX_FILE=index.php DOCUMENT_ROOT=/var/www/html/public docker compose up -d
```

### Behavior

| Request | File Exists | Result |
|---------|-------------|--------|
| `/` | — | Execute index.php |
| `/api/users` | No | Execute index.php |
| `/about/` | — | Execute index.php |
| `/index.php` | — | **404** (direct access blocked) |
| `/admin.php` | Yes | **404** (all .php blocked) |
| `/other.php` | No | **404** (all .php blocked) |
| `/style.css` | Yes | Serve static |
| `/missing.css` | No | Execute index.php |

### Key Security Feature

When `INDEX_FILE` is a PHP file, **ALL** `.php` requests return 404:
- Prevents access to internal PHP files
- Forces all routing through the framework
- Matches nginx `try_files` + `location ~ \.php$` deny pattern

### Direct Access Protection

Direct access to the index file is blocked:

```bash
curl http://localhost:8080/index.php      # → 404
curl http://localhost:8080/index.php/foo  # → 404
```

## SPA Mode (INDEX_FILE=index.html)

For Single Page Applications (React, Vue, Angular).

```bash
INDEX_FILE=index.html docker compose up -d
```

### Behavior

| Request | File Exists | Result |
|---------|-------------|--------|
| `/` | — | Serve index.html |
| `/about` | No | Serve index.html |
| `/users/123` | No | Serve index.html |
| `/index.html` | — | **404** (direct access blocked) |
| `/api.php` | Yes | Execute PHP |
| `/api.php` | No | Serve index.html |
| `/style.css` | Yes | Serve static |
| `/missing.css` | No | Serve index.html |

### Key Difference from Framework Mode

When `INDEX_FILE` is an HTML file:
- PHP files **still execute** if they exist
- Only non-existent paths fallback to index.html
- Allows hybrid PHP API + SPA frontend

## Directory Handling

### Trailing Slash Semantics

- `/about/` (trailing slash) = **directory mode**, looks for index file in `/about/`
- `/about` (no trailing slash) = **file mode**, checks if `/about` is a file

### No Automatic Redirect

Unlike nginx, tokio_php does **not** redirect `/about` to `/about/`:
- `/about` (directory exists) → 404
- `/about/` (directory exists) → resolve index file in directory

This strict behavior prevents redirect loops and matches modern SPA expectations.

## File Cache (LRU)

The `FileCache` reduces filesystem syscalls:

```rust
pub struct FileCache {
    entries: RwLock<HashMap<Box<str>, Option<FileType>>>,
    order: RwLock<Vec<Box<str>>>,
    capacity: usize,  // 200 entries
}

pub enum FileType {
    File,
    Dir,
}
```

### Cache Behavior

| Scenario | Action |
|----------|--------|
| Path in cache | Return cached FileType (File, Dir, None) |
| Path not in cache | Call `stat()`, cache result |
| Cache full | Evict oldest entry (LRU) |
| Negative cache | "File not found" is also cached as `None` |

### Performance Impact

| Operation | Without Cache | With Cache |
|-----------|---------------|------------|
| File check | ~26µs (stat syscall) | ~0µs |
| Cache lookup | — | O(1) HashMap |

## PHP $_SERVER Variables

The original request URI is preserved:

```php
<?php
// Request: GET /api/users?page=1

echo $_SERVER['REQUEST_URI'];     // /api/users?page=1
echo $_SERVER['SCRIPT_NAME'];     // /index.php
echo $_SERVER['SCRIPT_FILENAME']; // /var/www/html/public/index.php
echo $_SERVER['QUERY_STRING'];    // page=1
```

## Framework Examples

### Laravel

```yaml
# docker-compose.yml
services:
  app:
    image: tokio_php
    environment:
      - INDEX_FILE=index.php
      - DOCUMENT_ROOT=/var/www/html/public
    volumes:
      - ./laravel-app:/var/www/html:ro
```

### Symfony

```yaml
# docker-compose.yml
services:
  app:
    image: tokio_php
    environment:
      - INDEX_FILE=index.php
      - DOCUMENT_ROOT=/var/www/html/public
    volumes:
      - ./symfony-app:/var/www/html:ro
```

### React/Vue SPA

```bash
# Build your SPA
npm run build

# Serve with tokio_php
docker run -d -p 8080:8080 \
  -e INDEX_FILE=index.html \
  -v $(pwd)/dist:/var/www/html:ro \
  tokio_php
```

### Hybrid: PHP API + SPA Frontend

For apps needing both PHP API and SPA:

```
/var/www/html/
├── api.php            # PHP API endpoint
├── index.html         # SPA entry point
├── app.js
└── style.css
```

```bash
INDEX_FILE=index.html docker compose up -d
```

- `/api.php` → Execute PHP (file exists)
- `/users/123` → Serve index.html (SPA route)
- `/style.css` → Serve static (file exists)

## Comparison with nginx

```nginx
# nginx equivalent for Framework mode
location / {
    try_files $uri $uri/ /index.php$is_args$args;
}

location = /index.php {
    return 404;
}

location ~ \.php$ {
    return 404;  # Block all .php when INDEX_FILE is PHP
}
```

```nginx
# nginx equivalent for SPA mode
location / {
    try_files $uri $uri/ /index.html;
}

location = /index.html {
    return 404;
}

location ~ \.php$ {
    fastcgi_pass php-fpm:9000;  # PHP still works
}
```

## Configuration Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `INDEX_FILE` | Entry point filename | `index.php`, `index.html`, or empty |
| `DOCUMENT_ROOT` | Web root directory | `/var/www/html/public` |

## Troubleshooting

### 404 for All Requests

Check that `INDEX_FILE` is set correctly:

```bash
docker compose exec app env | grep INDEX_FILE
```

### PHP Files Return 404 in Framework Mode

This is expected behavior. When `INDEX_FILE=index.php`, all `.php` requests are blocked.

### Index File Not Found at Startup

The server validates the index file exists at startup:

```
Error: Index file not found: /var/www/html/public/index.php (INDEX_FILE=index.php)
```

Verify the path:

```bash
docker compose exec app ls -la /var/www/html/public/index.php
```

## See Also

- [Architecture](architecture.md) - System design overview, routing flow
- [Static Files](static-files.md) - Static file serving with SPA mode
- [Framework Compatibility](framework-compatibility.md) - Laravel, Symfony thread-safety
- [Static Caching](static-caching.md) - Cache headers for static files
- [Configuration](configuration.md) - Environment variables reference
