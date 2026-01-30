# gRPC Support

> **New to gRPC?** Start with [gRPC for Beginners](grpc-introduction.md) — a simple explanation with examples.

tokio_php includes an optional gRPC server for microservices architecture. Execute PHP scripts via gRPC for efficient service-to-service communication.

## Overview

```
┌─────────────────────┐         ┌─────────────────────┐
│   gRPC Client       │  proto  │     tokio_php       │
│   (Go/Python/PHP)   │────────►│   gRPC Server       │
└─────────────────────┘         └─────────┬───────────┘
                                          │
                                          ▼
                                ┌─────────────────────┐
                                │   PHP Worker Pool   │
                                │   (php-embed SAPI)  │
                                └─────────────────────┘
```

**Benefits:**
- Binary protocol (smaller payloads, faster parsing)
- HTTP/2 multiplexing
- Strongly typed contracts (Protobuf)
- Streaming support
- Native health checking

## Building with gRPC

gRPC support is a compile-time feature:

```bash
# Local build
cargo build --release --features grpc

# Docker build
CARGO_FEATURES=grpc docker compose build

# Verify gRPC is enabled
docker compose logs | grep -i grpc
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `GRPC_ADDR` | _(empty)_ | gRPC server address (e.g., `0.0.0.0:50051`) |
| `GRPC_TLS` | `off` | TLS mode: `off`, `auto`, `on` |
| `GRPC_TLS_CERT` | _(empty)_ | Certificate path (required when `GRPC_TLS=on`) |
| `GRPC_TLS_KEY` | _(empty)_ | Private key path (required when `GRPC_TLS=on`) |
| `GRPC_TLS_CA` | _(empty)_ | CA certificate for mTLS (optional) |
| `GRPC_TLS_AUTO_CN` | `localhost` | Common Name for auto-generated certs |
| `GRPC_TLS_AUTO_DAYS` | `365` | Validity days for auto-generated certs |

```bash
# Enable gRPC server on port 50051 (plaintext)
GRPC_ADDR=0.0.0.0:50051 docker compose up -d
```

## TLS Configuration

gRPC TLS supports three modes:

### Plaintext (default)

```bash
GRPC_TLS=off  # or omit
GRPC_ADDR=0.0.0.0:50051 docker compose up -d
```

No encryption. Use only for development or within trusted networks.

### Auto-generated Certificates (development)

```bash
GRPC_TLS=auto \
GRPC_ADDR=0.0.0.0:50051 \
docker compose up -d
```

Generates self-signed certificates on first start:
- Stored in `/tmp/tokio_php/grpc-{cert,key}.pem`
- Valid for 365 days (configurable)
- Regenerates automatically when expiring (<30 days)

Test with grpcurl:
```bash
grpcurl -insecure localhost:50051 list
```

### External Certificates (production)

```bash
GRPC_TLS=on \
GRPC_TLS_CERT=/run/secrets/grpc-cert.pem \
GRPC_TLS_KEY=/run/secrets/grpc-key.pem \
GRPC_ADDR=0.0.0.0:50051 \
docker compose up -d
```

Use with cert-manager, Vault, or other PKI solutions.

### Mutual TLS (mTLS)

Require client certificates for authentication:

```bash
GRPC_TLS=on \
GRPC_TLS_CERT=/run/secrets/grpc-cert.pem \
GRPC_TLS_KEY=/run/secrets/grpc-key.pem \
GRPC_TLS_CA=/run/secrets/ca.pem \
GRPC_ADDR=0.0.0.0:50051 \
docker compose up -d
```

Test with grpcurl (client cert required):
```bash
grpcurl \
  -cacert ca.pem \
  -cert client-cert.pem \
  -key client-key.pem \
  localhost:50051 list
