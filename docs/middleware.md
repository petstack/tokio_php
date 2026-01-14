# Middleware

tokio_php uses a middleware system for cross-cutting concerns like rate limiting, compression, logging, and error handling. See [Architecture](architecture.md) for system overview.

## Architecture

```
Request path (low priority first):
Request → Rate Limit → Access Log → Static Cache → Error Pages → Compression → Handler
             (-100)       (-90)         (50)          (90)         (100)

Response path (high priority first, reverse order):
Handler → Compression → Error Pages → Static Cache → Access Log → Rate Limit → Response
             (100)          (90)          (50)         (-90)        (-100)
```

Middleware is ordered by priority:
- **Lower priority** executes first for requests
- **Higher priority** executes first for responses (reverse order)

## Middleware Trait

```rust
pub trait Middleware: Send + Sync {
    /// Unique name for this middleware (used for logging/debugging).
    fn name(&self) -> &'static str;

    /// Priority determines execution order (lower = earlier for requests).
    /// Default is 0.
    fn priority(&self) -> i32 { 0 }

    /// Process incoming request. Return Next to continue, Stop to short-circuit.
    fn on_request(&self, req: Request, ctx: &mut Context) -> MiddlewareResult {
        MiddlewareResult::Next(req)
    }

    /// Process outgoing response (runs in reverse priority order).
    fn on_response(&self, res: Response, ctx: &Context) -> Response {
        res
    }
}
```

### Result Types

```rust
pub enum MiddlewareResult {
    /// Continue to next middleware with (possibly modified) request
    Next(Request),

    /// Short-circuit and return response immediately
    Stop(Response),
}
```

## Built-in Middleware

### Rate Limiting

Per-IP rate limiting with fixed-window algorithm. See [Rate Limiting](rate-limiting.md) for details.

| Setting | Description |
|---------|-------------|
| Priority | -100 (very early) |
| Config | `RATE_LIMIT`, `RATE_WINDOW` |
| Response | 429 Too Many Requests |

```bash
# 100 requests per minute per IP
RATE_LIMIT=100 RATE_WINDOW=60 docker compose up -d
```

**Response Headers (429 only):**

| Header | Description |
|--------|-------------|
| `X-RateLimit-Limit` | Maximum requests per window |
| `X-RateLimit-Remaining` | Always `0` (request blocked) |
| `X-RateLimit-Reset` | Seconds until window reset |
| `Retry-After` | Seconds to wait before retry |

**Example Response:**
```http
HTTP/1.1 429 Too Many Requests
Retry-After: 45
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 45
Content-Type: text/plain

429 Too Many Requests
```

### Access Logging

