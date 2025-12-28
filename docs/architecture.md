# Architecture

tokio_php is a high-performance async web server that executes PHP scripts using the php-embed SAPI.

## System Overview

```
┌───────────────────────────────────────────────────────────┐
│                    tokio_php Process                      │
├───────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐  │
│  │           Main Thread (Tokio Runtime)               │  │
│  │  ┌──────────────────┐  ┌─────────────────────┐      │  │
│  │  │  HTTP Server     │  │  Internal Server    │      │  │
│  │  │  (Hyper)         │  │ (/health, /metrics) │      │  │
│  │  │  Port 8080       │  │  Port 9090          │      │  │
│  │  └────────┬─────────┘  └─────────────────────┘      │  │
│  │           │ async                                   │  │
│  │           ▼                                         │  │
│  │  ┌──────────────────────────────────────────┐       │  │
│  │  │           Request Queue (mpsc)           │       │  │
│  │  └──────────────────────────────────────────┘       │  │
│  └─────────────────────────────────────────────────────┘  │
│                          │                                │
│          ┌───────────────┼───────────────┐                │
│          ▼               ▼               ▼                │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐       │
│  │ PHP Worker 0 │ │ PHP Worker 1 │ │ PHP Worker N │       │
│  │   (thread)   │ │   (thread)   │ │   (thread)   │       │
│  │              │ │              │ │              │       │
│  │ php_request  │ │ php_request  │ │ php_request  │       │
│  │  _startup()  │ │  _startup()  │ │  _startup()  │       │
│  │     ↓        │ │     ↓        │ │     ↓        │       │
│  │  execute()   │ │  execute()   │ │  execute()   │       │
│  │     ↓        │ │     ↓        │ │     ↓        │       │
│  │ php_request  │ │ php_request  │ │ php_request  │       │
│  │ _shutdown()  │ │ _shutdown()  │ │ _shutdown()  │       │
│  └──────────────┘ └──────────────┘ └──────────────┘       │
└───────────────────────────────────────────────────────────┘
```

## Core Components

### Main Thread (Tokio Runtime)

Single-threaded async runtime that handles:
- TCP connection acceptance
- HTTP request parsing (Hyper)
- Response building and sending
- TLS termination (rustls)
- Work distribution to PHP workers

Using single-threaded runtime avoids context switching overhead while PHP workers handle the blocking work.

### HTTP Server (Hyper)

Built on `hyper` with `hyper_util::server::conn::auto::Builder`:
- Automatic HTTP/1.1 and HTTP/2 protocol detection
- HTTP/2 h2c (cleartext) support via `--http2-prior-knowledge`
- HTTPS with ALPN for automatic HTTP/2 negotiation

### Request Queue

Bounded `sync_channel` connecting async server to blocking PHP workers:
- Capacity: `workers × 100` (configurable via `QUEUE_CAPACITY`)
- When full: returns HTTP 503 with `Retry-After: 1`
- Prevents memory exhaustion under load

### PHP Worker Pool

Multi-threaded worker pool using PHP 8.4 ZTS (Thread Safe):
- Each worker runs in a dedicated OS thread
- Workers share OPcache via shared memory
- Round-robin work distribution

Worker lifecycle per request:
1. Receive `ScriptRequest` from queue
2. `php_request_startup()` - initialize request state
3. Inject superglobals via `zend_eval_string` or FFI
4. Execute PHP script
5. Capture output via memfd redirect
6. `php_request_shutdown()` - cleanup
7. Send `ScriptResponse` back

## Request Flow

```
Client Request
     │
     ▼
┌─────────────┐
│  TCP Accept │
└──────┬──────┘
       │
       ▼
┌─────────────┐    TLS?     ┌─────────────┐
│  TLS Check  │────────────▶│TLS Handshake│
└──────┬──────┘     yes     └──────┬──────┘
       │ no                        │
       ▼                           ▼
┌─────────────────────────────────────────┐
│           HTTP Request Parse            │
│  - Method, URI, Headers                 │
│  - Query string → $_GET                 │
│  - Cookies → $_COOKIE                   │
│  - Body → $_POST / $_FILES              │
└──────────────────┬──────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│              Routing                    │
│  - Static file? → serve directly        │
│  - PHP script? → queue to worker        │
│  - Not found? → 404                     │
└──────────────────┬──────────────────────┘
                   │ PHP
                   ▼
┌─────────────────────────────────────────┐
│           Worker Queue                  │
│ - try_send() to bounded channel         │
│ - Queue full? → 503 Service Unavailable │
└──────────────────┬──────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│           PHP Worker                    │
│  1. php_request_startup()               │
│  2. Set superglobals                    │
│  3. Execute script                      │
│  4. Capture output + headers            │
│  5. php_request_shutdown()              │
└──────────────────┬──────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│         Response Building               │
│ - Parse PHP headers (Status, Location)  │
│ - Apply Brotli compression              │
│ - Set Content-Type, Content-Length      │
└──────────────────┬──────────────────────┘
                   │
                   ▼
              Client Response
```

## Executor System