```

### TLS Mode Comparison

| Mode | Security | Use Case |
|------|----------|----------|
| `off` | None | Development, service mesh (Istio/Linkerd handles TLS) |
| `auto` | Self-signed | Development, testing, internal services |
| `on` | Full PKI | Production, external certificates |
| `on` + CA | mTLS | Zero-trust, service-to-service auth |

## Service Definition

The gRPC service is defined in `proto/php_service.proto`:

```protobuf
service PhpService {
  // Execute PHP script (unary)
  rpc Execute(ExecuteRequest) returns (ExecuteResponse);

  // Execute with streaming response (SSE/long-polling)
  rpc ExecuteStream(ExecuteRequest) returns (stream StreamChunk);

  // Health check (gRPC standard)
  rpc Check(HealthCheckRequest) returns (HealthCheckResponse);
}
```

### ExecuteRequest

| Field | Type | Description |
|-------|------|-------------|
| `script_path` | string | Script path relative to document root |
| `method` | string | HTTP method (GET, POST, etc.) |
| `query_params` | map | Query parameters (`$_GET`) |
| `form_data` | map | Form data (`$_POST`) |
| `body` | bytes | Raw request body |
| `content_type` | string | Content-Type of body |
| `server_vars` | map | Server variables (`$_SERVER`) |
| `cookies` | map | Cookies (`$_COOKIE`) |
| `options` | RequestOptions | Execution options |

### RequestOptions

| Field | Type | Description |
|-------|------|-------------|
| `timeout_ms` | int64 | Request timeout (0 = default) |
| `enable_profiling` | bool | Enable profiling |
| `trace_parent` | string | W3C traceparent header |
| `trace_state` | string | W3C tracestate header |

### ExecuteResponse

| Field | Type | Description |
|-------|------|-------------|
| `status_code` | int32 | HTTP status code |
| `headers` | map | Response headers |
| `body` | bytes | Response body |
| `metadata` | ExecutionMetadata | Execution stats |

### ExecutionMetadata

| Field | Type | Description |
|-------|------|-------------|
| `request_id` | string | Request ID for tracing |
| `worker_id` | int32 | Worker thread ID |
| `execution_time_us` | int64 | Execution time (microseconds) |
| `queue_wait_us` | int64 | Queue wait time (microseconds) |
| `profile` | ProfileData | Profiling data (if enabled) |

## Testing with grpcurl

[grpcurl](https://github.com/fullstorydev/grpcurl) is a command-line tool for interacting with gRPC servers.

### Installation

```bash
# macOS
brew install grpcurl

# Linux
go install github.com/fullstorydev/grpcurl/cmd/grpcurl@latest
```

### Basic Commands

```bash
# List available services
grpcurl -plaintext localhost:50051 list

# Describe service
grpcurl -plaintext localhost:50051 describe tokio_php.v1.PhpService

# Health check
grpcurl -plaintext -d '{"service": ""}' \
  localhost:50051 tokio_php.v1.PhpService/Check
```

### Execute PHP Script

```bash
# GET request
grpcurl -plaintext -d '{
  "script_path": "index.php",
  "method": "GET",
  "query_params": {"page": "1", "limit": "10"}
}' localhost:50051 tokio_php.v1.PhpService/Execute

# POST request with form data
grpcurl -plaintext -d '{
  "script_path": "api/users.php",
  "method": "POST",
  "form_data": {"name": "John", "email": "john@example.com"},
  "server_vars": {"HTTP_AUTHORIZATION": "Bearer token123"}
}' localhost:50051 tokio_php.v1.PhpService/Execute

# POST with JSON body
grpcurl -plaintext -d '{
  "script_path": "api/data.php",
  "method": "POST",
  "body": "eyJrZXkiOiAidmFsdWUifQ==",
  "content_type": "application/json"
}' localhost:50051 tokio_php.v1.PhpService/Execute
```

Note: `body` field uses base64 encoding. `eyJrZXkiOiAidmFsdWUifQ==` = `{"key": "value"}`

### Streaming Response

```bash
# Stream response (SSE simulation)
grpcurl -plaintext -d '{
  "script_path": "stream.php",
  "method": "GET"
}' localhost:50051 tokio_php.v1.PhpService/ExecuteStream
```

## Client Examples

### Go Client

```go
package main

import (
    "context"
    "log"
    "time"

    "google.golang.org/grpc"
    "google.golang.org/grpc/credentials/insecure"

    pb "your-project/proto" // Generated from php_service.proto
)

