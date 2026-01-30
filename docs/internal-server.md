# Internal Server

tokio_php provides an optional internal HTTP server for health checks and Prometheus-compatible metrics, separate from the main application server.

## Quick Start

```bash
# Enable internal server on port 9090
INTERNAL_ADDR=0.0.0.0:9090 docker compose up -d

# Health check
curl http://localhost:9090/health

# Prometheus metrics
curl http://localhost:9090/metrics
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `INTERNAL_ADDR` | _(empty)_ | Internal server bind address (disabled if empty) |

```bash
# Production setup
INTERNAL_ADDR=0.0.0.0:9090 docker compose up -d

# Kubernetes: bind to pod IP only
INTERNAL_ADDR=127.0.0.1:9090 docker compose up -d
```

## Endpoints

| Endpoint | Description | Format |
|----------|-------------|--------|
| `/health` | Health check | JSON |
| `/metrics` | Prometheus metrics | Plain text |
| `/config` | Current server configuration | JSON |

## GET /config

Returns current server configuration as JSON. Keys are environment variable names, values are their effective settings (configured or default).

```bash
curl http://localhost:9090/config
```

**Response:**

```json
{
  "LISTEN_ADDR": "0.0.0.0:8080",
  "DOCUMENT_ROOT": "/var/www/html",
  "PHP_WORKERS": "14",
  "QUEUE_CAPACITY": "1400",
  "INDEX_FILE": "",
  "INTERNAL_ADDR": "0.0.0.0:9090",
  "ERROR_PAGES_DIR": "/var/www/html/errors",
  "DRAIN_TIMEOUT_SECS": "30",
  "STATIC_CACHE_TTL": "1d",
  "REQUEST_TIMEOUT": "2m",
  "SSE_TIMEOUT": "30m",
  "ACCESS_LOG": "0",
  "RATE_LIMIT": "0",
  "RATE_WINDOW": "60",
  "EXECUTOR": "ext",
  "TLS_CERT": "",
  "TLS_KEY": "",
  "RUST_LOG": "tokio_php=info",
  "SERVICE_NAME": "tokio_php"
}
```

| Key | Default | Description |
|-----|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | Server listen address |
| `DOCUMENT_ROOT` | `/var/www/html` | Document root directory |
| `PHP_WORKERS` | `0` (auto) | Number of PHP workers |
| `QUEUE_CAPACITY` | `0` (auto) | Request queue capacity |
| `INDEX_FILE` | _(empty)_ | Single entry point file |
| `INTERNAL_ADDR` | _(empty)_ | Internal server address |
| `ERROR_PAGES_DIR` | _(empty)_ | Custom error pages directory |
| `DRAIN_TIMEOUT_SECS` | `30` | Graceful shutdown timeout |
| `STATIC_CACHE_TTL` | `1d` | Static file cache TTL |
| `REQUEST_TIMEOUT` | `2m` | Request timeout |
| `SSE_TIMEOUT` | `30m` | SSE connection timeout |
| `ACCESS_LOG` | `0` | Access logging (`0`/`1`) |
| `RATE_LIMIT` | `0` | Rate limit per IP (`0` = disabled) |
| `RATE_WINDOW` | `60` | Rate limit window (seconds) |
| `EXECUTOR` | `ext` | Script executor (`ext`, `php`, `stub`) |
| `TLS_CERT` | _(empty)_ | TLS certificate path |
| `TLS_KEY` | _(empty)_ | TLS private key path |
| `RUST_LOG` | `tokio_php=info` | Log level filter |
| `SERVICE_NAME` | `tokio_php` | Service name for logs |

**Use Cases:**
- Verify deployment configuration matches expectations
- Debug environment variable issues
- Audit server settings in production
- CI/CD configuration validation

## GET /health

Returns server health status as JSON.

```bash
curl http://localhost:9090/health
```

**Response:**

```json
{
  "status": "ok",
  "timestamp": 1703361234,
  "active_connections": 5,
  "total_requests": 12345
}
```

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Always `"ok"` if responding |
| `timestamp` | number | Unix timestamp (seconds) |
| `active_connections` | number | Current active HTTP connections |
| `total_requests` | number | Total requests since startup |

**Use Cases:**
- Kubernetes liveness/readiness probes
- Load balancer health checks
- Monitoring systems

See [Health Checks](health-checks.md) for Kubernetes probes and Docker healthcheck configuration.

## GET /metrics

Returns Prometheus-compatible metrics.

```bash
curl http://localhost:9090/metrics
```

### Server Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_uptime_seconds` | gauge | Server uptime in seconds |
| `tokio_php_requests_per_second` | gauge | Lifetime average RPS |
| `tokio_php_response_time_avg_seconds` | gauge | Average response time |
| `tokio_php_active_connections` | gauge | Current active connections |

