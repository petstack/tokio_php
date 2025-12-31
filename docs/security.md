# Security

tokio_php follows security best practices for production deployments.

## Non-root Execution

The server runs as unprivileged `www-data` user (UID/GID 82):

```dockerfile
# Dockerfile
USER www-data
CMD ["tokio_php"]
```

### Benefits

| Aspect | Description |
|--------|-------------|
| Privilege separation | Process cannot modify system files |
| Container escape mitigation | Limited damage if container compromised |
| Kubernetes compatibility | Meets `runAsNonRoot` security context |
| Standard UID | UID 82 matches nginx/apache conventions |

### Verification

```bash
# Check user
docker compose exec tokio_php whoami
# www-data

# Check process
docker compose exec tokio_php ps aux
# PID   USER     COMMAND
#   1   www-data tokio_php

# Check file ownership
docker compose exec tokio_php ls -la /var/www/html/
# drwxr-xr-x www-data www-data /var/www/html
```

## File Permissions

All application files are owned by `www-data`:

```dockerfile
# Create directory with correct ownership
RUN mkdir -p /var/www/html && chown -R www-data:www-data /var/www/html

# Copy files with correct ownership
COPY --chown=www-data:www-data www/ /var/www/html/
```

### Recommended Permissions

| Path | Permission | Description |
|------|------------|-------------|
| `/var/www/html` | `755` | Document root (read + execute) |
| `*.php` | `644` | PHP files (read only) |
| `uploads/` | `755` | Upload directory (if needed) |
| `cache/` | `755` | Cache directory (if needed) |

## OPcache Preloading

Preload script runs as `www-data`:

```ini
opcache.preload=/var/www/html/preload.php
opcache.preload_user=www-data
```

This ensures preloaded classes have correct ownership context.

## Network Security

### Binding

```bash
# Public interface (default)
LISTEN_ADDR=0.0.0.0:8080

# Localhost only (more secure)
LISTEN_ADDR=127.0.0.1:8080
```

### Internal Server

Keep metrics/health endpoints on separate port:

```bash
# Public traffic
LISTEN_ADDR=0.0.0.0:8080

# Internal only (not exposed to public)
INTERNAL_ADDR=127.0.0.1:9090
```

### TLS

Enable HTTPS for production:

```bash
TLS_CERT=/certs/cert.pem
TLS_KEY=/certs/key.pem
```

Features:
- TLS 1.2 and TLS 1.3 support
- HTTP/2 via ALPN negotiation
- Strong cipher suites (rustls defaults)

## Rate Limiting

Protect against abuse:

```bash
# 100 requests per minute per IP
RATE_LIMIT=100
RATE_WINDOW=60
```

See [Rate Limiting](rate-limiting.md) for details.

## Kubernetes Security Context

Recommended security context for Kubernetes:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: tokio-php
spec:
  template:
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 82
        runAsGroup: 82
        fsGroup: 82
      containers:
        - name: app
          image: tokio-php:latest
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop:
                - ALL
          volumeMounts:
            - name: tmp
              mountPath: /tmp
            - name: cache
              mountPath: /var/cache
      volumes:
        - name: tmp
          emptyDir: {}
        - name: cache
          emptyDir: {}
```

### Security Context Explained

| Setting | Value | Purpose |
|---------|-------|---------|
| `runAsNonRoot` | `true` | Enforce non-root |
| `runAsUser` | `82` | www-data UID |
| `allowPrivilegeEscalation` | `false` | No sudo/setuid |
| `readOnlyRootFilesystem` | `true` | Immutable container |
| `capabilities.drop` | `ALL` | No Linux capabilities |

## Docker Security

### Read-only Volumes

Mount application code as read-only:

```yaml
volumes:
  - ./www:/var/www/html:ro
```

### Resource Limits

Prevent resource exhaustion:

```yaml
deploy:
  resources:
    limits:
      cpus: '2'
      memory: 1G
    reservations:
      cpus: '0.5'
      memory: 256M
```

### Network Isolation

Use Docker networks to isolate services:

```yaml
networks:
  frontend:
  backend:
    internal: true  # No external access

services:
  app:
    networks:
      - frontend
      - backend
  database:
    networks:
      - backend  # Only accessible from app
```

## Input Validation

tokio_php handles HTTP parsing securely:

| Protection | Implementation |
|------------|----------------|
| Header size limits | Hyper defaults (64KB) |
| Body size limits | Configurable in PHP |
| Path traversal | Resolved to document root |
| Request timeout | 5s header read timeout |

### PHP-level Validation

Always validate input in PHP:

```php
<?php

// Validate and sanitize input
$id = filter_input(INPUT_GET, 'id', FILTER_VALIDATE_INT);
if ($id === false) {
    http_response_code(400);
    exit('Invalid ID');
}

// Use prepared statements for SQL
$stmt = $pdo->prepare('SELECT * FROM users WHERE id = ?');
$stmt->execute([$id]);
```

## Logging

Access logs include client IP for audit:

```bash
ACCESS_LOG=1 docker compose up -d
```

Log format includes:
- Client IP (`ip`)
- Request ID (`request_id`)
- User-Agent (`ua`)
- X-Forwarded-For (`xff`)

See [Configuration](configuration.md) for details.

## Security Checklist

### Production Deployment

- [ ] Run as non-root (`USER www-data`)
- [ ] Enable TLS (`TLS_CERT`, `TLS_KEY`)
- [ ] Enable rate limiting (`RATE_LIMIT`)
- [ ] Separate internal endpoints (`INTERNAL_ADDR`)
- [ ] Read-only volumes (`:ro`)
- [ ] Resource limits configured
- [ ] Access logging enabled (`ACCESS_LOG=1`)
- [ ] Disable debug logging (`RUST_LOG=tokio_php=warn`)

### Kubernetes

- [ ] `runAsNonRoot: true`
- [ ] `allowPrivilegeEscalation: false`
- [ ] `readOnlyRootFilesystem: true`
- [ ] `capabilities.drop: ALL`
- [ ] Network policies configured
- [ ] Pod security policy/standards enforced

## Reporting Vulnerabilities

If you discover a security vulnerability, please report it responsibly:

1. Do not open a public issue
2. Email security details privately
3. Allow time for fix before disclosure

## See Also

- [Configuration](configuration.md) - Environment variables reference
- [HTTP/2 & TLS](http2-tls.md) - TLS configuration details
- [Rate Limiting](rate-limiting.md) - Abuse prevention
- [Graceful Shutdown](graceful-shutdown.md) - Zero-downtime deployments
- [Architecture](architecture.md) - System design overview
