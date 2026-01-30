# Observability

Production-grade observability for tokio_php with Prometheus metrics, Grafana dashboards, and optional OpenTelemetry distributed tracing.

## Quick Start

```bash
# Start tokio_php with monitoring stack (Prometheus + Grafana + Jaeger)
docker compose --profile monitoring up -d

# Access:
# - App: http://localhost:8080
# - Metrics: http://localhost:9090/metrics
# - Prometheus: http://localhost:9091
# - Grafana: http://localhost:3000 (admin/admin)
# - Jaeger UI: http://localhost:16686
```

### Enable OpenTelemetry Tracing

```bash
# Build with otel feature and enable tracing
CARGO_FEATURES=otel docker compose build

OTEL_ENABLED=1 docker compose --profile monitoring up -d

# Make requests
curl http://localhost:8080/index.php

# View traces in Jaeger UI: http://localhost:16686
```

## Prometheus Metrics

The internal server (`INTERNAL_ADDR`) exposes Prometheus-compatible metrics at `/metrics`.

### Server Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_uptime_seconds` | gauge | Server uptime in seconds |
| `tokio_php_requests_per_second` | gauge | Lifetime average RPS |
| `tokio_php_response_time_avg_seconds` | gauge | Average response time |
| `tokio_php_active_connections` | gauge | Current active connections |
| `tokio_php_pending_requests` | gauge | Requests waiting in queue |
| `tokio_php_dropped_requests` | counter | Requests dropped (queue full) |

### Request Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `tokio_php_requests_total` | counter | `method` | Total requests by HTTP method |
| `tokio_php_responses_total` | counter | `status` | Total responses by status class |

**Labels:**
- `method`: `GET`, `POST`, `HEAD`, `PUT`, `DELETE`, `OPTIONS`, `PATCH`, `OTHER`
- `status`: `2xx`, `3xx`, `4xx`, `5xx`

### SSE Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_sse_active_connections` | gauge | Current active SSE connections |
| `tokio_php_sse_connections_total` | counter | Total SSE connections |
| `tokio_php_sse_chunks_total` | counter | Total SSE chunks sent |
| `tokio_php_sse_bytes_total` | counter | Total SSE bytes sent |

### System Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `node_load1` | gauge | 1-minute load average |
| `node_load5` | gauge | 5-minute load average |
| `node_load15` | gauge | 15-minute load average |
| `node_memory_MemTotal_bytes` | gauge | Total memory in bytes |
| `node_memory_MemAvailable_bytes` | gauge | Available memory in bytes |
| `node_memory_MemUsed_bytes` | gauge | Used memory in bytes |
| `tokio_php_memory_usage_percent` | gauge | Memory usage percentage |

## Monitoring Stack

### Docker Compose Setup

The monitoring stack includes Prometheus and Grafana with pre-configured dashboards.

```bash
# Start with monitoring profile
docker compose --profile monitoring up -d

# Stop monitoring
docker compose --profile monitoring down
```

### Components

| Service | Port | Description |
|---------|------|-------------|
| `prometheus` | 9091 | Metrics collection and alerting |
| `grafana` | 3000 | Visualization and dashboards |
| `jaeger` | 16686 | Distributed tracing UI |
| `jaeger` | 4317 | OTLP gRPC endpoint |

### Configuration Files

```
deploy/
├── prometheus/
│   ├── prometheus.yml      # Scrape configuration
│   └── alerts.yml          # Alerting rules
└── grafana/
    ├── provisioning/
    │   ├── datasources/
    │   │   └── datasource.yml  # Prometheus datasource
    │   └── dashboards/
    │       └── dashboard.yml   # Dashboard provisioning
    └── tokio-php-dashboard.json  # Pre-built dashboard
```

### Grafana Dashboard

The pre-built dashboard (`deploy/grafana/tokio-php-dashboard.json`) includes:

| Panel | Description |
|-------|-------------|
| Request Rate | Requests per second |
| Response Time | Average response time |
| Active Connections | Current HTTP connections |
| Request Queue | Pending requests in queue |
| Error Rate | 4xx and 5xx responses |
| Requests by Method | GET, POST, etc. breakdown |
| Memory Usage | System memory utilization |
| Load Average | 1m, 5m, 15m load |
| Active SSE | Current SSE connections |
| SSE Throughput | SSE bytes per second |
| SSE Chunks Rate | SSE chunks per second |

