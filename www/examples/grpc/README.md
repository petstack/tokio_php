# gRPC Examples

Examples demonstrating how to call tokio_php gRPC service from various clients.

## Server Setup

Build and run tokio_php with gRPC support:

```bash
# Build with gRPC feature
cargo build --release --features grpc

# Or with Docker (add GRPC_ADDR to docker-compose.yml)
GRPC_ADDR=0.0.0.0:50051 docker compose up -d
```

## Testing with grpcurl

```bash
# Install grpcurl
brew install grpcurl  # macOS
# or download from https://github.com/fullstorydev/grpcurl

# Execute PHP script
grpcurl -plaintext -d '{
  "script_path": "index.php",
  "method": "GET",
  "query_params": {"page": "1", "limit": "10"}
}' localhost:50051 tokio_php.v1.PhpService/Execute

# Health check
grpcurl -plaintext localhost:50051 tokio_php.v1.PhpService/Check

# List available services
grpcurl -plaintext localhost:50051 list

# Describe service
grpcurl -plaintext localhost:50051 describe tokio_php.v1.PhpService
```

## PHP Client (using grpc extension)

See `php_client.php` for a complete example using the official PHP gRPC extension.

### Installation

```bash
# Install PHP gRPC extension
pecl install grpc
pecl install protobuf

# Add to php.ini
extension=grpc.so
extension=protobuf.so

# Install composer dependencies
composer require grpc/grpc google/protobuf
```

## Go Client

See `go_client.go` for a Go client example.

## Python Client

See `python_client.py` for a Python client example.

## Proto File

The service definition is in `proto/php_service.proto`:

```protobuf
service PhpService {
  rpc Execute(ExecuteRequest) returns (ExecuteResponse);
  rpc ExecuteStream(ExecuteRequest) returns (stream StreamChunk);
  rpc Check(HealthCheckRequest) returns (HealthCheckResponse);
}
```