Structured JSON access logs for request/response tracking. See [Configuration](configuration.md#access_log) for settings.

| Setting | Description |
|---------|-------------|
| Priority | -90 (early request, late response) |
| Config | `ACCESS_LOG=1` |
| Target | `access` (tracing target) |

```bash
ACCESS_LOG=1 docker compose up -d
```

**Log Format:**
```json
{
  "ts": "2025-01-15T10:30:00.456Z",
  "level": "info",
  "type": "access",
  "msg": "GET /api/users 200",
  "ctx": {
    "service": "tokio_php",
    "request_id": "65bdbab40000",
    "trace_id": "0af7651916cd43dd8448eb211c80319c",
    "span_id": "b7ad6b7169203331"
  },
  "data": {
    "method": "GET",
    "path": "/api/users",
    "query": "page=1",
    "http": "HTTP/1.1",
    "status": 200,
    "bytes": 1234,
    "duration_ms": 5.25,
    "ip": "10.0.0.1",
    "ua": "curl/8.0",
    "referer": null,
    "xff": null
  }
}
```

### Compression

Brotli compression for text-based responses. See [Compression](compression.md) for details.

| Setting | Description |
|---------|-------------|
| Priority | 100 (very late) |
| Algorithm | Brotli (quality 4) |
| Config | Automatic |

**Compression Rules:**
- Client must send `Accept-Encoding: br`
- Response body: 256 bytes - 3 MB
- Content-Type must be compressible

**Supported MIME Types:**
- `text/html`, `text/css`, `text/plain`, `text/xml`, `text/javascript`
- `application/javascript`, `application/json`, `application/xml`
- `application/xhtml+xml`, `application/rss+xml`, `application/atom+xml`
- `application/manifest+json`, `application/ld+json`
- `image/svg+xml`
- `font/ttf`, `font/otf`, `application/vnd.ms-fontobject`

**Response Headers:**
```http
Content-Encoding: br
Vary: Accept-Encoding
```

### Static File Caching

Cache-Control headers for static assets. See [Static Caching](static-caching.md) for details.

| Setting | Description |
|---------|-------------|
| Priority | 50 |
| Config | `STATIC_CACHE_TTL` |

```bash
# Cache static files for 1 week
STATIC_CACHE_TTL=1w docker compose up -d
```

**TTL Values:**

| Value | Duration |
|-------|----------|
| `1s` | 1 second |
| `1m` | 1 minute (60 seconds) |
| `1h` | 1 hour (3600 seconds) |
| `1d` | 1 day (86400 seconds) |
| `1w` | 1 week (604800 seconds) |
| `1y` | 1 year (31536000 seconds) |
| `off` | No caching headers |

**Note:** There is no month unit. Use `30d` for approximately one month.

**Cacheable Extensions:**
- Images: `png`, `jpg`, `jpeg`, `gif`, `ico`, `webp`, `svg`, `avif`
- Fonts: `woff`, `woff2`, `ttf`, `otf`, `eot`
- Scripts: `css`, `js`, `mjs`
- Other: `json`, `xml`, `txt`, `pdf`, `map`

**Response Header:**
```http
Cache-Control: public, max-age=604800
```

### Custom Error Pages

Serve custom HTML pages for 4xx/5xx errors. See [Error Pages](error-pages.md) for setup.

| Setting | Description |
|---------|-------------|
| Priority | 90 |
| Config | `ERROR_PAGES_DIR` |

```bash
ERROR_PAGES_DIR=/var/www/html/errors docker compose up -d
```

**File Naming:**
- `404.html` - Not Found
- `500.html` - Internal Server Error
- `503.html` - Service Unavailable

**Behavior:**
- Only served when client sends `Accept: text/html`
- Only for 4xx/5xx responses with empty body
- Files cached in memory at startup
- Missing files fall back to default text response

## Request Context

Middleware shares data via the `Context` object. See [Distributed Tracing](distributed-tracing.md) for trace/span IDs and [Profiling](profiling.md) for timing.

```rust
pub struct Context {
    // Client info
    pub client_ip: IpAddr,
    pub trace_id: String,           // 32-char W3C trace ID
    pub span_id: String,            // 16-char span ID
    pub parent_span_id: Option<String>, // Parent span (if propagated)
    pub request_id: String,         // Short ID for logs

    // Timing
    pub started_at: Instant,

    // Request metadata
    pub http_version: HttpVersion,  // HTTP/1.0, HTTP/1.1, HTTP/2.0
    pub profiling: bool,
    pub accepts_html: bool,
    pub accepts_brotli: bool,

    // Private fields with accessor methods:
    // - response_headers: HashMap<String, String>
    // - values: HashMap<String, Box<dyn Any + Send + Sync>>
}

impl Context {
    /// Set a typed value for middleware communication
    pub fn set<T: Send + Sync + 'static>(&mut self, key: &str, value: T);

    /// Get a typed value
    pub fn get<T: 'static>(&self, key: &str) -> Option<&T>;

    /// Add a response header
    pub fn set_response_header(&mut self, name: impl ToString, value: impl ToString);

    /// Get elapsed time in milliseconds
    pub fn elapsed_ms(&self) -> f64;
}
```

### Using Context in Middleware

```rust
impl Middleware for MyMiddleware {
    fn name(&self) -> &'static str { "my_middleware" }

    fn on_request(&self, req: Request, ctx: &mut Context) -> MiddlewareResult {
        // Store typed data for later middleware
        ctx.set("user_id", 12345u64);

        // Add response header
        ctx.set_response_header("X-Custom", "value");

        MiddlewareResult::Next(req)
    }

    fn on_response(&self, res: Response, ctx: &Context) -> Response {
        // Access stored data (type-safe)
        if let Some(user_id) = ctx.get::<u64>("user_id") {
            println!("User ID: {}", user_id);
        }

        // Access timing
        let elapsed = ctx.elapsed_ms();
        println!("Request took {} ms", elapsed);

        res
    }
}
```

## Priority Guidelines

| Range | Category | Examples |
|-------|----------|----------|
| -100..-50 | Security | Rate limiting, authentication |
| -50..0 | Logging | Access logs, request tracing |
| 0..50 | Request modification | Header injection, path rewriting |
| 50..100 | Response modification | Caching headers, error pages |
| 100+ | Encoding | Compression |

## Middleware Chain

The `MiddlewareChain` manages middleware execution:

```rust
let mut chain = MiddlewareChain::new();

// Add middleware (auto-sorted by priority)
chain.add(Box::new(RateLimitMiddleware::new(100, 60)));
chain.add(Box::new(AccessLogMiddleware::new()));
chain.add(Box::new(CompressionMiddleware::new()));

// Process request through chain
let result = chain.process_request(request, &mut context);

match result {
    MiddlewareResult::Next(req) => {
        // Continue to handler
        let response = handle_request(req);

        // Process response (reverse order)
        let final_response = chain.process_response(response, &mut context);
    }
    MiddlewareResult::Stop(res) => {
        // Return early response (e.g., 429)
        return res;
    }
}
```

## Configuration Reference

See [Configuration](configuration.md) for full environment variable reference.

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT` | `0` | Max requests per IP (0 = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window in seconds |
| `ACCESS_LOG` | `0` | Enable access logs |
| `STATIC_CACHE_TTL` | `1d` | Static file cache duration |
| `ERROR_PAGES_DIR` | _(empty)_ | Custom error pages directory |
| `PROFILE` | `0` | Enable profiling headers |

## See Also

- [Architecture](architecture.md) - System design overview
- [Rate Limiting](rate-limiting.md) - Detailed rate limiting documentation
- [Compression](compression.md) - Brotli compression settings
- [Error Pages](error-pages.md) - Custom error page setup
- [Static Caching](static-caching.md) - Static file caching
- [Logging](logging.md) - Access log format
- [Distributed Tracing](distributed-tracing.md) - W3C Trace Context
- [Profiling](profiling.md) - Request timing headers
- [Configuration](configuration.md) - All environment variables
