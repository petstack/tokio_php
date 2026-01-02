# Custom Error Pages

tokio_php supports custom HTML error pages for 4xx and 5xx HTTP responses, similar to [Symfony's error pages](https://symfony.com/doc/current/controller/error_pages.html).

## Overview

When a PHP script returns an error response (4xx/5xx) with an empty body, tokio_php automatically provides a response body:

1. **HTML clients** (`Accept: text/html`) — custom HTML page if configured, otherwise plain text
2. **API clients** (`Accept: application/json`, etc.) — plain text reason phrase

### Default Behavior (No Custom Pages)

Without `ERROR_PAGES_DIR`, the server returns standard HTTP reason phrases:

```bash
$ curl http://localhost:8080/nonexistent.php
Not Found

$ curl http://localhost:8080/timeout.php
Gateway Timeout
```

| Status | Response Body |
|--------|---------------|
| 400 | Bad Request |
| 401 | Unauthorized |
| 403 | Forbidden |
| 404 | Not Found |
| 405 | Method Not Allowed |
| 429 | Too Many Requests |
| 500 | Internal Server Error |
| 502 | Bad Gateway |
| 503 | Service Unavailable |
| 504 | Gateway Timeout |

### With Custom Pages

Set `ERROR_PAGES_DIR` to serve branded HTML error pages for browser clients:

## Configuration

Enable custom error pages by setting the `ERROR_PAGES_DIR` environment variable:

```bash
ERROR_PAGES_DIR=/var/www/html/errors docker compose up -d
```

## File Naming Convention

Error page files must be named using the HTTP status code:

```
{status_code}.html
```

Examples:
- `400.html` - Bad Request
- `401.html` - Unauthorized
- `403.html` - Forbidden
- `404.html` - Not Found
- `405.html` - Method Not Allowed
- `500.html` - Internal Server Error
- `502.html` - Bad Gateway
- `503.html` - Service Unavailable
- `504.html` - Gateway Timeout

## Directory Structure

```
www/
├── errors/
│   ├── 400.html
│   ├── 401.html
│   ├── 403.html
│   ├── 404.html
│   ├── 500.html
│   ├── 502.html
│   ├── 503.html
│   └── 504.html
├── index.php
└── ...
```

## How It Works

1. **Startup**: Server reads all `*.html` files from `ERROR_PAGES_DIR` and caches them in memory
2. **Request**: Client sends request with `Accept: text/html` header
3. **Response**: PHP returns 4xx/5xx status with empty body
4. **Injection**: Server replaces empty body with cached HTML content

### Response Selection Logic

```
Response with empty body (4xx/5xx)
         │
         ▼
   ┌─────────────────┐
   │ Accept: text/html? │
   └────────┬────────┘
            │
    ┌───────┴───────┐
    │ Yes           │ No
    ▼               ▼
┌─────────┐    ┌─────────────┐
│ Custom  │    │ Plain text  │
│ page    │    │ "Not Found" │
│ exists? │    └─────────────┘
└────┬────┘
     │
 ┌───┴───┐
 │ Yes   │ No
 ▼       ▼
┌────┐ ┌─────────────┐
│HTML│ │ Plain text  │
│page│ │ "Not Found" │
└────┘ └─────────────┘
```

| Client | Custom Page Exists | Response |
|--------|-------------------|----------|
| Browser (`Accept: text/html`) | Yes | Custom HTML page |
| Browser (`Accept: text/html`) | No | Plain text (e.g., "Not Found") |
| API (`Accept: application/json`) | — | Plain text (e.g., "Not Found") |
| curl (no Accept) | — | Plain text (e.g., "Not Found") |

## Performance

- **Memory caching**: Files loaded once at startup, no disk I/O per request
- **Zero overhead**: When disabled (`ERROR_PAGES_DIR` empty), no additional processing
- **Minimal latency**: Direct memory copy, no template rendering

## Symfony Integration

Symfony provides a command to [dump error pages as static HTML files](https://symfony.com/doc/current/controller/error_pages.html#dumping-error-pages-as-static-html-files). This approach is recommended for production because:

- Error pages are served even if PHP crashes
- No application overhead for error responses
- Instant delivery without framework initialization

### Generating Static Pages from Symfony

```bash
# Generate all error pages (4xx and 5xx)
APP_ENV=prod php bin/console error:dump var/error_pages/

# Generate specific status codes only
APP_ENV=prod php bin/console error:dump var/error_pages/ 401 403 404 500 503
```

This creates files like:
```
var/error_pages/
├── 400.html
├── 401.html
├── 403.html
├── 404.html
├── 500.html
├── 502.html
├── 503.html
└── 504.html
```

### Traditional Setup (Nginx + PHP-FPM)

With traditional PHP deployment, you need to configure Nginx to serve static error pages:

```nginx
server {
    # ... existing configuration ...

    error_page 400 /error_pages/400.html;
    error_page 401 /error_pages/401.html;
    error_page 403 /error_pages/403.html;
    error_page 404 /error_pages/404.html;
    error_page 500 /error_pages/500.html;
    error_page 502 /error_pages/502.html;
    error_page 503 /error_pages/503.html;
    error_page 504 /error_pages/504.html;

    location ^~ /error_pages/ {
        root /path/to/symfony/var;
        internal;  # prevent direct URL access
    }
}
```

### tokio_php Setup

With tokio_php, no web server configuration is needed. Just point to the directory:

```bash
# Use Symfony-generated error pages directly
ERROR_PAGES_DIR=/var/www/html/var/error_pages docker compose up -d
```

### Comparison

| Aspect | Nginx + PHP-FPM | tokio_php |
|--------|-----------------|-----------|
| Error page source | Symfony `error:dump` | Symfony `error:dump` or manual |
| Configuration | Nginx config per status | Single env variable |
| File naming | `{code}.html` | `{code}.html` |
| Direct URL access | Blocked via `internal` | Not exposed |
| Accept header check | No (always serves HTML) | Yes (respects client preference) |
| Memory caching | Nginx file cache | Loaded at startup |
| Hot reload | Automatic | Requires restart |

### Key Advantages of tokio_php

1. **Zero configuration**: No Nginx rules needed, just set `ERROR_PAGES_DIR`
2. **Content negotiation**: Respects `Accept` header - API clients get empty body, browsers get HTML
3. **Single binary**: No separate web server layer to configure
4. **Atomic deployment**: Error pages cached at startup, consistent across requests

### Migration from Nginx + PHP-FPM

1. Generate static error pages with Symfony:
   ```bash
   APP_ENV=prod php bin/console error:dump public/errors/
   ```

2. Copy to your tokio_php document root:
   ```bash
   cp -r var/error_pages/ /var/www/html/errors/
   ```

3. Configure tokio_php:
   ```bash
   ERROR_PAGES_DIR=/var/www/html/errors docker compose up -d
   ```

4. Remove Nginx error_page directives (no longer needed)

## PHP Integration

To trigger custom error pages from PHP, return the appropriate status code with an empty body:

```php
<?php

// Return 404 with empty body - error page will be injected
http_response_code(404);
exit;

// Or with header()
header('HTTP/1.1 404 Not Found');
exit;
```

For custom error handling in frameworks:

```php
<?php

// Laravel-style exception handler
try {
    // ... application logic
} catch (ModelNotFoundException $e) {
    http_response_code(404);
    exit; // Empty body triggers error page
} catch (Exception $e) {
    http_response_code(500);
    exit;
}
```

## Testing

```bash
# API client - always gets plain text
$ curl http://localhost:8080/nonexistent
Not Found

# Browser client with custom error pages
$ curl -H "Accept: text/html" http://localhost:8080/nonexistent
<!DOCTYPE html>
<html>... custom 404 page ...

# Browser client without custom pages (ERROR_PAGES_DIR not set)
$ curl -H "Accept: text/html" http://localhost:8080/nonexistent
Not Found

# Check headers
$ curl -sI http://localhost:8080/nonexistent
HTTP/1.1 404 Not Found
content-type: text/plain; charset=utf-8
content-length: 9

$ curl -sI -H "Accept: text/html" http://localhost:8080/nonexistent
HTTP/1.1 404 Not Found
content-type: text/html; charset=utf-8
content-length: 1234
```

## Troubleshooting

### Custom HTML pages not showing

1. **Check `ERROR_PAGES_DIR`**:
   ```bash
   docker compose exec tokio_php env | grep ERROR_PAGES
   ```

2. **Verify files exist**:
   ```bash
   docker compose exec tokio_php ls -la /var/www/html/errors/
   ```

3. **Check Accept header** — custom pages only served to HTML clients:
   ```bash
   # This shows custom HTML page (if configured)
   curl -H "Accept: text/html" http://localhost:8080/nonexistent

   # This always returns plain text "Not Found"
   curl http://localhost:8080/nonexistent
   ```

4. **Check response body is empty** — error pages only injected for empty bodies:
   ```php
   <?php

   // Wrong - body not empty, custom page won't show
   http_response_code(404);
   echo "Not found";  // This will be the response

   // Correct - empty body, allows error page injection
   http_response_code(404);
   exit;
   ```

### Getting "Not Found" instead of custom page

This is expected behavior when:
- `ERROR_PAGES_DIR` is not set
- File `{status}.html` doesn't exist in the directory
- Client doesn't send `Accept: text/html` header

The server always returns a human-readable reason phrase (e.g., "Not Found", "Bad Gateway") instead of an empty body.

### Startup logs

Check if files were loaded:
```bash
docker compose logs tokio_php | grep -i "error\|Loaded"
```

Expected output (JSON format):
```json
{"msg":"Error pages directory: \"/var/www/html/errors","level":"info","type":"app",...}
{"msg":"Loaded 3 error pages: [404, 503, 500]","level":"info","type":"app",...}
```

Or formatted:
```bash
docker compose logs tokio_php | jq -r 'select(.msg | contains("error") or contains("Loaded")) | .msg'
```

```
Error pages directory: "/var/www/html/errors
Loaded 3 error pages: [404, 503, 500]
```

## See Also

- [Middleware](middleware.md) - Middleware system overview
- [Configuration](configuration.md) - Environment variables reference
- [Symfony Error Pages](https://symfony.com/doc/current/controller/error_pages.html) - Symfony's approach to error pages
