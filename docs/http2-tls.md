# HTTP/2 & TLS Support

tokio_php supports HTTP/1.1, HTTP/2, and HTTPS with TLS 1.2/1.3.

## Protocol Support

| Protocol | Port | Description |
|----------|------|-------------|
| HTTP/1.1 | 8080 | Default protocol |
| HTTP/2 h2c | 8080 | Cleartext HTTP/2 (prior knowledge) |
| HTTPS + HTTP/2 | 8443 | TLS with ALPN negotiation (profile: tls) |

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

### Development Certificates Setup

Create the `certs/` directory with self-signed certificates:

```bash
# Create certs directory
mkdir -p certs

# Generate self-signed certificate for development
openssl req -x509 -newkey rsa:4096 -keyout certs/key.pem -out certs/cert.pem \
  -days 365 -nodes -subj "/CN=localhost"
```

Expected structure:
```
certs/
├── cert.pem    # Self-signed certificate
└── key.pem     # Private key
```

## Docker Services

| Service | Port | Protocol | Profile |
|---------|------|----------|---------|
| `tokio_php` | 8080 | HTTP/1.1, HTTP/2 h2c | default |
| `tokio_php_tls` | 8443 | HTTPS + HTTP/2 | tls |

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

rustls defaults are used, supporting TLS 1.2 and TLS 1.3. The server negotiates the highest version supported by the client.

## PHP Integration

The HTTP protocol version is available in PHP via `$_SERVER['SERVER_PROTOCOL']`:

```php
<?php

echo $_SERVER['SERVER_PROTOCOL']; // HTTP/1.1, HTTP/2.0
echo $_SERVER['HTTPS'];           // "on" for HTTPS, not set for HTTP
echo $_SERVER['SSL_PROTOCOL'];    // TLSv1.2 or TLSv1.3 for HTTPS
```

## Performance Notes

HTTP/2 provides:
- **Multiplexing**: Multiple requests over single connection
- **Header compression**: HPACK reduces header overhead
- **Server push**: (not implemented yet)

### Protocol Comparison

| Protocol | Characteristics |
|----------|-----------------|
| HTTP/1.1 | Simple, widely supported, one request per connection (without pipelining) |
| HTTP/2 h2c | Lower latency with multiplexing, requires `--http2-prior-knowledge` |
| HTTPS + HTTP/2 | TLS handshake adds initial latency, but keep-alive amortizes cost |

TLS handshake is a one-time cost per connection. With keep-alive connections, subsequent requests have similar latency to h2c.

## Limitations

- HTTP/3 (QUIC) is not yet implemented ([h3 crate is experimental](https://github.com/hyperium/h3))
- HTTP 103 Early Hints is not yet implemented ([next version of hyper](https://github.com/hyperium/h2/pull/865))

## See Also

- [Configuration](configuration.md) - TLS_CERT and TLS_KEY environment variables
- [Profiling](profiling.md) - TLS handshake timing metrics
- [Architecture](architecture.md) - Protocol detection implementation
