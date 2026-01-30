# Health Checks

tokio_php provides health check endpoints for container orchestration and monitoring.

## Internal Server

Enable the internal HTTP server to expose health and metrics endpoints:

```bash
INTERNAL_ADDR=0.0.0.0:9090 docker compose up -d
```

### Endpoints

| Endpoint | Description |
|----------|-------------|
| `/health` | Health check (JSON) |
| `/metrics` | Prometheus metrics |
| `/config` | Current server configuration (JSON) |

### Health Response

```bash
curl http://localhost:9090/health
```

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
| `status` | Always `"ok"` when server is healthy |
| `timestamp` | Unix timestamp |
| `active_connections` | Current active HTTP connections |
| `total_requests` | Total requests processed since start |

## Monitoring Stack

For full observability, enable the monitoring profile:

```bash
# Start with Prometheus + Grafana
docker compose --profile monitoring up -d

# Access:
# - Prometheus: http://localhost:9091
# - Grafana: http://localhost:3000 (admin/admin)
```

See [Observability](observability.md) for metrics, dashboards, and alerting.

## Docker Compose Healthcheck

docker-compose.yml includes built-in health checks:

```yaml
services:
  tokio_php:
    # ...
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:9090/health"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s
```

### Parameters

| Parameter | Value | Description |
|-----------|-------|-------------|
| `test` | `curl -sf` | Silent, fail on HTTP errors |
| `interval` | `10s` | Check every 10 seconds |
| `timeout` | `5s` | Fail if no response in 5s |
| `retries` | `3` | Mark unhealthy after 3 failures |
| `start_period` | `10s` | Grace period for startup |

### Check Status

```bash
# View health status
docker compose ps
NAME                    STATUS
tokio_php-tokio_php-1   Up 2 minutes (healthy)

# Detailed inspection
docker inspect tokio_php-tokio_php-1 --format='{{json .State.Health}}' | jq
{
  "Status": "healthy",
  "FailingStreak": 0,
  "Log": [...]
}
```

## Kubernetes Probes

Kubernetes provides three types of probes for container health management.

### Probe Types

| Probe | Purpose | On Failure |
|-------|---------|------------|
| **Liveness** | Is container alive? | Restart container |
| **Readiness** | Ready for traffic? | Remove from Service |
| **Startup** | App initialized? | Block liveness/readiness |

### Recommended Configuration

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: tokio-php
spec:
  replicas: 3
  selector:
    matchLabels:
      app: tokio-php
  template:
    metadata:
      labels:
        app: tokio-php
    spec:
      terminationGracePeriodSeconds: 30
      containers:
        - name: app
          image: tokio-php:latest
          ports:
            - name: http
              containerPort: 8080
            - name: internal
              containerPort: 9090
          env:
            - name: LISTEN_ADDR
              value: "0.0.0.0:8080"
            - name: INTERNAL_ADDR
              value: "0.0.0.0:9090"
            - name: PHP_WORKERS
              value: "8"
            - name: DRAIN_TIMEOUT_SECS
              value: "25"

          # Startup probe: wait for PHP workers initialization
          startupProbe:
            httpGet:
              path: /health
              port: internal
            failureThreshold: 30
            periodSeconds: 1

          # Liveness probe: restart if unresponsive
          livenessProbe:
            httpGet:
              path: /health
              port: internal
            initialDelaySeconds: 0
            periodSeconds: 10
            timeoutSeconds: 5
            failureThreshold: 3

          # Readiness probe: receive traffic when ready
          readinessProbe:
            httpGet:
              path: /health
              port: internal
            initialDelaySeconds: 0
            periodSeconds: 5
            timeoutSeconds: 3
            failureThreshold: 1

          # Graceful shutdown
          lifecycle:
            preStop:
              exec:
                command: ["sleep", "5"]

          resources:
            requests:
              memory: "256Mi"
              cpu: "250m"
            limits:
              memory: "1Gi"
              cpu: "2000m"
```

### Probe Parameters Explained

#### Startup Probe

```yaml
startupProbe:
  httpGet:
    path: /health
    port: internal
  failureThreshold: 30   # Max 30 attempts
  periodSeconds: 1       # Every 1 second
```

- Allows up to 30 seconds for PHP workers to initialize
- Blocks liveness/readiness until success
- Critical for applications with slow startup

#### Liveness Probe

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: internal
  initialDelaySeconds: 0  # Start immediately (after startup probe)
  periodSeconds: 10       # Check every 10 seconds
  timeoutSeconds: 5       # Timeout per check
  failureThreshold: 3     # 3 failures = restart
```

- Detects deadlocks and hung processes
- Container restarted after 3 consecutive failures
- Total time to restart: 3 × 10s = 30 seconds

#### Readiness Probe

