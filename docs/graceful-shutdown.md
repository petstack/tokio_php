# Graceful Shutdown

tokio_php supports graceful shutdown with connection draining for zero-downtime deployments.

## Overview

When the server receives a shutdown signal (SIGTERM/SIGINT), it doesn't terminate immediately. Instead, it:

1. Stops accepting new connections
2. Waits for in-flight requests to complete
3. Shuts down after all connections drain (or timeout)

This ensures that active requests are not interrupted during deployments, scaling events, or restarts.

## Configuration

```bash
# Set drain timeout (default: 30 seconds)
DRAIN_TIMEOUT_SECS=30 docker compose up -d
```

| Variable | Default | Description |
|----------|---------|-------------|
| `DRAIN_TIMEOUT_SECS` | `30` | Maximum time to wait for connections to drain |

## How It Works

```
                    SIGTERM/SIGINT
                          │
                          ▼
              ┌───────────────────────┐
              │  Stop accepting new   │
              │     connections       │
              └───────────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │  Active connections?  │
              └───────────────────────┘
                    │           │
                   Yes          No
                    │           │
                    ▼           │
              ┌─────────────┐   │
              │ Wait for    │   │
              │ drain or    │   │
              │ timeout     │   │
              └─────────────┘   │
                    │           │
                    ▼           ▼
              ┌───────────────────────┐
              │   Shutdown executor   │
              │   (PHP workers)       │
              └───────────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │   Process exits       │
              └───────────────────────┘
```

## Shutdown Logs

During graceful shutdown, you'll see logs like:

```
[INFO] Received shutdown signal, draining connections...
[INFO] Waiting up to 30s for 5 active connections to complete
[DEBUG] Waiting for 5 connections to drain...
[DEBUG] Waiting for 3 connections to drain...
[DEBUG] Waiting for 1 connections to drain...
[INFO] All connections drained successfully
[INFO] Shutdown complete
```

If timeout is reached:

```
[INFO] Received shutdown signal, draining connections...
[INFO] Waiting up to 30s for 10 active connections to complete
[WARN] Drain timeout reached with 2 active connections
[INFO] Forcing shutdown after drain timeout
[INFO] Shutdown complete
```

If no active connections:

```
[INFO] Received shutdown signal, draining connections...
[INFO] No active connections, shutting down immediately
[INFO] Shutdown complete
```

## Kubernetes Integration

### Pod Configuration

```yaml
apiVersion: v1
kind: Pod
spec:
  terminationGracePeriodSeconds: 30
  containers:
    - name: app
      image: your-app:latest
      env:
        - name: DRAIN_TIMEOUT_SECS
          value: "25"  # 5s buffer for preStop
      lifecycle:
        preStop:
          exec:
            command: ["sleep", "5"]
```

### Timeline

```
0s      SIGTERM sent to container
        │
0-5s    preStop hook executes
        └── Load balancer removes pod from service endpoints
        └── New requests stop arriving
        │
5s      Application receives SIGTERM (after preStop)
        │
5-30s   Application drains existing connections
        └── DRAIN_TIMEOUT_SECS=25 (25 seconds max)
        │
30s     Kubernetes sends SIGKILL if still running
```

### Why preStop Hook?

The `preStop` hook gives time for:
1. Kubernetes to update Endpoints
2. Load balancers to stop sending traffic
3. DNS caches to expire

Without `preStop`, new requests may arrive after SIGTERM, causing failures.

### Recommended Values

| Environment | `terminationGracePeriodSeconds` | `preStop` | `DRAIN_TIMEOUT_SECS` |
|-------------|--------------------------------|-----------|---------------------|
| Development | 10 | 0 | 5 |
| Production | 30 | 5 | 25 |
| Long requests | 60 | 5 | 55 |

## Docker Compose

### Stop with Timeout

```bash
# Stop with 30 second timeout (default)
docker compose stop

# Stop with custom timeout
docker compose stop -t 60

# Immediate stop (no drain)
docker compose kill
```

