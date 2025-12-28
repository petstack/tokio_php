# Rate Limiting

Per-IP rate limiting protects your application from abuse, ensures fair resource distribution, and prevents individual clients from overwhelming the server.

## Quick Start

```bash
# 100 requests per minute per IP
RATE_LIMIT=100 RATE_WINDOW=60 docker compose up -d

# Verify it's enabled
docker compose logs | grep "Rate limiting"
# Rate limiting enabled: 100 requests per 60 seconds per IP
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT` | `0` | Max requests per IP per window (0 = disabled) |
| `RATE_WINDOW` | `60` | Window duration in seconds |

### Examples

```bash
# Disabled (default)
RATE_LIMIT=0

# Standard API rate limit: 100 req/min
RATE_LIMIT=100
RATE_WINDOW=60

# High-traffic site: 1000 req/min
RATE_LIMIT=1000
RATE_WINDOW=60

# Strict protection: 10 req/10s
RATE_LIMIT=10
RATE_WINDOW=10

# Hourly limit: 5000 req/hour
RATE_LIMIT=5000
RATE_WINDOW=3600
```

## Response Headers

Every response includes rate limit information when rate limiting is enabled.

### Successful Request (within limit)

```
HTTP/1.1 200 OK
X-Request-ID: 65bdbab40000
...
```

### Rate Limited Request

```
HTTP/1.1 429 Too Many Requests
Content-Type: text/plain
Retry-After: 45
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 45
X-Request-ID: 65bdbab40001

429 Too Many Requests
```

### Header Reference

| Header | Description |
|--------|-------------|
| `Retry-After` | Seconds until client should retry (RFC 7231) |
| `X-RateLimit-Limit` | Maximum requests allowed per window |
| `X-RateLimit-Remaining` | Requests remaining in current window |
| `X-RateLimit-Reset` | Seconds until current window resets |

## Algorithm

tokio_php uses a **fixed window** algorithm:

```
Window 1 (0-60s)      Window 2 (60-120s)
├─────────────────────┼─────────────────────┤
│ ████████░░ 80/100   │ ██░░░░░░░░ 20/100   │
│ requests allowed    │ counter reset       │
└─────────────────────┴─────────────────────┘
```

### How It Works

1. First request from IP starts a new window
2. Each request increments the counter
3. When counter reaches `RATE_LIMIT`, subsequent requests get 429
4. When `RATE_WINDOW` seconds pass, counter resets to 0

### Characteristics

| Property | Behavior |
|----------|----------|
| Storage | In-memory (HashMap with RwLock) |
| Persistence | Resets on server restart |
| Granularity | Per IP address |
| Precision | Second-level |

## Rate Limiting vs Queue Capacity

tokio_php has two protection mechanisms that work together:

```
Request Flow:

  Client Request
        │
        ▼
  ┌─────────────┐
  │ Rate Limit  │ ──429──► Client
  │  (per-IP)   │
  └──────┬──────┘
         │ OK
         ▼
  ┌─────────────┐
  │    Queue    │ ──503──► Client
  │  (global)   │
  └──────┬──────┘
         │ OK
         ▼
  ┌─────────────┐
  │   Worker    │
  │    Pool     │
  └──────┬──────┘
         │
         ▼
     Response
```

### Comparison

| Feature | Rate Limit | Queue Capacity |
|---------|------------|----------------|
| Scope | Per-IP | Global |
| Response | 429 Too Many Requests | 503 Service Unavailable |
| Purpose | Fairness, abuse prevention | Server overload protection |
| Config | `RATE_LIMIT`, `RATE_WINDOW` | `QUEUE_CAPACITY` |
| Header | `Retry-After` | `Retry-After: 1` |

### Recommended Configuration

```bash
# Production: rate limit + queue protection
RATE_LIMIT=100      # 100 req/min per IP
RATE_WINDOW=60
QUEUE_CAPACITY=1000 # 1000 pending requests max
PHP_WORKERS=8

docker compose up -d
```

## Use Cases

### API Server

Protect your API from excessive requests:

```bash
# 60 requests per minute (1 req/sec average)
RATE_LIMIT=60
RATE_WINDOW=60
```

### Public Website

Allow burst traffic but prevent abuse:

```bash
# 200 requests per minute
RATE_LIMIT=200
RATE_WINDOW=60
```

### Login/Authentication Endpoint

Strict limits for security-sensitive endpoints:

```bash
# 5 attempts per minute (brute-force protection)
RATE_LIMIT=5
RATE_WINDOW=60
```

