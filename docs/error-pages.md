# Custom Error Pages

tokio_php supports custom HTML error pages for 4xx and 5xx HTTP responses, similar to [Symfony's error pages](https://symfony.com/doc/current/controller/error_pages.html).

## Overview

When a PHP script returns an error response (4xx/5xx) with an empty body, tokio_php can automatically serve a custom HTML page instead of a plain text message. This provides a better user experience with branded, styled error pages.

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

### Activation Conditions

Error pages are served only when ALL conditions are met:

| Condition | Description |
|-----------|-------------|
| `ERROR_PAGES_DIR` is set | Directory path configured |
| Status code 4xx or 5xx | Error response from PHP |
| Empty response body | PHP returned no content |
| `Accept: text/html` | Client accepts HTML |
| File exists | `{status}.html` found in cache |

If any condition is not met, the default behavior applies (plain text or PHP output).

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
# Test 404 page (browser request)
curl -H "Accept: text/html" http://localhost:8080/nonexistent

# Test with API client (no Accept header - plain text)
curl http://localhost:8080/nonexistent

# Test 503 (queue full scenario)
wrk -t10 -c1000 -d5s http://localhost:8080/slow.php
curl -H "Accept: text/html" http://localhost:8080/index.php
```

## Troubleshooting

### Error pages not showing

1. **Check `ERROR_PAGES_DIR`**:
   ```bash
   docker compose exec tokio_php env | grep ERROR_PAGES
   ```

2. **Verify files exist**:
   ```bash
   docker compose exec tokio_php ls -la /var/www/html/errors/
   ```

3. **Check Accept header**:
   ```bash
   # This should show error page
   curl -H "Accept: text/html" http://localhost:8080/nonexistent

   # This returns plain text
   curl http://localhost:8080/nonexistent
   ```

4. **Check response body is empty**:
   ```php
   <?php
   
   // Wrong - body not empty, error page won't show
   http_response_code(404);
   echo "Not found";

   // Correct - empty body
   http_response_code(404);
   exit;
   ```

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