### Queue Metrics

See [Worker Pool](worker-pool.md) for queue configuration and [Rate Limiting](rate-limiting.md) for per-IP limits.

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_pending_requests` | gauge | Requests waiting in queue |
| `tokio_php_dropped_requests` | counter | Requests dropped (queue full, returns 503) |

### Request/Response Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `tokio_php_requests_total` | counter | `method` | Requests by HTTP method |
| `tokio_php_responses_total` | counter | `status` | Responses by status class |

**Method Labels:** `GET`, `POST`, `HEAD`, `PUT`, `DELETE`, `OPTIONS`, `PATCH`, `OTHER`

**Status Labels:** `2xx`, `3xx`, `4xx`, `5xx`

### System Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `node_load1` | gauge | 1-minute load average |
| `node_load5` | gauge | 5-minute load average |
| `node_load15` | gauge | 15-minute load average |
| `node_memory_MemTotal_bytes` | gauge | Total memory |
| `node_memory_MemAvailable_bytes` | gauge | Available memory |
| `node_memory_MemUsed_bytes` | gauge | Used memory |
| `tokio_php_memory_usage_percent` | gauge | Memory usage % |

### Example Output

```
# HELP tokio_php_uptime_seconds Server uptime in seconds
# TYPE tokio_php_uptime_seconds gauge
tokio_php_uptime_seconds 3600.123

# HELP tokio_php_requests_per_second Current requests per second (lifetime average)
# TYPE tokio_php_requests_per_second gauge
tokio_php_requests_per_second 1234.56

# HELP tokio_php_response_time_avg_seconds Average response time in seconds
# TYPE tokio_php_response_time_avg_seconds gauge
tokio_php_response_time_avg_seconds 0.002500

# HELP tokio_php_active_connections Current number of active connections
# TYPE tokio_php_active_connections gauge
tokio_php_active_connections 15

# HELP tokio_php_pending_requests Requests waiting in queue for PHP worker
# TYPE tokio_php_pending_requests gauge
tokio_php_pending_requests 3

# HELP tokio_php_dropped_requests Total requests dropped due to queue overflow
# TYPE tokio_php_dropped_requests counter
tokio_php_dropped_requests 0

# HELP tokio_php_requests_total Total number of HTTP requests by method
# TYPE tokio_php_requests_total counter
tokio_php_requests_total{method="GET"} 10000
tokio_php_requests_total{method="POST"} 500
tokio_php_requests_total{method="HEAD"} 100
tokio_php_requests_total{method="PUT"} 50
tokio_php_requests_total{method="DELETE"} 25
tokio_php_requests_total{method="OPTIONS"} 10
tokio_php_requests_total{method="PATCH"} 5
tokio_php_requests_total{method="OTHER"} 0

# HELP tokio_php_responses_total Total number of HTTP responses by status class
# TYPE tokio_php_responses_total counter
tokio_php_responses_total{status="2xx"} 9500
tokio_php_responses_total{status="3xx"} 200
tokio_php_responses_total{status="4xx"} 250
tokio_php_responses_total{status="5xx"} 50

# HELP node_load1 1-minute load average
# TYPE node_load1 gauge
node_load1 1.50

# HELP node_load5 5-minute load average
# TYPE node_load5 gauge
node_load5 1.25

# HELP node_load15 15-minute load average
# TYPE node_load15 gauge
node_load15 1.10

# HELP node_memory_MemTotal_bytes Total memory in bytes
# TYPE node_memory_MemTotal_bytes gauge
node_memory_MemTotal_bytes 17179869184

# HELP node_memory_MemAvailable_bytes Available memory in bytes
# TYPE node_memory_MemAvailable_bytes gauge
node_memory_MemAvailable_bytes 8589934592