Note: This applies globally. For endpoint-specific limits, implement in PHP:

```php
<?php
// Custom rate limiting for login
$ip = $_SERVER['REMOTE_ADDR'];
$key = "login_attempts:$ip";

// Use Redis/Memcached for production
$attempts = apcu_fetch($key) ?: 0;
if ($attempts >= 5) {
    http_response_code(429);
    header('Retry-After: 60');
    exit('Too many login attempts');
}
apcu_store($key, $attempts + 1, 60);
```

### Development/Testing

Disable rate limiting:

```bash
RATE_LIMIT=0 docker compose up -d
```

## Monitoring

### Logs

Rate limit events appear in access logs:

```bash
# View rate-limited requests
docker compose logs | jq -c 'select(.data.status == 429)'
```

### Metrics

Monitor rate limiting via `/metrics` endpoint:

```bash
curl http://localhost:9090/metrics | grep responses
# tokio_php_responses_total{status="4xx"} 150
```

## Client Handling

### Retry Logic

Clients should respect `Retry-After` header:

```javascript
// JavaScript example
async function fetchWithRetry(url, maxRetries = 3) {
  for (let i = 0; i < maxRetries; i++) {
    const response = await fetch(url);

    if (response.status === 429) {
      const retryAfter = response.headers.get('Retry-After') || 60;
      console.log(`Rate limited. Waiting ${retryAfter}s...`);
      await new Promise(r => setTimeout(r, retryAfter * 1000));
      continue;
    }

    return response;
  }
  throw new Error('Max retries exceeded');
}
```

```python
# Python example
import requests
import time

def fetch_with_retry(url, max_retries=3):
    for _ in range(max_retries):
        response = requests.get(url)

        if response.status_code == 429:
            retry_after = int(response.headers.get('Retry-After', 60))
            print(f'Rate limited. Waiting {retry_after}s...')
            time.sleep(retry_after)
            continue

        return response

    raise Exception('Max retries exceeded')
```

### Exponential Backoff

For production clients, combine `Retry-After` with exponential backoff:

```python
import time
import random

def fetch_with_backoff(url, max_retries=5):
    for attempt in range(max_retries):
        response = requests.get(url)

        if response.status_code == 429:
            retry_after = int(response.headers.get('Retry-After', 1))
            # Add jitter to prevent thundering herd
            jitter = random.uniform(0, 0.1 * retry_after)
            wait_time = retry_after + jitter
            time.sleep(wait_time)
            continue

        return response

    raise Exception('Max retries exceeded')
```

## Limitations

### Current Limitations

| Limitation | Description | Workaround |
|------------|-------------|------------|
| In-memory only | State lost on restart | Accept or use external store |
| No clustering | Each instance has separate counters | Use load balancer rate limiting |
| IP-based only | No user/API key support | Implement in application |
| Fixed window | Burst at window boundaries | Use shorter windows |

### Not Supported (Yet)

- Sliding window algorithm
- Token bucket algorithm
- Redis/external storage backend
- Per-endpoint limits
- User/API key based limits
- Whitelist/blacklist IPs

### Production Alternatives

For advanced rate limiting, consider:

| Solution | Use Case |
|----------|----------|
| nginx `limit_req` | High-performance, before app |
| Cloudflare Rate Limiting | Edge, DDoS protection |
| Kong/API Gateway | API management, complex rules |
| Redis + Lua | Distributed, sliding window |

## Best Practices

1. **Start conservative** - Begin with strict limits, relax as needed
2. **Monitor 429 rates** - High 429% may indicate limits too strict
3. **Document limits** - Tell API consumers about rate limits
4. **Use with queue capacity** - Defense in depth
5. **Consider window size** - Shorter windows = smoother traffic
6. **Test under load** - Verify limits work as expected

## Troubleshooting

### Rate Limiting Not Working

```bash
# Check if enabled
docker compose logs | grep "Rate limiting enabled"

# Verify environment variables
docker compose exec tokio_php env | grep RATE
```

### Too Many 429 Responses

```bash
# Increase limit
RATE_LIMIT=200 docker compose up -d

# Or increase window
RATE_WINDOW=120 docker compose up -d
```

### Memory Growth (Many Unique IPs)

The rate limiter stores counters per IP. Under normal operation, expired entries are cleaned up. If memory grows:

```bash
# Restart to clear counters
docker compose restart

# Or reduce window to expire entries faster
RATE_WINDOW=30 docker compose up -d
```
