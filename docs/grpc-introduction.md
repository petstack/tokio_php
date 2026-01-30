# gRPC for Beginners

A simple explanation of what gRPC is, why you need it, and when to use it.

## What is gRPC?

**gRPC** is a way for programs to communicate with each other. Imagine you have two services: one handles users, another handles orders. They need to exchange data somehow.

```
┌───────────────┐                    ┌───────────────┐
│    Order      │   "Give me user"   │     User      │
│   Service     │ ──────────────────►│    Service    │
│               │◄────────────────── │               │
└───────────────┘  {id: 1, name: ..} └───────────────┘
```

There are two popular ways to do this:

| Method | Data Format | Speed | Simplicity |
|--------|-------------|-------|------------|
| **REST API** | JSON (text) | Slower | Simpler |
| **gRPC** | Protobuf (binary) | Faster | More complex |

## Real-World Analogy

**REST API** is like email correspondence:
- You write a letter in plain language (JSON)
- Send it, wait for a response
- Anyone can read it (human-readable format)

**gRPC** is like a phone call:
- Fast information exchange
- Need to speak the same language (contract)
- Can talk simultaneously (streaming)

## Why Use gRPC?

### 1. Speed

JSON is text. It needs to be converted to a string, transmitted, then parsed back.

```json
{"user_id": 12345, "name": "John", "email": "john@example.com", "active": true}
```
**~80 bytes**

Protobuf is a binary format. The same data takes less space.

```
[binary data]
```
**~25 bytes** (3x smaller!)

With millions of requests, this is huge savings in traffic and time.

### 2. Strict Contract

In REST API you can send anything:

```bash
# Typo in field name — you'll only find out at runtime
curl -X POST /api/users -d '{"naem": "John"}'  # typo in "name"
```

In gRPC there's a schema (`.proto` file) that's checked at compile time:

```protobuf
message User {
  int32 id = 1;
  string name = 2;      // Error will be caught immediately
  string email = 3;
}
```

### 3. Streaming

REST API: one request → one response.

gRPC: data can be streamed:
- Server sends real-time updates
- Client sends a large file in chunks
- Bidirectional chat

## When to Use gRPC?

### Use gRPC when:

| Scenario | Why gRPC is better |
|----------|-------------------|
| Microservices communicating | Faster, less traffic |
| High load (>10K RPS) | CPU savings on parsing |
| Real-time updates | Built-in streaming |
| Mobile application | Battery and traffic savings |
| Strict types matter | Contract checked at compile time |

### Use REST API when:

| Scenario | Why REST is better |
|----------|-------------------|
| Public API | Easier to integrate, any client |
| Web browser | Native fetch/axios support |
| Simple CRUD | Less code, faster development |
| Debug and testing | JSON is human-readable |
| Team doesn't know gRPC | Learning curve |

## Practical Example

### Task

You have an online store. There are three services:
- **API Gateway** — receives requests from browsers
- **User Service** — manages users
- **Order Service** — manages orders

```
┌──────────┐      REST       ┌─────────────┐
│ Browser  │ ───────────────►│ API Gateway │
└──────────┘    (public)     └──────┬──────┘
                                    │
                    ┌───────────────┼───────────────┐
                    │ gRPC          │ gRPC          │
                    ▼               ▼               │
             ┌─────────────┐ ┌─────────────┐        │
             │User Service │ │Order Service│◄───────┘
             └─────────────┘ └─────────────┘
                  (internal services)
```

**Why this setup?**
- Browser → Gateway: REST, because browsers don't support gRPC directly
- Gateway → Services: gRPC, because it's an internal network, speed matters

### Without gRPC (REST everywhere)

```php
// Order Service calls User Service via HTTP
$ch = curl_init('http://user-service/api/users/123');
curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
$response = curl_exec($ch);
$user = json_decode($response, true);

// ~5ms network + ~1ms JSON parsing
```

### With gRPC

```php
// Order Service calls User Service via gRPC
$user = $userClient->GetUser(new GetUserRequest(['id' => 123]));

// ~1ms network + ~0.1ms Protobuf parsing
```

**Savings: ~5x faster per internal request.**

At 1000 requests per second:
- REST: 6 seconds CPU time
- gRPC: 1.1 seconds CPU time

## How gRPC Works in tokio_php

tokio_php allows calling PHP scripts via gRPC. This is useful when:

1. **PHP is part of microservices architecture**

   Other services (Go, Python, Rust) can call PHP via gRPC.

2. **High-load backend**

   Instead of HTTP calls between services — fast gRPC.

