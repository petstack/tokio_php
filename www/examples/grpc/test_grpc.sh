#!/bin/bash
# Test tokio_php gRPC service with grpcurl
#
# Prerequisites:
#   brew install grpcurl  # macOS
#   # or download from https://github.com/fullstorydev/grpcurl
#
# Usage:
#   ./test_grpc.sh [host:port]

set -e

HOST="${1:-localhost:50051}"
PROTO_PATH="../../../proto"

echo "=== tokio_php gRPC Test Script ==="
echo "Host: $HOST"
echo ""

# Check if grpcurl is installed
if ! command -v grpcurl &> /dev/null; then
    echo "Error: grpcurl is not installed"
    echo "Install with: brew install grpcurl"
    exit 1
fi

# Check if server is available
echo "1. Checking server connectivity..."
if ! grpcurl -plaintext "$HOST" list &> /dev/null; then
    echo "   Error: Cannot connect to $HOST"
    echo "   Make sure tokio_php is running with gRPC enabled:"
    echo "   GRPC_ADDR=0.0.0.0:50051 cargo run --features grpc"
    exit 1
fi
echo "   Server is reachable"
echo ""

# List services
echo "2. Available services:"
grpcurl -plaintext "$HOST" list
echo ""

# Describe service
echo "3. PhpService methods:"
grpcurl -plaintext "$HOST" describe tokio_php.v1.PhpService
echo ""

# Health check
echo "4. Health check (Check):"
grpcurl -plaintext -d '{"service": ""}' "$HOST" tokio_php.v1.PhpService/Check
echo ""

# Execute GET request
echo "5. Execute GET request:"
grpcurl -plaintext -d '{
  "script_path": "index.php",
  "method": "GET",
  "query_params": {
    "page": "1",
    "limit": "10"
  }
}' "$HOST" tokio_php.v1.PhpService/Execute
echo ""

# Execute POST request
echo "6. Execute POST request:"
grpcurl -plaintext -d '{
  "script_path": "api/users.php",
  "method": "POST",
  "form_data": {
    "name": "John Doe",
    "email": "john@example.com"
  },
  "server_vars": {
    "HTTP_AUTHORIZATION": "Bearer token123"
  },
  "options": {
    "timeout_ms": 5000
  }
}' "$HOST" tokio_php.v1.PhpService/Execute
echo ""

# Execute with JSON body
echo "7. Execute with JSON body:"
grpcurl -plaintext -d '{
  "script_path": "api/data.php",
  "method": "POST",
  "body": "eyJrZXkiOiAidmFsdWUifQ==",
  "content_type": "application/json"
}' "$HOST" tokio_php.v1.PhpService/Execute
echo ""

# Streaming (if supported)
echo "8. Execute streaming request (will timeout after 5 chunks or 5 seconds):"
timeout 5 grpcurl -plaintext -d '{
  "script_path": "stream.php",
  "method": "GET"
}' "$HOST" tokio_php.v1.PhpService/ExecuteStream || true
echo ""

echo "=== All tests completed ==="