```yaml
readinessProbe:
  httpGet:
    path: /health
    port: internal
  initialDelaySeconds: 0  # Start immediately
  periodSeconds: 5        # Check every 5 seconds
  timeoutSeconds: 3       # Timeout per check
  failureThreshold: 1     # 1 failure = remove from LB
```

- Controls traffic routing via Service
- Fast removal (5s) when overloaded
- Pod stays running, just no new requests

### Graceful Shutdown Timeline

```
0s    SIGTERM sent to container
      │
      ├── preStop hook starts: sleep 5
      │   (allows load balancer to remove pod)
      │
5s    preStop completes, SIGTERM delivered to app
      │
      ├── Server stops accepting new connections
      ├── In-flight requests continue (DRAIN_TIMEOUT_SECS=25)
      │
30s   terminationGracePeriodSeconds reached
      └── SIGKILL if still running
```

**Key settings:**
- `terminationGracePeriodSeconds: 30` - total shutdown window
- `DRAIN_TIMEOUT_SECS=25` - app drain timeout (< termination - preStop)
- `preStop: sleep 5` - LB deregistration time

### Service Configuration

```yaml
apiVersion: v1
kind: Service
metadata:
  name: tokio-php
spec:
  selector:
    app: tokio-php
  ports:
    - name: http
      port: 80
      targetPort: http
    - name: metrics
      port: 9090
      targetPort: internal
```

### HorizontalPodAutoscaler

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: tokio-php
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: tokio-php
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
```

## Advanced Patterns

### Custom Health Endpoint

For application-level health checks (database, cache, etc.), create a PHP endpoint:

```php
<?php
// www/health.php

header('Content-Type: application/json');

$checks = [
    'php' => true,
    'opcache' => function_exists('opcache_get_status') && opcache_get_status() !== false,
];

// Add database check
// $checks['database'] = checkDatabaseConnection();

// Add Redis check
// $checks['redis'] = checkRedisConnection();

$healthy = !in_array(false, $checks, true);

http_response_code($healthy ? 200 : 503);

echo json_encode([
    'status' => $healthy ? 'ok' : 'degraded',
    'checks' => $checks,
    'timestamp' => time(),
]);
```

Use as readiness probe:

```yaml
readinessProbe:
  httpGet:
    path: /health.php
    port: http  # Main app port, not internal
  periodSeconds: 5
```

### Separate Liveness and Readiness

| Probe | Endpoint | Purpose |
|-------|----------|---------|
| Liveness | `/health` (internal) | Process alive |
| Readiness | `/health.php` (app) | Dependencies ready |

This allows:
- Quick liveness checks (no PHP execution)
- Deep readiness checks (database, cache)

### PodDisruptionBudget

Ensure availability during updates:

```yaml
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: tokio-php
spec:
  minAvailable: 2  # or: maxUnavailable: 1
  selector:
    matchLabels:
      app: tokio-php
```

## Troubleshooting

### Container Not Becoming Healthy

```bash
# Check health logs
docker inspect <container> --format='{{json .State.Health.Log}}' | jq

# Test endpoint manually
docker exec <container> curl -sf http://localhost:9090/health

# Check if internal server is running
docker exec <container> netstat -tlnp | grep 9090
```

### Kubernetes Pod Not Ready

```bash
# Describe pod for events
kubectl describe pod <pod-name>

# Check probe status
kubectl get pod <pod-name> -o jsonpath='{.status.conditions}'

# Test from within pod
kubectl exec <pod-name> -- curl -sf http://localhost:9090/health

# Check logs
kubectl logs <pod-name> --tail=50
```

### Common Issues

| Issue | Cause | Solution |
|-------|-------|----------|
| Slow startup | Many PHP workers | Increase `startupProbe.failureThreshold` |
| Frequent restarts | Timeout too short | Increase `livenessProbe.timeoutSeconds` |
| Traffic during shutdown | No preStop hook | Add `preStop: sleep 5` |
| Cascading failures | All pods restart | Add `PodDisruptionBudget` |

## Best Practices

1. **Always use startup probe** for applications with initialization time
2. **Keep liveness simple** - check process health, not dependencies
3. **Use readiness for dependencies** - database, cache, external APIs
4. **Match drain timeout** - `DRAIN_TIMEOUT_SECS` < `terminationGracePeriodSeconds` - preStop
5. **Add preStop hook** - allow load balancer to deregister pod
6. **Set resource limits** - prevent probe timeouts from resource starvation
7. **Use named ports** - clearer configuration, easier updates

## See Also

- [Internal Server](internal-server.md) - Full /health and /metrics endpoint reference
- [Observability](observability.md) - Monitoring stack, Grafana, Prometheus alerts
- [Graceful Shutdown](graceful-shutdown.md) - Shutdown behavior and Kubernetes integration
- [Configuration](configuration.md) - Environment variables reference
