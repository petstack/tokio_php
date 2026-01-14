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
| `TLS_CERT` | Path to PEM certificate file (inside container) |
| `TLS_KEY` | Path to PEM private key file (inside container) |
| `TLS_CERT_FILE` | Docker secrets: host path to certificate (default: `./certs/cert.pem`) |
| `TLS_KEY_FILE` | Docker secrets: host path to private key (default: `./certs/key.pem`) |

### Using Docker Secrets (Recommended)

Docker secrets provide secure certificate handling:

```bash
# Use default paths (./certs/cert.pem, ./certs/key.pem)
docker compose --profile tls up -d

# Custom certificate paths
TLS_CERT_FILE=/path/to/cert.pem TLS_KEY_FILE=/path/to/key.pem docker compose --profile tls up -d
```

### Using Custom Certificates (Non-Docker)

```bash
# Generate self-signed certificate (development)
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem \
  -days 365 -nodes -subj "/CN=localhost"

# Start with custom certificates (direct path, non-Docker)
TLS_CERT=/path/to/cert.pem TLS_KEY=/path/to/key.pem ./tokio_php
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

| Service | Main Port | Protocol (Main) | Internal Port | Profile |
|---------|-----------|-----------------|---------------|---------|
| `tokio_php` | 8080 | HTTP/1.1, HTTP/2 h2c | 9090 (HTTP) | default |
| `tokio_php_tls` | 8443 | HTTPS + HTTP/2 | 9090 (HTTP) | tls |

- **Main port**: Application traffic with protocol support as listed
- **Internal port**: Health checks and metrics — always plain HTTP (no TLS)

**Note:** Run only one service at a time (`tokio_php` OR `tokio_php_tls`) to avoid port 9090 conflict.

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

- **HTTP/3 (QUIC)**: Not yet implemented ([h3 crate is experimental](https://github.com/hyperium/h3))
- **HTTP 103 Early Hints**: Infrastructure ready via bridge (`tokio_early_hints()` PHP function exists), but full streaming support pending server handler changes. See [tokio_sapi Extension](tokio-sapi-extension.md#tokio_early_hints) for current status.

## See Also

- [Configuration](configuration.md) - TLS_CERT and TLS_KEY environment variables
- [Profiling](profiling.md) - TLS handshake timing metrics
- [Architecture](architecture.md) - Protocol detection implementation
- [tokio_sapi Extension](tokio-sapi-extension.md) - Early Hints PHP function
