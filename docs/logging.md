# Logging

tokio_php uses structured JSON logging for easy parsing and aggregation. This document covers the log format, configuration, and how to configure PHP applications (Monolog) to use the same format.

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ACCESS_LOG` | `0` | Enable access logs (`1` = enabled) |
| `RUST_LOG` | `tokio_php=info` | Log level: `trace`, `debug`, `info`, `warn`, `error` |

```bash
# Enable access logs
ACCESS_LOG=1 docker compose up -d

# Debug logging
RUST_LOG=tokio_php=debug docker compose up -d

# Quiet mode (errors only)
RUST_LOG=tokio_php=error docker compose up -d
```

## Log Format

All logs use a unified JSON format:

```json
{
  "ts": "2025-01-15T10:30:00.123Z",
  "level": "info",
  "type": "app",
  "msg": "Server listening on http://0.0.0.0:8080",
  "ctx": {
    "service": "tokio_php",
    "request_id": "65bdbab40000"
  },
  "data": {}
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `ts` | string | ISO 8601 timestamp with milliseconds, UTC |
| `level` | string | `debug`, `info`, `warn`, `error` |
| `type` | string | Log type: `app`, `access`, `error` |
| `msg` | string | Short human-readable message |
| `ctx` | object | Context: service name, request_id, trace_id, etc. |
| `data` | object | Type-specific structured data |

### Log Types

| Type | Description |
|------|-------------|
| `app` | Application events (startup, shutdown, config) |
| `access` | HTTP request/response logs |
| `error` | Errors and exceptions |

## Access Logs

Enable with `ACCESS_LOG=1`. Each request generates one log entry.

### Async I/O Architecture

Access logs use **non-blocking async I/O** via a background task:

```
Request Handler                    Background Task
      │                                  │
      │  tx.send(log_entry)              │
      │ ─────────────────────►           │
      │  (~10ns, non-blocking)           │
      │                                  │
      │  Ok(response)                    │  tokio::io::stdout()
      │ ──────► Client                   │  .write_all().await
      │                                  │  .flush().await
      │                                  │
```

- **Channel-based**: `mpsc::unbounded_channel` for log entries
- **Non-blocking**: Request handler returns immediately after `send()`
- **Async stdout**: Uses `tokio::io::stdout()` with `AsyncWriteExt`
- **Zero latency impact**: Logging doesn't block response delivery

This architecture ensures access logging adds **~10ns overhead** per request instead of blocking on I/O.

### Log Format

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
    "referer": "https://example.com",
    "xff": "203.0.113.1",
    "tls": "TLSv1.3"
  }
}
```

### Access Log Fields

| Field | Type | Description |
|-------|------|-------------|
| `method` | string | HTTP method (GET, POST, etc.) |
| `path` | string | Request path |
| `query` | string? | Query string (without `?`) |
| `http` | string | HTTP version |
| `status` | number | Response status code |
| `bytes` | number | Response body size |
| `duration_ms` | number | Request duration in milliseconds |
| `ip` | string | Client IP address |
| `ua` | string? | User-Agent header |
| `referer` | string? | Referer header |
| `xff` | string? | X-Forwarded-For header |
| `tls` | string? | TLS protocol version (HTTPS only) |

## PHP Application Logging (Monolog)

To maintain consistent log format across tokio_php and your PHP application, use this Monolog formatter.

### TokioPhpFormatter

Create `src/Logging/TokioPhpFormatter.php`:

```php
<?php

namespace App\Logging;

use Monolog\Formatter\JsonFormatter;
use Monolog\LogRecord;

/**
 * Monolog formatter matching tokio_php log format.
 *
 * Output format:
 * {"ts":"2025-01-15T10:30:00.123Z","level":"info","type":"app","msg":"...","ctx":{...},"data":{...}}
 */
class TokioPhpFormatter extends JsonFormatter
{
    private string $service;

    public function __construct(string $service = 'app')
    {
        parent::__construct();
        $this->service = $service;
    }

    public function format(LogRecord $record): string
    {
        $context = $record->context;
        $extra = $record->extra;

        // Build ctx object
        $ctx = [
            'service' => $this->service,
        ];

        // Add request_id from $_SERVER if available
        if (isset($_SERVER['TOKIO_REQUEST_ID'])) {
            $ctx['request_id'] = $_SERVER['TOKIO_REQUEST_ID'];
        } elseif (isset($_SERVER['HTTP_X_REQUEST_ID'])) {
            $ctx['request_id'] = $_SERVER['HTTP_X_REQUEST_ID'];
        }

        // Add trace context if available
        if (isset($_SERVER['TRACE_ID'])) {
            $ctx['trace_id'] = $_SERVER['TRACE_ID'];
        }
        if (isset($_SERVER['SPAN_ID'])) {
            $ctx['span_id'] = $_SERVER['SPAN_ID'];
        }

        // Move known context fields to ctx
        foreach (['request_id', 'trace_id', 'span_id', 'user_id'] as $field) {
            if (isset($context[$field])) {
                $ctx[$field] = $context[$field];
                unset($context[$field]);
            }
        }

        // Determine log type
        $type = $context['type'] ?? 'app';
        unset($context['type']);

        // Build data object from remaining context
        $data = array_merge($context, $extra);

        // Handle exception
        if (isset($data['exception']) && $data['exception'] instanceof \Throwable) {
            $e = $data['exception'];
            $data['exception'] = [
                'class' => get_class($e),
                'message' => $e->getMessage(),
                'code' => $e->getCode(),
                'file' => $e->getFile(),
                'line' => $e->getLine(),
                'trace' => array_slice($e->getTrace(), 0, 10),
            ];
        }

        $output = [
            'ts' => $record->datetime->format('Y-m-d\TH:i:s.v\Z'),
            'level' => strtolower($record->level->name),
            'type' => $type,
            'msg' => $record->message,
            'ctx' => $ctx,
            'data' => (object)$data, // Force {} for empty
        ];

        return $this->toJson($output) . "\n";
    }
}
```

### Laravel Integration

**config/logging.php:**

```php
<?php