### Health Check Integration

```yaml
services:
  app:
    image: tokio_php
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9090/health"]
      interval: 5s
      timeout: 3s
      retries: 3
      start_period: 10s
    stop_grace_period: 30s
```

## Testing Graceful Shutdown

### Create a Slow Endpoint

```php
<?php
// www/slow.php
$sleep = $_GET['sleep'] ?? 5;
sleep((int)$sleep);
echo json_encode([
    'status' => 'completed',
    'slept' => (int)$sleep,
    'time' => date('c')
]);
```

### Test Drain Behavior

```bash
# Terminal 1: Start slow request
curl "http://localhost:8080/slow.php?sleep=10"

# Terminal 2: Send SIGTERM while request is running
docker compose stop -t 15

# Expected: Request completes successfully before container stops
```

### Verify with Logs

```bash
# Watch logs during shutdown
docker compose logs -f | grep -E "(shutdown|drain|connections)"
```

## Comparison with Other Servers

| Server | Graceful Shutdown | Drain Timeout | Config |
|--------|------------------|---------------|--------|
| **tokio_php** | Yes | Configurable | `DRAIN_TIMEOUT_SECS` |
| Nginx | Yes | Fixed 30s | `worker_shutdown_timeout` |
| Apache | Limited | None | `GracefulShutdownTimeout` |
| PHP-FPM | Yes | Fixed | `process_control_timeout` |
| Caddy | Yes | Configurable | `grace_period` |

## Best Practices

### 1. Match Kubernetes Timeout

```bash
# If terminationGracePeriodSeconds=30 and preStop=5s
DRAIN_TIMEOUT_SECS=25
```

### 2. Monitor Active Connections

Use the `/metrics` endpoint to monitor connections:

```bash
curl http://localhost:9090/metrics | grep active_connections
# tokio_php_active_connections 5
```

### 3. Set Appropriate Request Timeouts in PHP

```php
<?php
// Ensure PHP scripts don't run longer than drain timeout
set_time_limit(25);  // Match DRAIN_TIMEOUT_SECS
```

### 4. Use Health Checks

Kubernetes uses health checks to determine pod readiness:

```yaml
readinessProbe:
  httpGet:
    path: /health
    port: 9090
  initialDelaySeconds: 5
  periodSeconds: 5
```

### 5. Log Long-Running Requests

Monitor requests that might exceed drain timeout:

```php
<?php
$start = microtime(true);
register_shutdown_function(function() use ($start) {
    $duration = microtime(true) - $start;
    if ($duration > 10) {
        error_log("Long request: {$duration}s");
    }
});
```

## Troubleshooting

### Connections Not Draining

**Symptom**: Timeout reached with active connections

**Causes**:
1. PHP scripts running longer than `DRAIN_TIMEOUT_SECS`
2. Slow database queries
3. External API calls hanging

**Solutions**:
- Increase `DRAIN_TIMEOUT_SECS`
- Add timeouts to external calls
- Use `set_time_limit()` in PHP

### Container Killed Immediately

**Symptom**: No drain logs, container stops instantly

**Causes**:
1. Using `docker kill` instead of `docker stop`
2. Kubernetes `terminationGracePeriodSeconds=0`
3. Signal not reaching application

**Solutions**:
- Use `docker stop` or `docker compose stop`
- Set appropriate `terminationGracePeriodSeconds`
- Ensure entrypoint forwards signals

### Requests Failing During Shutdown

**Symptom**: New requests fail with connection refused

**Causes**:
1. Load balancer still sending traffic
2. DNS not updated
3. No `preStop` hook

**Solutions**:
- Add `preStop` hook with sleep
- Use readiness probe
- Configure load balancer drain

## See Also

- [Configuration](configuration.md) - Environment variables reference
- [Metrics](metrics.md) - Monitoring active connections
- [Kubernetes Graceful Shutdown](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#pod-termination)