3. **Streaming is needed**

   PHP streams data (e.g., report generation).

### Example: Go Service Calls PHP

```go
// Go service (Order Service)
func (s *OrderService) CreateOrder(ctx context.Context, req *pb.CreateOrderRequest) (*pb.Order, error) {
    // Call PHP script via gRPC
    phpResp, err := s.phpClient.Execute(ctx, &php.ExecuteRequest{
        ScriptPath: "api/orders/create.php",
        Method:     "POST",
        FormData: map[string]string{
            "user_id": req.UserId,
            "items":   marshalItems(req.Items),
        },
    })

    if err != nil {
        return nil, err
    }

    // Parse PHP response
    var order pb.Order
    json.Unmarshal(phpResp.Body, &order)
    return &order, nil
}
```

```php
<?php
// api/orders/create.php — regular PHP, works via both HTTP and gRPC

$userId = $_POST['user_id'];
$items = json_decode($_POST['items'], true);

// Business logic for order creation
$order = createOrder($userId, $items);

header('Content-Type: application/json');
echo json_encode($order);
```

**Key point:** PHP code doesn't change! It works the same via HTTP and gRPC.

## Quick Start

### 1. Build tokio_php with gRPC

```bash
CARGO_FEATURES=grpc docker compose build
```

### 2. Run with gRPC server

```bash
GRPC_ADDR=0.0.0.0:50051 docker compose up -d
```

### 3. Verify it works

```bash
# Install grpcurl (gRPC testing tool)
brew install grpcurl  # macOS

# List services
grpcurl -plaintext localhost:50051 list

# Call PHP script
grpcurl -plaintext -d '{
  "script_path": "index.php",
  "method": "GET"
}' localhost:50051 tokio_php.v1.PhpService/Execute
```

### 4. Check service health

```bash
grpcurl -plaintext -d '{}' localhost:50051 tokio_php.v1.PhpService/Check
# {"status": "SERVING"}
```

## Comparing Calls

The same PHP script can be called two ways:

### Via HTTP (REST)

```bash
curl -X POST http://localhost:8080/api/users.php \
  -d "name=John&email=john@example.com"
```

### Via gRPC

```bash
grpcurl -plaintext -d '{
  "script_path": "api/users.php",
  "method": "POST",
  "form_data": {
    "name": "John",
    "email": "john@example.com"
  }
}' localhost:50051 tokio_php.v1.PhpService/Execute
```

**Same result**, but gRPC is faster for server-to-server communication.

## Common Beginner Mistakes

### 1. "Using gRPC for everything"

Wrong:
```
Browser ──gRPC──► Server
```

Correct:
```
Browser ──REST──► API Gateway ──gRPC──► Microservices
```

### 2. "gRPC is complex, I won't bother"

Actually:
- Write `.proto` file once
- Generate code for all languages
- Use like regular functions

### 3. "REST is fast enough"

For 100 requests per second — yes. For 100,000 — the difference is significant.

| Load | REST overhead | gRPC overhead |
|------|---------------|---------------|
| 100 RPS | ~0.5ms | ~0.1ms |
| 10K RPS | ~50ms CPU/sec | ~10ms CPU/sec |
| 100K RPS | ~500ms CPU/sec | ~100ms CPU/sec |

## What's Next?

1. **Try it** — run the example from `www/examples/grpc/`
2. **Study the contract** — look at `proto/php_service.proto`
3. **Write a client** — examples for Go, Python, PHP in documentation

## Glossary

| Term | Simple Explanation |
|------|-------------------|
| **gRPC** | Protocol for programs to communicate, faster than REST |
| **Protobuf** | Binary data format (like JSON, but more compact) |
| **Proto file** | Data schema, describes what can be sent |
| **Streaming** | Sending data in a stream, not all at once |
| **Unary RPC** | One request → one response (like regular HTTP) |
| **Server streaming** | One request → many responses (server sends updates) |
| **Client streaming** | Many requests → one response (client sends file in chunks) |
| **Bidirectional** | Both send data simultaneously (chat) |

## Useful Links

- [gRPC Official Documentation](https://grpc.io/docs/)
- [Protocol Buffers](https://protobuf.dev/)
- [grpcurl — CLI for testing](https://github.com/fullstorydev/grpcurl)
- [tokio_php gRPC Documentation](grpc.md) — technical documentation

## See Also

- [gRPC Technical Documentation](grpc.md) — implementation details, all parameters
- [Architecture](architecture.md) — how tokio_php works
- [Health Checks](health-checks.md) — service health checking
