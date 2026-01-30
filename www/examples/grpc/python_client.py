#!/usr/bin/env python3
"""
Python gRPC Client for tokio_php

This example demonstrates how to call the tokio_php gRPC service from Python.

Requirements:
    pip install grpcio grpcio-tools

Generate proto classes:
    python -m grpc_tools.protoc \
        -I../../../proto \
        --python_out=. \
        --grpc_python_out=. \
        ../../../proto/php_service.proto

Usage:
    python python_client.py
"""

import os
import sys

# Try to import generated classes, fall back to raw gRPC if not available
try:
    import php_service_pb2
    import php_service_pb2_grpc
    PROTO_GENERATED = True
except ImportError:
    PROTO_GENERATED = False

import grpc


def main():
    host = os.environ.get('GRPC_HOST', 'localhost:50051')
    print(f"=== tokio_php Python gRPC Client ===")
    print(f"Connecting to: {host}\n")

    # Create channel
    channel = grpc.insecure_channel(host)

    if PROTO_GENERATED:
        run_with_generated_classes(channel)
    else:
        run_with_raw_grpc(channel)

    channel.close()


def run_with_generated_classes(channel):
    """Use protoc-generated classes (recommended)"""
    print("Using generated proto classes\n")

    stub = php_service_pb2_grpc.PhpServiceStub(channel)

    # 1. Health check
    print("1. Health Check:")
    try:
        request = php_service_pb2.HealthCheckRequest(service="")
        response = stub.Check(request)
        status_names = {0: "UNKNOWN", 1: "SERVING", 2: "NOT_SERVING", 3: "SERVICE_UNKNOWN"}
        print(f"   Status: {status_names.get(response.status, response.status)}")
    except grpc.RpcError as e:
        print(f"   Error: {e.details()} (code: {e.code()})")

    print()

    # 2. Execute PHP script
    print("2. Execute Script:")
    try:
        request = php_service_pb2.ExecuteRequest(
            script_path="index.php",
            method="GET",
            query_params={"page": "1", "limit": "10"},
            options=php_service_pb2.RequestOptions(
                timeout_ms=5000,
                enable_profiling=False,
            )
        )
        response = stub.Execute(request)
        print(f"   Status Code: {response.status_code}")
        print(f"   Headers: {dict(response.headers)}")
        print(f"   Body length: {len(response.body)} bytes")
        if response.metadata:
            print(f"   Request ID: {response.metadata.request_id}")
            print(f"   Execution time: {response.metadata.execution_time_us}µs")
    except grpc.RpcError as e:
        print(f"   Error: {e.details()} (code: {e.code()})")

    print()

    # 3. Execute with POST data
    print("3. Execute POST Request:")
    try:
        request = php_service_pb2.ExecuteRequest(
            script_path="api/users.php",
            method="POST",
            form_data={
                "name": "John Doe",
                "email": "john@example.com",
            },
            server_vars={
                "HTTP_AUTHORIZATION": "Bearer token123",
            },
            cookies={
                "session_id": "abc123",
            },
        )
        response = stub.Execute(request)
        print(f"   Status Code: {response.status_code}")
        print(f"   Body: {response.body.decode('utf-8', errors='replace')[:200]}...")
    except grpc.RpcError as e:
        print(f"   Error: {e.details()} (code: {e.code()})")

    print()

    # 4. Streaming response (SSE simulation)
    print("4. Streaming Response:")
    try:
        request = php_service_pb2.ExecuteRequest(
            script_path="stream.php",
            method="GET",
        )
        stream = stub.ExecuteStream(request)
        chunk_count = 0
        for chunk in stream:
            chunk_count += 1
            print(f"   Chunk {chunk.sequence}: {len(chunk.data)} bytes, final={chunk.is_final}")
            if chunk.is_final or chunk_count >= 5:
                break
    except grpc.RpcError as e:
        print(f"   Error: {e.details()} (code: {e.code()})")


def run_with_raw_grpc(channel):
    """Fallback when proto classes are not generated"""
    print("Proto classes not generated. Run:")
    print("  python -m grpc_tools.protoc -I../../../proto \\")
    print("      --python_out=. --grpc_python_out=. \\")
    print("      ../../../proto/php_service.proto\n")

    print("Attempting raw gRPC health check...\n")

    # Raw gRPC call example
    try:
        # Health check with empty request
        response = channel.unary_unary(
            '/tokio_php.v1.PhpService/Check',
            request_serializer=lambda x: x,
            response_deserializer=lambda x: x,
        )(b'')
        print(f"Raw response: {response.hex()}")
    except grpc.RpcError as e:
        print(f"Error: {e.details()} (code: {e.code()})")


if __name__ == '__main__':
    main()