The `ScriptExecutor` trait defines the interface for script execution:

```rust
pub trait ScriptExecutor: Send + Sync {
    fn execute(&self, request: ScriptRequest) -> impl Future<Output = Result<ScriptResponse>>;
}
```

### Available Executors

| Executor | Selection | Method | Performance |
|----------|-----------|--------|-------------|
| `ExtExecutor` | `USE_EXT=1` | `php_execute_script()` + FFI | **~34K RPS** (recommended) |
| `PhpExecutor` | Default | `zend_eval_string()` | ~16K RPS |
| `StubExecutor` | `USE_STUB=1` | No PHP | ~100K RPS (benchmarking) |

**ExtExecutor is 2x faster** because:
- Uses native `php_execute_script()` - fully optimized for OPcache/JIT
- Sets superglobals via direct FFI calls (no eval parsing)
- PhpExecutor re-parses wrapper code on every request

Selection priority in `main.rs`:
1. `USE_STUB=1` → StubExecutor
2. `USE_EXT=1` → ExtExecutor **← recommended for production**
3. Default → PhpExecutor

## Key Technical Decisions

### SAPI Name Override

OPcache disables itself for "embed" SAPI. Solution: change SAPI name to "cli-server" before `php_embed_init`:

```rust
php_embed_module.name = "cli-server\0".as_ptr();
php_embed_init(...);
```

This enables OPcache and JIT, providing ~84% performance improvement.

### Output Capture

PHP output is captured via stdout redirect to memfd:
1. Create memfd (in-memory file)
2. `dup2(memfd_fd, STDOUT_FILENO)`
3. Execute PHP script
4. Restore stdout
5. Read memfd contents

Using memfd instead of pipes avoids deadlock with large outputs (like phpinfo()).

### Header Capture

PHP headers are captured via custom SAPI `header_handler` callback:
- Intercepts all `header()` calls
- Captures `http_response_code()`
- Works even after `exit()` calls

This solves the issue where `header('Location: ...'); exit();` wouldn't work with stdout capture alone.

## Memory Model

```
┌─────────────────────────────────────────────┐
│            Shared Memory                    │
│  ┌───────────────────────────────────────┐  │
│  │           OPcache                     │  │
│  │  - Compiled scripts                   │  │
│  │  - JIT compiled code                  │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
         │             │             │
         ▼             ▼             ▼
    ┌──────────┐  ┌──────────┐  ┌──────────┐
    │ Worker 0 │  │ Worker 1 │  │ Worker N │
    │          │  │          │  │          │
    │ Thread   │  │ Thread   │  │ Thread   │
    │ Local    │  │ Local    │  │ Local    │
    │ Storage  │  │ Storage  │  │ Storage  │
    │ (TSRM)   │  │ (TSRM)   │  │ (TSRM)   │
    └──────────┘  └──────────┘  └──────────┘
```

- **Shared**: OPcache, JIT compiled code
- **Per-thread**: Request state, superglobals (TSRM - Thread Safe Resource Manager)

## Comparison with PHP-FPM

### Benchmark Results

| Server | RPS (bench.php) | RPS (index.php) | Latency |
|--------|-----------------|-----------------|---------|
| **tokio_php** | **35,350** | **32,913** | 2.8ms |
| nginx + PHP-FPM | 13,890 | 12,471 | 7.2ms |

**tokio_php is 2.5x faster than PHP-FPM** (same hardware, 14 workers each, OPcache+JIT enabled).

### Architecture Comparison

| Aspect | tokio_php | PHP-FPM |
|--------|-----------|---------|
| Architecture | Single process, multi-threaded | Multi-process |
| Memory | Shared via TSRM | Copy-on-write per process |
| OPcache | Shared memory (direct) | Shared memory (IPC) |
| Communication | In-process channels | Unix socket/TCP |
| HTTP Server | Built-in (Hyper) | Requires nginx/Apache |

### Request Flow Comparison

