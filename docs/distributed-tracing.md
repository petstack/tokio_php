# Distributed Tracing

tokio_php supports W3C Trace Context for distributed tracing, enabling request correlation across microservices.

## Quick Start

```bash
# Request without trace context (new trace generated)
curl -I http://localhost:8080/index.php
# traceparent: 00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01

# Request with trace context (propagated)
curl -H "traceparent: 00-11111111111111111111111111111111-2222222222222222-01" \
     -I http://localhost:8080/index.php
# traceparent: 00-11111111111111111111111111111111-a1b2c3d4e5f67890-01
```

## W3C Trace Context Format

```
traceparent: {version}-{trace-id}-{parent-id}-{flags}
             00       -32 hex   -16 hex    -01
```

| Field | Length | Description |
|-------|--------|-------------|
| version | 2 hex | Always `00` for current spec |
| trace-id | 32 hex | Unique ID for the entire trace (16 bytes) |
| parent-id | 16 hex | Span ID of the caller (8 bytes) |
| flags | 2 hex | `01` = sampled, `00` = not sampled |

## How It Works

```
Gateway (trace_id=abc, span_id=001)
    │
    │ traceparent: 00-abc...-001-01
    ▼
tokio_php (trace_id=abc, span_id=002, parent=001)
    │
    │ traceparent: 00-abc...-002-01
    ▼
Backend API (trace_id=abc, span_id=003, parent=002)
```

1. **Incoming request without traceparent**: New trace_id and span_id generated
2. **Incoming request with traceparent**: trace_id preserved, new span_id generated, parent_span_id set

## PHP Integration

Access trace context in PHP via `$_SERVER`:

```php
<?php

// Trace identifiers
$traceId = $_SERVER['TRACE_ID'];           // 32-char hex
$spanId = $_SERVER['SPAN_ID'];             // 16-char hex
$parentSpanId = $_SERVER['PARENT_SPAN_ID'] ?? null; // 16-char hex or null

// Full traceparent header
$traceparent = $_SERVER['HTTP_TRACEPARENT'];

// Example: Add to outgoing requests
$ch = curl_init('https://api.example.com/data');
curl_setopt($ch, CURLOPT_HTTPHEADER, [
    "traceparent: 00-{$traceId}-" . bin2hex(random_bytes(8)) . "-01"
]);
```

### Logging with Trace Context

```php
<?php

function log_with_trace(string $message, array $data = []): void {
    $entry = [
        'ts' => date('c'),
        'msg' => $message,
        'trace_id' => $_SERVER['TRACE_ID'],
        'span_id' => $_SERVER['SPAN_ID'],
        'data' => $data,
    ];
    error_log(json_encode($entry));
}

log_with_trace('Processing order', ['order_id' => 12345]);
```

### Creating Child Spans

```php
<?php

function create_child_traceparent(): string {
    $traceId = $_SERVER['TRACE_ID'];
    $newSpanId = bin2hex(random_bytes(8));
    return "00-{$traceId}-{$newSpanId}-01";
}

// Use when calling external services
$headers = [
    'traceparent: ' . create_child_traceparent()
];
```

## Response Headers

tokio_php adds these headers to all responses:

| Header | Description |
|--------|-------------|
| `traceparent` | W3C trace context with this request's span_id |
| `x-request-id` | Short ID for logs: `{trace_id[0:12]}-{span_id[0:4]}` |

## Access Logs

With `ACCESS_LOG=1`, logs include trace context:

```json
{
  "ts": "2025-12-29T15:04:05.123Z",
  "level": "info",
  "type": "access",
  "msg": "GET /api/users 200",
  "ctx": {
    "service": "tokio_php",
    "request_id": "0af7651916cd-b7ad",
    "trace_id": "0af7651916cd43dd8448eb211c80319c",
    "span_id": "b7ad6b7169203331"
  },
  "data": {
    "method": "GET",
    "path": "/api/users",
    "status": 200,
    "duration_ms": 12.5
  }
}
```

## Integration with Tracing Systems

### Jaeger

```yaml
# docker-compose.yml
services:
  jaeger:
    image: jaegertracing/all-in-one:latest
    ports:
      - "16686:16686"  # UI
      - "14268:14268"  # Collector
```

Send traces from PHP:
```php
<?php
// Using OpenTelemetry PHP SDK
use OpenTelemetry\SDK\Trace\TracerProvider;

$tracer = TracerProvider::getDefault()->getTracer('my-app');
$span = $tracer->spanBuilder('process-order')
    ->setParent(/* extract from $_SERVER */)
    ->startSpan();
```

### Zipkin

```yaml
services:
  zipkin:
    image: openzipkin/zipkin:latest
    ports:
      - "9411:9411"
```

### Grafana Tempo

```yaml
services:
  tempo:
    image: grafana/tempo:latest
    ports:
      - "3200:3200"
```

## Log Correlation

Query logs by trace_id across all services:

```bash
# Elasticsearch/OpenSearch
GET /logs/_search
{
  "query": {
    "term": { "ctx.trace_id": "0af7651916cd43dd8448eb211c80319c" }
  }
}

# Loki (Grafana)
{app="tokio_php"} | json | trace_id="0af7651916cd43dd8448eb211c80319c"
```

## Best Practices

### 1. Always Propagate Trace Context

```php
<?php

function call_api(string $url, array $data): array {
    $ch = curl_init($url);
    curl_setopt_array($ch, [
        CURLOPT_POST => true,
        CURLOPT_POSTFIELDS => json_encode($data),
        CURLOPT_HTTPHEADER => [
            'Content-Type: application/json',
            'traceparent: ' . create_child_traceparent(),
        ],
        CURLOPT_RETURNTRANSFER => true,
    ]);
    return json_decode(curl_exec($ch), true);
}
```

### 2. Include Trace ID in Error Responses

```php
<?php

function json_error(int $code, string $message): never {
    http_response_code($code);
    header('Content-Type: application/json');
    echo json_encode([
        'error' => $message,
        'trace_id' => $_SERVER['TRACE_ID'],
    ]);
    exit;
}
```

### 3. Log Trace Context on Errors

```php
<?php

set_exception_handler(function (Throwable $e) {
    error_log(json_encode([
        'level' => 'error',
        'msg' => $e->getMessage(),
        'trace_id' => $_SERVER['TRACE_ID'] ?? 'unknown',
        'span_id' => $_SERVER['SPAN_ID'] ?? 'unknown',
        'file' => $e->getFile(),
        'line' => $e->getLine(),
    ]));
});
```

## $_SERVER Variables Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `TRACE_ID` | 32-char trace identifier | `0af7651916cd43dd8448eb211c80319c` |
| `SPAN_ID` | 16-char span identifier | `b7ad6b7169203331` |
| `PARENT_SPAN_ID` | 16-char parent span (if propagated) | `a1b2c3d4e5f67890` |
| `HTTP_TRACEPARENT` | Full W3C traceparent header | `00-0af7...-b7ad...-01` |

## See Also

- [Configuration](configuration.md) - ACCESS_LOG environment variable
- [Logging](logging.md) - Log format with trace context
- [Middleware](middleware.md) - Access logging middleware
- [Profiling](profiling.md) - Request timing with trace context
- [W3C Trace Context Specification](https://www.w3.org/TR/trace-context/)
- [OpenTelemetry PHP](https://opentelemetry.io/docs/instrumentation/php/)
- [Jaeger Documentation](https://www.jaegertracing.io/docs/)
