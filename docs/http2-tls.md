# HTTP/2 & TLS Support

tokio_php supports HTTP/1.1, HTTP/2, and HTTPS with TLS 1.3.

## Protocol Support

| Protocol | Port | Description |
|----------|------|-------------|
| HTTP/1.1 | 8080 | Default protocol |
| HTTP/2 h2c | 8080 | Cleartext HTTP/2 (prior knowledge) |
| HTTPS + HTTP/2 | 8443 | TLS 1.3 with ALPN negotiation |

## Usage

### HTTP/1.1 (Default)

```bash
curl http://localhost:8080/index.php
```

### HTTP/2 Cleartext (h2c)

HTTP/2 over plain TCP requires the client to know the server supports it (prior knowledge):

```bash
curl --http2-prior-knowledge http://localhost:8080/index.php
```

### HTTPS with HTTP/2

Start the TLS-enabled services:

```bash
# Start with TLS profile
docker compose --profile tls up -d
```

Test HTTPS connection:

```bash
# HTTPS (auto-negotiates HTTP/2 via ALPN)
curl -k https://localhost:8443/index.php

# Force HTTP/1.1 over HTTPS
curl -k --http1.1 https://localhost:8443/index.php

# Check protocol version in PHP
curl -k https://localhost:8443/protocol.php
# Output: SERVER_PROTOCOL = HTTP/2.0
```

## TLS Configuration

### Environment Variables

| Variable | Description |
|----------|-------------|
| `TLS_CERT` | Path to PEM certificate file |
| `TLS_KEY` | Path to PEM private key file |

### Using Custom Certificates

```bash
# Generate self-signed certificate (development)
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem \
  -days 365 -nodes -subj "/CN=localhost"

# Start with custom certificates
TLS_CERT=/path/to/cert.pem TLS_KEY=/path/to/key.pem docker compose up -d
```

### Default Development Certificates

The `certs/` directory contains self-signed certificates for development:

```
certs/
├── cert.pem    # Self-signed certificate
└── key.pem     # Private key
```

These are automatically used by the TLS services in docker-compose.yml.

## Docker Services

| Service | Port | Protocol |
|---------|------|----------|
| `tokio_php` | 8080 | HTTP/1.1, HTTP/2 h2c |
| `tokio_php_tls` | 8443 | HTTPS + HTTP/2 |

## Implementation Details

### Automatic Protocol Detection

Uses `hyper_util::server::conn::auto::Builder` for automatic protocol detection:

```rust
let builder = auto::Builder::new(TokioExecutor::new())
    .http1()
    .http2();
```

### ALPN Negotiation

For HTTPS connections, ALPN protocols are configured in order of preference:

1. `h2` - HTTP/2
2. `http/1.1` - HTTP/1.1 fallback

```rust
let mut config = ServerConfig::builder()
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
```

### TLS Version

Only TLS 1.3 is enabled for maximum security:

```rust
config.versions(&[&rustls::version::TLS13]);
```

## PHP Integration

The HTTP protocol version is available in PHP via `$_SERVER['SERVER_PROTOCOL']`:

```php
<?php

echo $_SERVER['SERVER_PROTOCOL']; // HTTP/1.1, HTTP/2.0
echo $_SERVER['HTTPS'];           // "on" for HTTPS, not set for HTTP
echo $_SERVER['SSL_PROTOCOL'];    // TLSv1_3 for HTTPS
```

## Performance Notes

HTTP/2 provides:
- **Multiplexing**: Multiple requests over single connection
- **Header compression**: HPACK reduces header overhead
- **Server push**: (not implemented yet)

Benchmark comparison:

| Protocol | Latency | Notes |
|----------|---------|-------|
| HTTP/1.1 | ~1.7ms | Simple, well-supported |
| HTTP/2 h2c | ~0.6ms | Lower latency, multiplexing |
| HTTPS + HTTP/2 | ~0.2ms + 11ms TLS | Initial TLS handshake overhead |

Note: TLS handshake (~11ms) is a one-time cost per connection. With keep-alive connections, subsequent requests have similar latency to h2c.

## Limitations

- HTTP/3 (QUIC) is not yet implemented ([h3 crate is experimental](https://github.com/hyperium/h3))
- HTTP 103 Early Hints is not yet implemented ([next version of hyper](https://github.com/hyperium/h2/pull/865))