### Prometheus Scrape Config

```yaml
# deploy/prometheus/prometheus.yml
scrape_configs:
  - job_name: 'tokio_php'
    static_configs:
      - targets: ['tokio_php:9090']
    scrape_interval: 5s
    metrics_path: /metrics
```

## Alerting Rules

Import `deploy/prometheus/alerts.yml` for production alerts.

### Alert Rules

| Alert | Condition | Severity |
|-------|-----------|----------|
| TokioPhpDown | Instance unreachable > 1m | critical |
| TokioPhpHighLatency | Avg response > 1s for 5m | warning |
| TokioPhpHighErrorRate | 5xx > 5% for 5m | warning |
| TokioPhpQueueSaturated | Queue > 80% for 2m | warning |
| TokioPhpHighMemory | Memory > 90% for 10m | warning |
| TokioPhpNoTraffic | No requests for 10m | warning |

### Example Alert Configuration

```yaml
# deploy/prometheus/alerts.yml
groups:
  - name: tokio_php
    rules:
      - alert: TokioPhpDown
        expr: up{job="tokio_php"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "tokio_php is down"

      - alert: TokioPhpHighErrorRate
        expr: |
          sum(rate(tokio_php_responses_total{status="5xx"}[5m]))
          / sum(rate(tokio_php_responses_total[5m])) > 0.05
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High 5xx error rate (> 5%)"
```

## PromQL Examples

### Request Rate
```promql
sum(rate(tokio_php_requests_total[5m]))
```

### Requests by Method
```promql
sum(rate(tokio_php_requests_total[5m])) by (method)
```

### Error Rate (%)
```promql
sum(rate(tokio_php_responses_total{status=~"4xx|5xx"}[5m]))
/ sum(rate(tokio_php_responses_total[5m])) * 100
```

### 5xx Error Rate
```promql
sum(rate(tokio_php_responses_total{status="5xx"}[5m]))
/ sum(rate(tokio_php_responses_total[5m])) * 100
```

### Average Response Time
```promql
tokio_php_response_time_avg_seconds
```

### Queue Utilization
```promql
tokio_php_pending_requests
```

### SSE Connection Rate
```promql
rate(tokio_php_sse_connections_total[5m])
```

### SSE Throughput (bytes/sec)
```promql
rate(tokio_php_sse_bytes_total[1m])
```

### Memory Usage
```promql
tokio_php_memory_usage_percent
```

## OpenTelemetry Integration

Enable distributed tracing with the `otel` Cargo feature.

### Build

```bash
# Local build
cargo build --release --features otel

# Docker build
CARGO_FEATURES=otel docker compose build
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OTEL_ENABLED` | `0` | Enable tracing (`1` = enabled) |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP gRPC endpoint |
| `OTEL_SERVICE_NAME` | `tokio_php` | Service name |
| `OTEL_SERVICE_VERSION` | _(from Cargo)_ | Service version |
| `OTEL_ENVIRONMENT` | `development` | Deployment environment |
| `OTEL_SAMPLING_RATIO` | `1.0` | Sampling ratio (0.0-1.0) |

### Example with Jaeger

```bash
# Build with otel feature
CARGO_FEATURES=otel docker compose build

# Start with monitoring profile (includes Jaeger)
OTEL_ENABLED=1 \
OTEL_SERVICE_NAME=my-php-app \
docker compose --profile monitoring up -d

# Make requests
curl http://localhost:8080/index.php
curl http://localhost:8080/index.php?page=1

# View traces at http://localhost:16686
# Select Service: "my-php-app" → Find Traces
```

### Standalone Jaeger (without monitoring profile)

```bash
# Start Jaeger only
docker compose --profile tracing up -d jaeger

# Start tokio_php with external Jaeger
OTEL_ENABLED=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://jaeger:4317 \
docker compose up -d
```

### Sampling Recommendations

| Environment | Ratio | Description |
|-------------|-------|-------------|
| Development | `1.0` | Trace all requests |
| Staging | `0.5` | 50% sampling |
| Production | `0.1` | 10% sampling |
| High-traffic | `0.01` | 1% sampling |