func main() {
    conn, err := grpc.NewClient("localhost:50051",
        grpc.WithTransportCredentials(insecure.NewCredentials()),
    )
    if err != nil {
        log.Fatal(err)
    }
    defer conn.Close()

    client := pb.NewPhpServiceClient(conn)
    ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
    defer cancel()

    // Execute PHP script
    resp, err := client.Execute(ctx, &pb.ExecuteRequest{
        ScriptPath:  "index.php",
        Method:      "GET",
        QueryParams: map[string]string{"page": "1"},
        Options: &pb.RequestOptions{
            TimeoutMs: 5000,
        },
    })
    if err != nil {
        log.Fatal(err)
    }

    log.Printf("Status: %d", resp.StatusCode)
    log.Printf("Body: %s", string(resp.Body))
}
```

Generate Go code:
```bash
protoc --go_out=. --go-grpc_out=. proto/php_service.proto
```

### Python Client

```python
import grpc
from proto import php_service_pb2 as pb
from proto import php_service_pb2_grpc as rpc

def main():
    channel = grpc.insecure_channel('localhost:50051')
    client = rpc.PhpServiceStub(channel)

    # Execute PHP script
    response = client.Execute(pb.ExecuteRequest(
        script_path='index.php',
        method='GET',
        query_params={'page': '1', 'limit': '10'},
        options=pb.RequestOptions(timeout_ms=5000),
    ))

    print(f'Status: {response.status_code}')
    print(f'Body: {response.body.decode()}')

    # Health check
    health = client.Check(pb.HealthCheckRequest(service=''))
    print(f'Health: {health.status}')

if __name__ == '__main__':
    main()
```

Generate Python code:
```bash
pip install grpcio grpcio-tools
python -m grpc_tools.protoc -I. --python_out=. --grpc_python_out=. proto/php_service.proto
```

### PHP Client

Using the gRPC PHP extension:

```php
<?php
require_once 'vendor/autoload.php';

use Grpc\ChannelCredentials;

$channel = new \Grpc\Channel('localhost:50051', [
    'credentials' => ChannelCredentials::createInsecure(),
]);

$client = new \Tokio_php\V1\PhpServiceClient('localhost:50051', [
    'credentials' => ChannelCredentials::createInsecure(),
]);

// Execute PHP script
$request = new \Tokio_php\V1\ExecuteRequest();
$request->setScriptPath('index.php');
$request->setMethod('GET');
$request->setQueryParams(['page' => '1']);

list($response, $status) = $client->Execute($request)->wait();

if ($status->code === \Grpc\STATUS_OK) {
    echo "Status: " . $response->getStatusCode() . "\n";
    echo "Body: " . $response->getBody() . "\n";
}
```

Install PHP gRPC extension:
```bash
pecl install grpc protobuf
composer require grpc/grpc google/protobuf
```

Generate PHP code:
```bash
protoc --php_out=. --grpc_out=. \
  --plugin=protoc-gen-grpc=$(which grpc_php_plugin) \
  proto/php_service.proto
```

## PHP Script Compatibility

PHP scripts work identically whether called via HTTP or gRPC:

```php
<?php
// api_example.php - works via HTTP and gRPC

header('Content-Type: application/json');

$method = $_SERVER['REQUEST_METHOD'] ?? 'GET';
$requestId = $_SERVER['HTTP_X_REQUEST_ID'] ?? uniqid();

switch ($method) {
    case 'GET':
        $page = (int)($_GET['page'] ?? 1);
        echo json_encode([
            'success' => true,
            'data' => ['page' => $page],
            'request_id' => $requestId,
        ]);
        break;

    case 'POST':
        $name = $_POST['name'] ?? '';
        echo json_encode([
            'success' => true,
            'data' => ['created' => $name],
            'request_id' => $requestId,
        ]);
        break;
}
```

Test both ways:
```bash
# HTTP
curl -X POST http://localhost:8080/api_example.php \
  -d "name=John&email=john@example.com"

# gRPC
grpcurl -plaintext -d '{
  "script_path": "api_example.php",
  "method": "POST",
  "form_data": {"name": "John", "email": "john@example.com"}
}' localhost:50051 tokio_php.v1.PhpService/Execute
```

## Health Checking

gRPC health checking follows the [gRPC Health Checking Protocol](https://github.com/grpc/grpc/blob/master/doc/health-checking.md):

```bash
# Check overall health
grpcurl -plaintext -d '{"service": ""}' \
  localhost:50051 tokio_php.v1.PhpService/Check

