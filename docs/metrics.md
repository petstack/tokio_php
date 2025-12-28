# Internal Server & Metrics

tokio_php includes an optional internal HTTP server for health checks and Prometheus-compatible metrics.

## Enabling Internal Server

Set `INTERNAL_ADDR` environment variable:

```bash
INTERNAL_ADDR=0.0.0.0:9090 docker compose up -d
```

## Endpoints

| Endpoint | Description | Format |
|----------|-------------|--------|
| `/health` | Health check | JSON |
| `/metrics` | Prometheus metrics | Plain text |

## Health Check

```bash
curl http://localhost:9090/health
```

Response:

```json
{
  "status": "ok",
  "timestamp": 1703361234,
  "active_connections": 5,
  "total_requests": 1000
}
```

| Field | Description |
|-------|-------------|
| `status` | Always "ok" |
| `timestamp` | Unix timestamp |
| `active_connections` | Current open connections |
| `total_requests` | Total requests processed |

## Prometheus Metrics

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

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_pending_requests` | gauge | Requests waiting in queue |
| `tokio_php_dropped_requests` | counter | Requests dropped (queue full) |

### Request Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_requests_total{method="GET"}` | counter | Requests by HTTP method |
| `tokio_php_requests_total{method="POST"}` | counter | |
| `tokio_php_requests_total{method="PUT"}` | counter | |
| `tokio_php_requests_total{method="DELETE"}` | counter | |
| `tokio_php_requests_total{method="HEAD"}` | counter | |

### Response Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_responses_total{status="2xx"}` | counter | 200-299 responses |
| `tokio_php_responses_total{status="3xx"}` | counter | 300-399 responses |
| `tokio_php_responses_total{status="4xx"}` | counter | 400-499 responses |
| `tokio_php_responses_total{status="5xx"}` | counter | 500-599 responses |

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

## Example Output

```
# HELP tokio_php_uptime_seconds Server uptime in seconds
# TYPE tokio_php_uptime_seconds gauge
tokio_php_uptime_seconds 3600.000

# HELP tokio_php_requests_per_second Lifetime average requests per second
# TYPE tokio_php_requests_per_second gauge
tokio_php_requests_per_second 1322.81

# HELP tokio_php_response_time_avg_seconds Average response time in seconds
# TYPE tokio_php_response_time_avg_seconds gauge
tokio_php_response_time_avg_seconds 0.012632

# HELP tokio_php_active_connections Current number of active connections
# TYPE tokio_php_active_connections gauge
tokio_php_active_connections 50

# HELP tokio_php_pending_requests Requests waiting in queue for PHP worker
# TYPE tokio_php_pending_requests gauge
tokio_php_pending_requests 15

# HELP tokio_php_dropped_requests Total requests dropped due to queue overflow
# TYPE tokio_php_dropped_requests counter
tokio_php_dropped_requests 0

# HELP tokio_php_requests_total Total number of HTTP requests by method
# TYPE tokio_php_requests_total counter
tokio_php_requests_total{method="GET"} 10000
tokio_php_requests_total{method="POST"} 500

# HELP tokio_php_responses_total Total number of HTTP responses by status class
# TYPE tokio_php_responses_total counter
tokio_php_responses_total{status="2xx"} 9500
tokio_php_responses_total{status="3xx"} 100
tokio_php_responses_total{status="4xx"} 350
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
node_memory_MemTotal_bytes 16777216000

# HELP node_memory_MemAvailable_bytes Available memory in bytes
# TYPE node_memory_MemAvailable_bytes gauge
node_memory_MemAvailable_bytes 8388608000

# HELP node_memory_MemUsed_bytes Used memory in bytes
# TYPE node_memory_MemUsed_bytes gauge
node_memory_MemUsed_bytes 8388608000

# HELP tokio_php_memory_usage_percent Memory usage percentage
# TYPE tokio_php_memory_usage_percent gauge
tokio_php_memory_usage_percent 50.00
```

## Prometheus Configuration

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'tokio_php'
    static_configs:
      - targets: ['tokio_php:9090']
    scrape_interval: 15s
```

## Grafana Dashboard

Example dashboard queries:

```
# Request rate
rate(tokio_php_requests_total[5m])

# Error rate
sum(rate(tokio_php_responses_total{status=~"4xx|5xx"}[5m])) /
sum(rate(tokio_php_responses_total[5m]))

# Queue pressure
tokio_php_pending_requests / tokio_php_active_connections

# Memory usage
tokio_php_memory_usage_percent
```

## Docker Compose Setup

```yaml
# docker-compose.yml
services:
  tokio_php:
    image: tokio_php
    ports:
      - "8080:8080"
      - "9090:9090"
    environment:
      - INTERNAL_ADDR=0.0.0.0:9090

  prometheus:
    image: prom/prometheus
    ports:
      - "9091:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml

  grafana:
    image: grafana/grafana
    ports:
      - "3000:3000"
```

## Kubernetes Integration

### Liveness Probe

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 9090
  initialDelaySeconds: 5
  periodSeconds: 10
```

### Readiness Probe

```yaml
readinessProbe:
  httpGet:
    path: /health
    port: 9090
  initialDelaySeconds: 5
  periodSeconds: 5
```

### ServiceMonitor (Prometheus Operator)

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

## Alerting Examples

### High Error Rate

```yaml
- alert: HighErrorRate
  expr: |
    sum(rate(tokio_php_responses_total{status=~"5xx"}[5m])) /
    sum(rate(tokio_php_responses_total[5m])) > 0.05
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "High 5xx error rate (> 5%)"
```

### Queue Overflow

```yaml
- alert: QueueOverflow
  expr: tokio_php_dropped_requests > 0
  for: 1m
  labels:
    severity: warning
  annotations:
    summary: "Requests being dropped due to queue overflow"
```

### High Memory Usage

```yaml
- alert: HighMemoryUsage
  expr: tokio_php_memory_usage_percent > 90
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Memory usage above 90%"
```

## Security

The internal server should not be exposed publicly:

```yaml
# Docker Compose - bind to localhost only
ports:
  - "127.0.0.1:9090:9090"

# Kubernetes - use ClusterIP
service:
  type: ClusterIP
  ports:
    - port: 9090
      name: metrics
```

Use network policies to restrict access to the metrics endpoint.