```bash
# Production with 10% sampling
OTEL_SAMPLING_RATIO=0.1 docker compose up -d
```

### Trace Context Propagation

W3C Trace Context headers are automatically:
- **Extracted** from incoming requests (`traceparent`, `tracestate`)
- **Injected** into responses

PHP scripts can access trace context via `$_SERVER`:

```php
<?php
// Access current trace context
$traceId = $_SERVER['TRACE_ID'] ?? null;
$spanId = $_SERVER['SPAN_ID'] ?? null;
$parentSpanId = $_SERVER['PARENT_SPAN_ID'] ?? null;

// Propagate to downstream services
$downstreamSpanId = bin2hex(random_bytes(8));
$headers = [
    "traceparent: 00-{$traceId}-{$downstreamSpanId}-01"
];

$ch = curl_init('https://api.example.com/endpoint');
curl_setopt($ch, CURLOPT_HTTPHEADER, $headers);
$response = curl_exec($ch);
```

## Log Correlation

Enable access logs with trace context for correlation:

```bash
ACCESS_LOG=1 docker compose up -d
```

Logs include `trace_id` and `span_id`:

```json
{
  "ts": "2025-01-15T10:30:00.123Z",
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
    "bytes": 1234,
    "duration_ms": 5.25
  }
}
```

Filter logs by trace ID:

```bash
docker compose logs | jq -c 'select(.ctx.trace_id == "0af7651916cd43dd8448eb211c80319c")'
```

## Best Practices

### Production Checklist

1. **Enable internal server** for metrics:
   ```bash
   INTERNAL_ADDR=0.0.0.0:9090
   ```

2. **Configure Prometheus scraping** every 5-15s

3. **Import Grafana dashboard** from `deploy/grafana/tokio-php-dashboard.json`

4. **Set up alerts** from `deploy/prometheus/alerts.yml`

5. **Enable access logs** for debugging:
   ```bash
   ACCESS_LOG=1
   ```

6. **Use sampling** for OpenTelemetry in production:
   ```bash
   OTEL_SAMPLING_RATIO=0.1
   ```

### Capacity Planning

Monitor these metrics for scaling decisions:

| Metric | Threshold | Action |
|--------|-----------|--------|
| `tokio_php_pending_requests` | > 0 consistently | Add workers or scale out |
| `tokio_php_dropped_requests` | Increasing | Increase `QUEUE_CAPACITY` |
| `tokio_php_memory_usage_percent` | > 80% | Optimize or add memory |
| `tokio_php_response_time_avg_seconds` | > 1s | Profile and optimize PHP |

## Troubleshooting

### No Metrics Appearing

1. Check internal server is running:
   ```bash
   curl http://localhost:9090/health
   ```

2. Verify `INTERNAL_ADDR` is set

3. Check Prometheus target status in UI

### Grafana Shows Empty Panels

1. Verify Prometheus datasource is configured correctly
2. Check Prometheus is scraping tokio_php (Status → Targets)
3. Verify metric names match (check `/metrics` endpoint)

### Traces Not Showing in Jaeger

1. Verify `OTEL_ENABLED=1`:
   ```bash
   docker compose exec tokio_php env | grep OTEL
   ```

2. Verify `otel` feature is compiled:
   ```bash
   CARGO_FEATURES=otel docker compose build
   ```

3. Check Jaeger is running:
   ```bash
   docker compose --profile monitoring ps jaeger
   curl http://localhost:16686/api/services
   ```

4. Check network connectivity:
   ```bash
   docker compose exec tokio_php curl -v http://jaeger:4317
   ```

5. Check Jaeger collector logs:
   ```bash
   docker compose --profile monitoring logs jaeger
   ```

6. Verify traces are being sent (check tokio_php logs):
   ```bash
   RUST_LOG=tokio_php=debug docker compose up -d
   docker compose logs tokio_php | grep -i otel
   ```

## See Also

- [Configuration](configuration.md) - Environment variables
- [Internal Server](internal-server.md) - Health checks and endpoints
- [Distributed Tracing](distributed-tracing.md) - W3C Trace Context
- [Logging](logging.md) - JSON log format
