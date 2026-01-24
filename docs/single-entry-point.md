# Single Entry Point Mode

tokio_php supports single entry point mode for frameworks like Laravel and Symfony that route all requests through one PHP file.

## Overview

In traditional PHP servers, each URL maps to a specific PHP file. Modern frameworks use a single `index.php` file as an entry point, handling routing internally.

```
Traditional Mode:
/users.php     → www/users.php
/products.php  → www/products.php
/api/v1.php    → www/api/v1.php

Single Entry Point Mode:
/users         → www/public/index.php
/products      → www/public/index.php
/api/v1        → www/public/index.php
```

## Configuration

Set the `INDEX_FILE` environment variable:

```bash
# Laravel/Symfony
INDEX_FILE=index.php DOCUMENT_ROOT=/var/www/html/public docker compose up -d

# Or in docker-compose.yml
environment:
  - INDEX_FILE=index.php
  - DOCUMENT_ROOT=/var/www/html/public
```

## Behavior

### All Requests → Index File

```bash
curl http://localhost:8080/               # → index.php
curl http://localhost:8080/users          # → index.php
curl http://localhost:8080/api/v1/users   # → index.php
curl http://localhost:8080/any/path       # → index.php
```

### Direct Access Blocked

Accessing the index file directly returns 404:

```bash
curl http://localhost:8080/index.php      # → 404 Not Found
curl http://localhost:8080/index.php/foo  # → 404 Not Found
```

This prevents duplicate URLs and follows framework best practices.

### PHP $_SERVER Variables

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

```bash
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

Laravel routing works automatically:

```php
// routes/web.php
Route::get('/users', [UserController::class, 'index']);
Route::get('/api/v1/users', [ApiController::class, 'users']);
```

### Symfony

```bash
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

Symfony routing works automatically:

```yaml
# config/routes.yaml
users:
    path: /users
    controller: App\Controller\UserController::index
```

### Custom Framework

```php
<?php

// public/index.php

$uri = $_SERVER['REQUEST_URI'];
$path = parse_url($uri, PHP_URL_PATH);

// Simple router
$routes = [
    '/' => 'HomeController',
    '/users' => 'UserController',
    '/api/v1/users' => 'ApiController',
];

$controller = $routes[$path] ?? 'NotFoundController';
(new $controller)->handle();
```

## Startup Validation

The server validates the index file at startup:

```json
// If file exists
{"ts":"...","level":"info","msg":"Single entry point mode: all requests -> index.php",...}

// If file doesn't exist - server exits with error
Error: Index file not found: /var/www/html/public/index.php (INDEX_FILE=index.php)
```

The server exits with an error if the index file is missing, preventing silent failures.

## Performance Optimization

Single entry point mode skips file existence checks entirely:

```
Normal mode:
1. Parse URL → /users.php
2. Check file exists → stat() or LRU cache lookup
3. Execute PHP or return 404

Single entry point mode:
1. Parse URL → /users
2. Skip file check (index.php pre-validated at startup)
3. Route directly to index.php
```

### Why It's Faster

| Operation | Normal Mode | Single Entry Point |
|-----------|-------------|-------------------|
| File check | ~26µs (first) / 0µs (cached) | **0µs (skipped)** |
| Path resolution | stat() syscall | No syscall |

The index.php path is validated once at server startup. All subsequent requests route directly without filesystem checks.

### Server Variables Optimization

Server variables (`$_SERVER`) are built with zero-allocation optimizations:

- `DOCUMENT_ROOT` — cached at startup, reused via `Cow::Borrowed`
- `REQUEST_METHOD` — static constants for GET, POST, PUT, DELETE, etc.
- `SERVER_PROTOCOL` — static constants for HTTP/1.0, HTTP/1.1, HTTP/2.0

This reduces server_vars build time from ~15µs to ~6-7µs per request.

## Static Files

tokio_php uses `try_files` behavior — static files are served directly, other requests go to index.php:

```bash
curl http://localhost:8080/style.css    # → Served directly (file exists)
curl http://localhost:8080/app.js       # → Served directly (file exists)
curl http://localhost:8080/missing.css  # → index.php (file not found)
curl http://localhost:8080/api/users    # → index.php (not a file)
```

### How It Works

```
Request → Check if file exists in document root
              │
              ├── File exists & not .php → Serve static file directly
              │
              └── File missing or .php → Route to index.php
```

This matches nginx `try_files $uri $uri/ /index.php` behavior.

## Configuration Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `INDEX_FILE` | Entry point filename | `index.php` |
| `DOCUMENT_ROOT` | Web root directory | `/var/www/html/public` |

## Troubleshooting

### 404 for All Requests

Check that `INDEX_FILE` is set correctly:

```bash
docker compose exec app env | grep INDEX_FILE
```

### Index File Not Found

Verify the path:

```bash
docker compose exec app ls -la /var/www/html/public/index.php
```

### Routes Not Working

Check `$_SERVER['REQUEST_URI']` in your PHP code:

```php
<?php
var_dump($_SERVER['REQUEST_URI']);
var_dump($_SERVER['SCRIPT_NAME']);
?>
```

## Comparison with nginx

tokio_php single entry point mode is equivalent to nginx `try_files`:

```nginx
# nginx equivalent
location / {
    try_files $uri $uri/ /index.php$is_args$args;
}

location = /index.php {
    return 404;
}

location ~ \.php$ {
    fastcgi_pass php-fpm:9000;
}
```

Both:
- Serve static files directly if they exist
- Route other requests to index.php
- Block direct access to index.php

## Using HTML Index File (SPA Mode)

`INDEX_FILE` can point to an HTML file for Single Page Applications (SPA) like React, Vue, or Angular.

### Configuration

```bash
# SPA mode - serve index.html for all routes
INDEX_FILE=index.html docker compose up -d
```

### Behavior

When `INDEX_FILE` points to an HTML file:

```
Request              Resolved Path                  Action
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/                    /var/www/html/index.html       Static serve
/about               /var/www/html/index.html       Static serve
/api/users           /var/www/html/index.html       Static serve
/style.css           /var/www/html/style.css        Static serve (exists)
/app.js              /var/www/html/app.js           Static serve (exists)
```

**Important:** PHP is **not executed** when `INDEX_FILE` is an HTML file. The file is served as static content.

### How It Works

The routing decision is based on file extension:

```rust
// routing.rs - resolve to index file
if file_not_found || extension == "php" {
    return index_file_path;  // e.g., index.html
}

// connection.rs - serve based on extension
if extension == "php" {
    // Execute PHP
} else {
    // Serve as static file ← HTML goes here
}
```

### When to Use

| Use Case | INDEX_FILE | Result |
|----------|------------|--------|
| Laravel, Symfony | `index.php` | PHP routing |
| React, Vue, Angular SPA | `index.html` | Client-side routing |
| Static site | `index.html` | Static HTML |

### SPA Example

```bash
# Build your SPA
npm run build

# Serve with tokio_php
docker run -d -p 8080:8080 \
  -e INDEX_FILE=index.html \
  -v $(pwd)/dist:/var/www/html:ro \
  tokio_php
```

All routes serve `index.html`, JavaScript handles routing on the client.

### Mixing PHP and SPA

If you need both PHP API and SPA frontend, use separate paths:

```
/var/www/html/
├── api/
│   └── index.php      # PHP API (accessed directly)
├── index.html         # SPA entry point
├── app.js
└── style.css
```

```bash
# Don't use INDEX_FILE - access files directly
docker run -d -p 8080:8080 \
  -v $(pwd)/dist:/var/www/html:ro \
  tokio_php
```

Or use a reverse proxy (nginx) to route `/api/*` to PHP and other routes to SPA.

## See Also

- [Configuration](configuration.md) - Environment variables reference
- [Architecture](architecture.md) - System design overview
- [Framework Compatibility](framework-compatibility.md) - Laravel, Symfony setup and caveats
- [Static Caching](static-caching.md) - Cache headers for static files
- [Static Files](static-files.md) - Static file serving details
