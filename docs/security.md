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

When enabled, preload script runs as `www-data`:

```ini
; Uncomment for frameworks (Laravel, Symfony)
opcache.preload=/var/www/html/preload.php
opcache.preload_user=www-data
```

This ensures preloaded classes have correct ownership context. See [OPcache & JIT](opcache-jit.md) for configuration details.

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
TLS_CERT=/path/to/cert.pem
TLS_KEY=/path/to/key.pem
```

Features:
- TLS 1.2 and TLS 1.3 support
- HTTP/2 via ALPN negotiation
- Strong cipher suites (rustls defaults)

#### Docker Secrets

Docker Compose uses secrets for secure certificate handling:

```yaml
# docker-compose.yml
services:
  app:
    environment:
      - TLS_CERT=/run/secrets/tls_cert
      - TLS_KEY=/run/secrets/tls_key
    secrets:
      - tls_cert
      - tls_key

secrets:
  tls_cert:
    file: ./certs/cert.pem
  tls_key:
    file: ./certs/key.pem
```

Benefits:
- Files mounted in tmpfs (memory only)
- Not visible in `docker inspect`
- Mode 0444 by default (read-only)

#### Kubernetes Secrets

Create TLS secret from certificate files:

```bash
kubectl create secret tls tokio-php-tls \
  --cert=cert.pem \
  --key=key.pem
```

Or declaratively:

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: tokio-php-tls
type: kubernetes.io/tls
data:
  tls.crt: <base64-encoded-cert>
  tls.key: <base64-encoded-key>
```

Mount in deployment:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: tokio-php
spec:
  template:
    spec:
      containers:
        - name: app
          image: tokio-php:latest
          env:
            - name: TLS_CERT
              value: /tls/tls.crt
            - name: TLS_KEY
              value: /tls/tls.key
          volumeMounts:
            - name: tls
              mountPath: /tls
              readOnly: true
      volumes:
        - name: tls
          secret:
            secretName: tokio-php-tls
            defaultMode: 0400  # Owner read-only
```

#### cert-manager Integration

For automatic certificate management with Let's Encrypt:

```yaml
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: tokio-php-cert
spec:
  secretName: tokio-php-tls
  issuerRef:
    name: letsencrypt-prod
    kind: ClusterIssuer
  dnsNames:
    - example.com
    - www.example.com
```

cert-manager automatically:
- Obtains certificates from Let's Encrypt
- Stores them in the specified Secret
- Renews before expiration (30 days by default)

#### Ingress TLS Termination

Alternative: terminate TLS at Ingress level:

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: tokio-php
spec:
  tls:
    - hosts:
        - example.com
      secretName: tokio-php-tls
  rules:
    - host: example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: tokio-php
                port:
                  number: 8080
```

Considerations:
| Approach | Pros | Cons |
|----------|------|------|
| App-level TLS | End-to-end encryption, HTTP/2 to app | More resource usage |
| Ingress TLS | Centralized certs, offload crypto | Internal traffic unencrypted |

For sensitive data, use app-level TLS or enable mTLS with service mesh (Istio, Linkerd).

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
- [ ] Use secrets for certificates (not plain volumes)
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
- [ ] TLS via `kubernetes.io/tls` Secret
- [ ] Secret volume `defaultMode: 0400`
- [ ] cert-manager for auto-renewal (if using Let's Encrypt)
- [ ] Network policies configured
- [ ] Pod security policy/standards enforced

## Reporting Vulnerabilities

If you discover a security vulnerability, please report it responsibly:

1. Do not open a public issue
2. Email security details privately
3. Allow time for fix before disclosure

## See Also

- [Docker](docker.md) - Docker security, resource limits, read-only volumes
- [Configuration](configuration.md) - Environment variables reference
- [HTTP/2 & TLS](http2-tls.md) - TLS configuration details
- [Rate Limiting](rate-limiting.md) - Abuse prevention
- [Logging](logging.md) - Access logs and audit trail
- [Graceful Shutdown](graceful-shutdown.md) - Zero-downtime deployments
- [Architecture](architecture.md) - System design overview
