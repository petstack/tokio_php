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

| Aspect | tokio_php | PHP-FPM |
|--------|-----------|---------|
| Architecture | Single process, multi-threaded | Multi-process |
| Memory | Shared via TSRM | Copy-on-write per process |
| OPcache | Shared memory | Shared memory |
| Communication | In-process channels | Unix socket/TCP |
| HTTP Server | Built-in (Hyper) | Requires nginx/Apache |
| Performance | ~40K req/s | ~500 req/s per worker |