# HELP node_memory_MemUsed_bytes Used memory in bytes
# TYPE node_memory_MemUsed_bytes gauge
node_memory_MemUsed_bytes 8589934592

# HELP tokio_php_memory_usage_percent Memory usage percentage
# TYPE tokio_php_memory_usage_percent gauge
tokio_php_memory_usage_percent 50.00
```

## Prometheus Integration

### scrape_config

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'tokio_php'
    static_configs:
      - targets: ['tokio_php:9090']
    metrics_path: /metrics
    scrape_interval: 15s
```

### Kubernetes ServiceMonitor

```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: tokio-php
spec:
  selector:
    matchLabels:
      app: tokio-php
  endpoints:
    - port: metrics
      path: /metrics
      interval: 15s
```

## Grafana Dashboard

### Key Queries

```
# Request rate
rate(tokio_php_requests_total[1m])

# Error rate percentage
sum(rate(tokio_php_responses_total{status=~"4xx|5xx"}[5m])) /
sum(rate(tokio_php_responses_total[5m])) * 100

# Response time (ms)
tokio_php_response_time_avg_seconds * 1000

# Queue depth
tokio_php_pending_requests

# Memory usage
tokio_php_memory_usage_percent
```

### Sample Dashboard JSON

```json
{
  "title": "tokio_php",
  "panels": [
    {
      "title": "Requests/sec",
      "type": "stat",
      "targets": [{"expr": "tokio_php_requests_per_second"}]
    },
    {
      "title": "Active Connections",
      "type": "gauge",
      "targets": [{"expr": "tokio_php_active_connections"}]
    },
    {
      "title": "Response Time (ms)",
      "type": "timeseries",
      "targets": [{"expr": "tokio_php_response_time_avg_seconds * 1000"}]
    },
    {
      "title": "Status Codes",
      "type": "piechart",
      "targets": [{"expr": "tokio_php_responses_total", "legendFormat": "{{status}}"}]
    }
  ]
}
```

## Alerting Examples

```yaml
groups:
  - name: tokio_php
    rules:
      # High error rate
      - alert: HighErrorRate
        expr: |
          sum(rate(tokio_php_responses_total{status="5xx"}[5m])) /
          sum(rate(tokio_php_responses_total[5m])) > 0.05
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "High 5xx error rate (> 5%)"

      # Queue overflow
      - alert: QueueOverflow
        expr: increase(tokio_php_dropped_requests[5m]) > 0
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "Requests being dropped"

      # High memory usage
      - alert: HighMemoryUsage
        expr: tokio_php_memory_usage_percent > 90
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Memory usage above 90%"

      # Slow response time
      - alert: SlowResponseTime
        expr: tokio_php_response_time_avg_seconds > 0.5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Average response time > 500ms"
```

## Security

The internal server should not be exposed publicly:

```yaml
# docker-compose.yml - bind to localhost only
ports:
  - "127.0.0.1:9090:9090"

# Kubernetes - use ClusterIP (internal only)
apiVersion: v1
kind: Service
metadata:
  name: tokio-php-metrics
spec:
  type: ClusterIP
  ports:
    - port: 9090
      name: metrics
  selector:
    app: tokio-php
```

### Network Isolation

```yaml
# docker-compose.yml
services:
  tokio_php:
    ports:
      - "8080:8080"           # Public: main server
      # - "9090:9090"         # Don't expose metrics publicly
    networks:
      - public
      - internal

  prometheus:
    networks:
      - internal             # Prometheus in same internal network
```

## Docker Compose Example

```yaml
services:
  tokio_php:
    image: tokio_php
    ports:
      - "8080:8080"
      - "9090:9090"          # Restrict in production
    environment:
      - INTERNAL_ADDR=0.0.0.0:9090
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:9090/health"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s

  prometheus:
    image: prom/prometheus
    ports:
      - "9092:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml

  grafana:
    image: grafana/grafana
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
```

## See Also

- [Health Checks](health-checks.md) - Kubernetes probes and Docker healthcheck
- [Graceful Shutdown](graceful-shutdown.md) - Connection draining and shutdown
- [Worker Pool](worker-pool.md) - Queue configuration
- [Rate Limiting](rate-limiting.md) - Per-IP request limits
- [Profiling](profiling.md) - Per-request timing breakdown
- [Configuration](configuration.md) - All environment variables