```
┌─────────────────────────────────────────────────────────────────┐
│                     nginx + PHP-FPM                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Client ──► nginx ──► FastCGI socket ──► php-fpm (process)      │
│         HTTP    parse    encode/decode      execute              │
│                                                                  │
│  Overhead:                                                       │
│  • nginx HTTP parsing + routing (~1ms)                          │
│  • FastCGI protocol encode/decode (~0.5ms)                      │
│  • Unix socket communication (~0.5ms)                           │
│  • Process context switches                                      │
│  • Response: php-fpm → nginx → client (reverse path)            │
│                                                                  │
│  Total overhead: ~4ms                                            │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                        tokio_php                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Client ──► tokio_php (HTTP + PHP in single process)            │
│         HTTP    Hyper parses, worker thread executes PHP        │
│                                                                  │
│  Advantages:                                                     │
│  • No network hop between web server and PHP                    │
│  • Threads instead of processes (no context switch)             │
│  • Direct shared memory via TSRM                                │
│  • Single binary deployment                                      │
│                                                                  │
│  Total overhead: ~0.1ms                                          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Why tokio_php is Faster

1. **No network hop** — PHP-FPM requires nginx → socket → FPM → socket → nginx. tokio_php handles everything in one process.

2. **Threads vs Processes** — Thread context switches are cheaper than process context switches. No fork() overhead.

3. **No FastCGI protocol** — FastCGI encode/decode adds ~1ms per request.

4. **Direct OPcache access** — Threads share OPcache directly via TSRM, without shared memory IPC.

5. **No reverse proxy** — Response goes directly to client, not through nginx.

### SAPI Differences

| Feature | PHP-FPM SAPI | tokio_php (embed) |
|---------|--------------|-------------------|
| Superglobals | `fcgi_getenv()` from FastCGI env | FFI calls or `zend_eval_string()` |
| Output | Write to FastCGI stream | memfd + stdout redirect |
| Headers | `send_headers` → FastCGI | `header_handler` capture |
| Cookies | FastCGI `HTTP_COOKIE` | Parsed from HTTP request |

### tokio_php Overhead (negligible)

| Operation | Time | Notes |
|-----------|------|-------|
| memfd_create | ~10μs | In-memory file for output capture |
| stdout redirect | ~8μs | dup2() syscalls |
| FFI superglobals | ~40μs | Direct C calls to set $_GET, $_POST, etc. |
| Header capture | ~5μs | Thread-local storage |
| **Total** | **~65μs** | vs ~4ms for nginx+FastCGI |

## Comparison with FrankenPHP

FrankenPHP is a modern PHP application server written in Go, built on Caddy. It embeds PHP directly into the web server using CGO, similar to tokio_php's approach.

### Benchmark Results

| Server | RPS (bench.php) | RPS (index.php) | Latency |
|--------|-----------------|-----------------|---------|
| **tokio_php** | **32,600** | **30,250** | 3.1ms |
| FrankenPHP | 18,350 | 17,530 | 5.5ms |

**tokio_php is 1.8x faster than FrankenPHP** (same hardware, tokio_php 14 workers, FrankenPHP 29 threads).

### Architecture Comparison

| Aspect | tokio_php | FrankenPHP |
|--------|-----------|------------|
| Language | Rust + Tokio | Go + Caddy |
| PHP Integration | php-embed SAPI | CGO bindings |
| Threading Model | Multi-threaded pool | Goroutines |
| HTTP Server | Hyper | Caddy |
| Protocol Support | HTTP/1.1, HTTP/2, HTTPS | HTTP/1-3, HTTPS, QUIC |
| Worker Mode | Always persistent | Optional (worker mode) |

### Request Flow Comparison

```
┌─────────────────────────────────────────────────────────────────┐
│                        FrankenPHP                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Client ──► Caddy (Go) ──► CGO bridge ──► PHP (Zend Engine)     │
│                                                                  │
│  Overhead:                                                       │
│  • CGO call overhead (~1-2ms per request)                       │
│  • Go ↔ C memory copying                                        │
│  • Goroutine scheduling                                          │
│  • Caddy middleware processing                                   │
│                                                                  │
│  Total overhead: ~2.5ms                                          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                        tokio_php                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Client ──► Hyper (Rust) ──► direct FFI ──► PHP (Zend Engine)   │
│                                                                  │
│  Advantages:                                                     │
│  • No CGO overhead (Rust has zero-cost FFI)                     │
│  • Direct memory access, no copying                              │
│  • Predictable thread scheduling                                 │
│  • Minimal HTTP processing                                       │
│                                                                  │
│  Total overhead: ~0.1ms                                          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Why tokio_php is Faster

1. **No CGO overhead** — Go's CGO has significant call overhead (~100-200ns per call). Rust's FFI is zero-cost.

2. **No memory copying** — CGO requires copying data between Go and C heaps. Rust can share memory directly.

3. **Thread-based vs Goroutines** — Goroutines add scheduler overhead. OS threads are more predictable for CPU-bound PHP work.

4. **Minimal HTTP layer** — Hyper is a minimal HTTP implementation. Caddy includes many middleware by default.

5. **Native async** — Tokio's async model maps directly to epoll/kqueue. Go's runtime adds abstraction.

### Feature Comparison

| Feature | tokio_php | FrankenPHP |
|---------|-----------|------------|
| OPcache + JIT | ✓ | ✓ |
| HTTP/2 | ✓ | ✓ |
| HTTP/3 (QUIC) | ✗ | ✓ |
| Worker Mode | Built-in | Optional |
| Early Hints | ✗ | ✓ |
| Automatic HTTPS | ✗ | ✓ (ACME) |
| Mercure Support | ✗ | ✓ |
| Binary Size | ~15MB | ~100MB |

### When to Choose

**Choose tokio_php when:**
- Maximum performance is critical
- You need minimal resource usage
- Deploying behind a reverse proxy (nginx/Traefik)
- Custom integration with Rust ecosystem

**Choose FrankenPHP when:**
- You need HTTP/3 or Early Hints
- Automatic HTTPS with Let's Encrypt is important
- Real-time features with Mercure are needed
- Single binary with batteries included