return [
    'default' => env('LOG_CHANNEL', 'tokio'),

    'channels' => [
        'tokio' => [
            'driver' => 'monolog',
            'handler' => Monolog\Handler\StreamHandler::class,
            'with' => [
                'stream' => 'php://stderr',
            ],
            'formatter' => App\Logging\TokioPhpFormatter::class,
            'formatter_with' => [
                'service' => env('APP_NAME', 'laravel'),
            ],
        ],

        // Keep other channels for development
        'stack' => [
            'driver' => 'stack',
            'channels' => ['tokio'],
        ],
    ],
];
```

**Usage:**

```php
use Illuminate\Support\Facades\Log;

// Simple message
Log::info('User logged in');
// {"ts":"...","level":"info","type":"app","msg":"User logged in","ctx":{"service":"laravel","request_id":"..."},"data":{}}

// With context data
Log::info('Order created', ['order_id' => 123, 'amount' => 99.99]);
// {"ts":"...","level":"info","type":"app","msg":"Order created","ctx":{...},"data":{"order_id":123,"amount":99.99}}

// Error with exception
try {
    // ...
} catch (\Exception $e) {
    Log::error('Payment failed', ['exception' => $e, 'order_id' => 123]);
}

// Custom type (for filtering)
Log::info('API request', ['type' => 'api', 'endpoint' => '/users']);
// {"ts":"...","level":"info","type":"api","msg":"API request","ctx":{...},"data":{"endpoint":"/users"}}
```

### Symfony Integration

**config/packages/monolog.yaml:**

```yaml
monolog:
    handlers:
        main:
            type: stream
            path: "php://stderr"
            level: debug
            formatter: App\Logging\TokioPhpFormatter
```

**config/services.yaml:**

```yaml
services:
    App\Logging\TokioPhpFormatter:
        arguments:
            $service: '%env(APP_NAME)%'
```

### Standalone PHP

```php
<?php

require 'vendor/autoload.php';

use Monolog\Logger;
use Monolog\Handler\StreamHandler;
use App\Logging\TokioPhpFormatter;

$log = new Logger('myapp');
$handler = new StreamHandler('php://stderr', Logger::DEBUG);
$handler->setFormatter(new TokioPhpFormatter('myapp'));
$log->pushHandler($handler);

$log->info('Application started');
$log->error('Database connection failed', ['host' => 'db.local', 'port' => 5432]);
```

## Log Filtering

### With jq

```bash
# All logs
docker compose logs -f

# Access logs only
docker compose logs -f | jq -c 'select(.type == "access")'

# Errors only
docker compose logs -f | jq -c 'select(.level == "error")'

# Slow requests (> 100ms)
docker compose logs -f | jq -c 'select(.type == "access" and .data.duration_ms > 100)'

# Specific request by ID
docker compose logs | jq -c 'select(.ctx.request_id == "65bdbab40000")'

# 5xx errors
docker compose logs -f | jq -c 'select(.type == "access" and .data.status >= 500)'

# By trace ID (distributed tracing)
docker compose logs | jq -c 'select(.ctx.trace_id == "0af7651916cd43dd8448eb211c80319c")'
```

### With grep

```bash
# Quick filter by type
docker compose logs -f | grep '"type":"error"'

# Filter by status
docker compose logs -f | grep '"status":500'
```

## Log Aggregation

The JSON format is compatible with popular log aggregation tools:

| Tool | Configuration |
|------|---------------|
| **Fluentd** | Use `fluent-plugin-json` parser |
| **Fluent Bit** | `Parser json` in config |
| **Logstash** | `json` codec |
| **Vector** | `json` codec |
| **Loki** | Use `json` pipeline stage |
| **CloudWatch** | JSON logs auto-parsed |
| **Datadog** | JSON logs auto-parsed |
| **Elasticsearch** | Direct JSON ingestion |

### Fluent Bit Example

```ini
[INPUT]
    Name              forward
    Listen            0.0.0.0
    Port              24224

[FILTER]
    Name              parser
    Match             *
    Key_Name          log
    Parser            json

[OUTPUT]
    Name              es
    Match             *
    Host              elasticsearch
    Port              9200
    Index             tokio_php
    Type              _doc
```

### Docker Compose with Fluent Bit

```yaml
services:
  tokio_php:
    image: diolektor/tokio_php
    logging:
      driver: fluentd
      options:
        fluentd-address: localhost:24224
        tag: tokio_php
```

## Best Practices

1. **Always enable ACCESS_LOG in production** — essential for debugging and monitoring
2. **Use structured data** — put variables in `data`, not in message string
3. **Include request_id** — enables request correlation across services
4. **Use consistent service names** — makes filtering easier
5. **Keep messages short** — details go in `data` object

### Good

```php
Log::info('Order created', ['order_id' => 123, 'user_id' => 456, 'amount' => 99.99]);
```

### Bad

```php
Log::info("Order 123 created by user 456 for $99.99");  // Hard to parse
```

## See Also

- [Distributed Tracing](distributed-tracing.md) — W3C Trace Context support
- [Configuration](configuration.md) — Environment variables reference
- [Internal Server](internal-server.md) — Metrics endpoint
- [Profiling](profiling.md) — Request timing headers