# Response
{
  "status": "SERVING"
}
```

### ServingStatus Values

| Status | Code | Description |
|--------|------|-------------|
| `UNKNOWN` | 0 | Status unknown |
| `SERVING` | 1 | Server is healthy |
| `NOT_SERVING` | 2 | Server is unhealthy |
| `SERVICE_UNKNOWN` | 3 | Service not recognized |

## Distributed Tracing

Pass W3C Trace Context via RequestOptions:

```bash
grpcurl -plaintext -d '{
  "script_path": "index.php",
  "method": "GET",
  "options": {
    "trace_parent": "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
    "trace_state": "vendor=value"
  }
}' localhost:50051 tokio_php.v1.PhpService/Execute
```

The trace context is available in PHP via:
```php
$_SERVER['TRACE_ID'];        // 0af7651916cd43dd8448eb211c80319c
$_SERVER['SPAN_ID'];         // b7ad6b7169203331
$_SERVER['HTTP_TRACEPARENT']; // Full header value
```

## Performance Considerations

| Aspect | HTTP | gRPC |
|--------|------|------|
| Protocol | HTTP/1.1 or HTTP/2 | HTTP/2 only |
| Encoding | JSON/Form | Protobuf (binary) |
| Payload size | Larger | Smaller (~30%) |
| Parsing | Slower | Faster |
| Streaming | SSE | Native |
| Browser support | Yes | Limited |

**When to use gRPC:**
- Service-to-service communication
- High-throughput internal APIs
- Streaming requirements
- Strict type contracts needed

**When to use HTTP:**
- Browser clients
- Public APIs
- Simple integrations
- Human-readable debugging

## Kubernetes Deployment

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
          image: tokio-php:grpc
          ports:
            - name: http
              containerPort: 8080
            - name: grpc
              containerPort: 50051
            - name: internal
              containerPort: 9090
          env:
            - name: LISTEN_ADDR
              value: "0.0.0.0:8080"
            - name: GRPC_ADDR
              value: "0.0.0.0:50051"
            - name: INTERNAL_ADDR
              value: "0.0.0.0:9090"
---
apiVersion: v1
kind: Service
metadata:
  name: tokio-php
spec:
  ports:
    - name: http
      port: 80
      targetPort: http
    - name: grpc
      port: 50051
      targetPort: grpc
```

## Troubleshooting

### Connection Refused

```bash
# Verify gRPC server is running
docker compose logs | grep "gRPC server"

# Check port is open
netstat -tlnp | grep 50051
```

Ensure:
1. Built with `--features grpc`
2. `GRPC_ADDR` is set
3. Port 50051 is exposed in docker-compose.yml

### grpcurl: Server does not support reflection

tokio_php uses server reflection. If you see this error:
```bash
# Use proto file directly
grpcurl -plaintext -proto proto/php_service.proto \
  localhost:50051 tokio_php.v1.PhpService/Check
```

### Status Code 14 (UNAVAILABLE)

Server is not reachable or shutting down. Check:
```bash
docker compose ps
docker compose logs --tail=50
```

### Status Code 4 (DEADLINE_EXCEEDED)

Request timeout. Increase timeout in options:
```bash
grpcurl -plaintext -d '{
  "script_path": "slow_script.php",
  "options": {"timeout_ms": 30000}
}' localhost:50051 tokio_php.v1.PhpService/Execute
```

## Examples

Complete client examples are available in `www/examples/grpc/`:

| File | Description |
|------|-------------|
| `test_grpc.sh` | Shell script with grpcurl examples |
| `go_client.go` | Go client example |
| `python_client.py` | Python client example |
| `php_client.php` | PHP client example |
| `api_example.php` | PHP API that works via HTTP and gRPC |

## See Also

- [Health Checks](health-checks.md) — Kubernetes probes
- [Distributed Tracing](distributed-tracing.md) — W3C Trace Context
- [Internal Server](internal-server.md) — Metrics endpoint
- [Configuration](configuration.md) — Environment variables
