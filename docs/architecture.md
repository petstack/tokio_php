# Architecture

tokio_php is a high-performance async web server that executes PHP scripts using the php-embed SAPI.

## System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         tokio_php Process                           │
├─────────────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                  Main Thread (Tokio Runtime)                  │  │
│  │                                                               │  │
│  │  ┌──────────────────────┐     ┌──────────────────────┐        │  │
│  │  │     HTTP Server      │     │   Internal Server    │        │  │
│  │  │       (Hyper)        │     │  (/health, /metrics) │        │  │
│  │  │   Port 8080/8443     │     │     Port 9090        │        │  │
│  │  └──────────┬───────────┘     └──────────────────────┘        │  │
│  │             │                                                 │  │
│  │             ▼                                                 │  │
│  │  ┌──────────────────────────────────────────────────────┐     │  │
│  │  │                  Middleware Chain                    │     │  │
│  │  │  Rate Limit → Access Log → ... → Error Pages → Brotli│     │  │
│  │  └──────────────────────────┬───────────────────────────┘     │  │
│  │                             │                                 │  │
│  │                             ▼                                 │  │
│  │  ┌──────────────────────────────────────────────────────┐     │  │
│  │  │              Request Queue (sync_channel)            │     │  │
│  │  │           Capacity: workers × 100 (default)          │     │  │
│  │  └──────────────────────────┬───────────────────────────┘     │  │
│  └─────────────────────────────│─────────────────────────────────┘  │
│                                │                                    │
│            ┌───────────────────┼───────────────────┐                │
│            ▼                   ▼                   ▼                │
│  ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐     │
│  │   PHP Worker 0   │ │   PHP Worker 1   │ │   PHP Worker N   │     │
│  │     (thread)     │ │     (thread)     │ │     (thread)     │     │
│  │                  │ │                  │ │                  │     │
│  │  ┌────────────┐  │ │  ┌────────────┐  │ │  ┌────────────┐  │     │
│  │  │ PHP ZTS    │  │ │  │ PHP ZTS    │  │ │  │ PHP ZTS    │  │     │
│  │  │ Runtime    │  │ │  │ Runtime    │  │ │  │ Runtime    │  │     │
│  │  └────────────┘  │ │  └────────────┘  │ │  └────────────┘  │     │
│  └──────────────────┘ └──────────────────┘ └──────────────────┘     │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                    Shared OPcache + JIT                       │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Source Structure

```
src/
├── main.rs              # Entry point, runtime initialization
├── types.rs             # ScriptRequest/ScriptResponse
├── profiler.rs          # Request timing profiler
├── trace_context.rs     # W3C Trace Context (distributed tracing)
├── logging.rs           # JSON logging setup
│
├── server/              # HTTP server
│   ├── mod.rs           # Server struct, main loop
│   ├── config.rs        # ServerConfig
│   ├── connection.rs    # Connection handling, request processing
│   ├── routing.rs       # URL routing, static files
│   ├── internal.rs      # Internal server (/health, /metrics)
│   ├── rate_limit.rs    # Per-IP rate limiting
│   ├── access_log.rs    # Structured access logging
│   ├── error_pages.rs   # Custom error pages cache
│   ├── request/         # Request parsing
│   │   ├── mod.rs       # Request module
│   │   ├── parser.rs    # HTTP request parser
│   │   └── multipart.rs # multipart/form-data
│   └── response/        # Response building
│       ├── mod.rs       # Response builder
│       ├── compression.rs # Brotli compression
│       └── static_file.rs # Static file serving
│
├── middleware/          # Middleware system
│   ├── mod.rs           # Middleware trait
│   ├── chain.rs         # MiddlewareChain
│   ├── rate_limit.rs    # Rate limiting middleware
│   ├── compression.rs   # Brotli compression middleware
│   ├── access_log.rs    # Access logging middleware
│   ├── error_pages.rs   # Custom error pages middleware
│   └── static_cache.rs  # Static file caching middleware
│
├── executor/            # PHP execution backends
│   ├── mod.rs           # ScriptExecutor trait
│   ├── common.rs        # WorkerPool, HeartbeatContext
│   ├── ext.rs           # ExtExecutor (FFI, recommended)
│   ├── php.rs           # PhpExecutor (eval-based)
│   ├── stub.rs          # StubExecutor (benchmarking)
│   └── sapi.rs          # PHP SAPI initialization
│
├── core/                # Core types
│   ├── mod.rs
│   ├── context.rs       # Request context
│   ├── request.rs       # Core request type
│   ├── response.rs      # Core response type
│   └── error.rs         # Error types
│
├── config/              # Configuration
│   ├── mod.rs           # Config aggregation
│   ├── server.rs        # Server config
│   ├── executor.rs      # Executor config
│   ├── middleware.rs    # Middleware config
│   ├── logging.rs       # Logging config
│   ├── parse.rs         # Env var parsing
│   └── error.rs         # Config errors
│
└── listener/            # Network listeners
    ├── mod.rs           # Listener trait
    ├── tcp.rs           # TCP listener
    └── tls.rs           # TLS listener
```

## Core Components

### Main Thread (Tokio Runtime)

Single-threaded async runtime that handles:
- TCP/TLS connection acceptance
- HTTP/1.1 and HTTP/2 request parsing (Hyper)
- Middleware chain execution
- Response building and compression
- Work distribution to PHP workers
- Graceful shutdown coordination

Using single-threaded runtime avoids context switching overhead while PHP workers handle the blocking work.

### HTTP Server (Hyper)

Built on `hyper` with `hyper_util::server::conn::auto::Builder`:
- Automatic HTTP/1.1 and HTTP/2 protocol detection
- HTTP/2 h2c (cleartext) support via `--http2-prior-knowledge`
- HTTPS with ALPN for automatic HTTP/2 negotiation
- TLS 1.3 via rustls

### Middleware Chain

Priority-based middleware execution:
- **Request flow**: Lower priority executes first
- **Response flow**: Higher priority executes first (reverse order)

Built-in middleware:

| Middleware | Priority | Function |
|------------|----------|----------|
| Rate Limit | -100 | Per-IP request throttling |
| Access Log | -90 | Structured JSON logging |
| Static Cache | 50 | Cache-Control headers |
| Error Pages | 90 | Custom HTML error pages |
| Compression | 100 | Brotli compression |

### Request Queue

Bounded `sync_channel` connecting async server to blocking PHP workers:
- Capacity: `workers × 100` (configurable via `QUEUE_CAPACITY`)
- When full: returns HTTP 503 with `Retry-After: 1`
- Prevents memory exhaustion under load

### PHP Worker Pool

Multi-threaded worker pool using PHP 8.5/8.4 ZTS (Thread Safe):
- Each worker runs in a dedicated OS thread
- Workers share OPcache via shared memory
- Work-stealing load balancing

Worker lifecycle per request:
1. Receive `ScriptRequest` from queue
2. `php_request_startup()` - initialize request state
3. Set superglobals via FFI or `zend_eval_string`
4. Execute PHP script via `php_execute_script()` or `zend_eval_string`
5. Capture output via memfd redirect
6. Capture headers via SAPI `header_handler`
7. `php_request_shutdown()` - cleanup
8. Send `ScriptResponse` back

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
┌─────────────┐    TLS?     ┌──────────────────┐
│  TLS Check  │────────────▶│  TLS Handshake   │
└──────┬──────┘     yes     │  (rustls 1.3)    │
       │ no                 └────────┬─────────┘
       ▼                             │
┌────────────────────────────────────┴────────────────┐
│                 HTTP Request Parse                  │
│  - Method, URI, Headers, HTTP version               │
│  - Query string → $_GET                             │
│  - Cookies → $_COOKIE                               │
│  - Body → $_POST / $_FILES                          │
│  - W3C Trace Context (traceparent header)           │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│              Middleware Chain (Request)             │
│  - Rate limiting (429 if exceeded)                  │
│  - Access log start                                 │
│  - Request context setup                            │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│                     Routing                         │
│  - Static file? → serve directly (with caching)    │
│  - PHP script? → queue to worker                   │
│  - INDEX_FILE mode? → route all to index.php       │
│  - Not found? → 404                                │
└──────────────────────────┬──────────────────────────┘
                           │ PHP
                           ▼
┌─────────────────────────────────────────────────────┐
│                   Worker Queue                      │
│  - try_send() to bounded channel                   │
│  - Queue full? → 503 Service Unavailable           │
│  - HeartbeatContext for timeout extension          │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│                    PHP Worker                       │
│  1. php_request_startup()                          │
│  2. Set superglobals (FFI or eval)                 │
│  3. Set $_SERVER[TRACE_ID], $_SERVER[SPAN_ID]      │
│  4. Execute script                                 │
│  5. Capture output + headers                       │
│  6. php_request_shutdown()                         │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│             Middleware Chain (Response)             │
│  - Custom error pages (4xx/5xx)                    │
│  - Static file caching headers                     │
│  - Brotli compression                              │
│  - Access log complete                             │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│                 Response Building                   │
│  - Parse PHP headers (Status, Location)            │
│  - Add X-Request-ID, traceparent                   │
│  - Add profiling headers (if enabled)              │
│  - Set Content-Type, Content-Length                │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
               Client Response
```

## Executor System

The `ScriptExecutor` trait defines the interface for script execution:

```rust
#[async_trait]
pub trait ScriptExecutor: Send + Sync {
    /// Executes a script with the given request data.
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError>;

    /// Returns the name of this executor for logging purposes.
    fn name(&self) -> &'static str;

    /// Shuts down the executor, releasing any resources.
    fn shutdown(&self) {}

    /// Returns true if this executor should skip file existence checks.
    fn skip_file_check(&self) -> bool { false }
}
```

### Available Executors

| Executor | Selection | Method | Best For |
|----------|-----------|--------|----------|
| `ExtExecutor` | `USE_EXT=1` (default) | `php_execute_script()` + FFI | **Real apps (48% faster)** |
| `PhpExecutor` | `USE_EXT=0` | `zend_eval_string()` | Minimal scripts |
| `StubExecutor` | `USE_STUB=1` | No PHP | Benchmarking only |

### Performance Comparison

| Script | PhpExecutor | ExtExecutor | Difference |
|--------|-------------|-------------|------------|
| bench.php (minimal) | **22,821** RPS | 20,420 RPS | PhpExecutor +12% |
| index.php (superglobals) | 17,119 RPS | **25,307** RPS | **ExtExecutor +48%** |

*Benchmark: 14 workers, OPcache+JIT, wrk -t4 -c100 -d10s*

**ExtExecutor is faster for real apps** because:
- FFI batch API sets all `$_SERVER` vars in one C call
- Uses native `php_execute_script()` - fully OPcache/JIT optimized
- No PHP string parsing overhead

**PhpExecutor is faster for minimal scripts** because:
- No tokio_sapi extension overhead (~100µs per request)
- Simple `zend_eval_string()` is very fast for tiny scripts

Selection priority in `main.rs`:
1. `USE_STUB=1` → StubExecutor
2. `USE_EXT=1` (default) → ExtExecutor **← production default**
3. `USE_EXT=0` → PhpExecutor

## Request Heartbeat

Long-running PHP scripts can extend their timeout deadline:

```
┌───────────────────────────────────────────────────────────────┐
│                     HeartbeatContext                          │
├───────────────────────────────────────────────────────────────┤
│  start: Instant         │ Reused from queued_at               │
│  deadline_ms: AtomicU64 │ Milliseconds from start             │
│  max_extension_secs: u64│ = REQUEST_TIMEOUT                   │
└───────────────────────────────────────────────────────────────┘
         │
         │ Shared between async runtime and PHP worker
         │
         ▼
┌─────────────────┐     tokio_request_heartbeat(30)     ┌─────────────────┐
│  Async Runtime  │ ◄───────────────────────────────────│   PHP Worker    │
│                 │     Atomically extends deadline     │                 │
│  Sleeps until   │                                     │  Long-running   │
│  deadline or    │                                     │  script calls   │
│  response ready │                                     │  heartbeat()    │
└─────────────────┘                                     └─────────────────┘
```

Uses `Instant`-based timing (not SystemTime) for minimal syscall overhead.

## Distributed Tracing

W3C Trace Context support for request correlation:

```
Incoming Request                          Outgoing Response
       │                                         │
       ▼                                         ▼
traceparent: 00-{trace_id}-{parent_span}-01     traceparent: 00-{trace_id}-{new_span}-01
                    │                                              │
                    ▼                                              │
           ┌───────────────┐                                       │
           │ TraceContext  │───────────────────────────────────────┘
           │               │
           │ trace_id      │──► $_SERVER['TRACE_ID']
           │ span_id       │──► $_SERVER['SPAN_ID']
           │ parent_span_id│──► $_SERVER['PARENT_SPAN_ID']
           └───────────────┘
```

- Incoming `traceparent` header is parsed and propagated
- New `span_id` generated for this request
- `X-Request-ID` derived from trace: `{trace_id[0:12]}-{span_id[0:4]}`

## Memory Model

```
┌─────────────────────────────────────────────────────────────┐
│                      Shared Memory                          │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                     OPcache                           │  │
│  │  - Compiled PHP scripts (bytecode)                    │  │
│  │  - JIT compiled native code                           │  │
│  │  - Interned strings                                   │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
              │              │              │
              ▼              ▼              ▼
       ┌───────────┐  ┌───────────┐  ┌───────────┐
       │ Worker 0  │  │ Worker 1  │  │ Worker N  │
       │           │  │           │  │           │
       │  Thread   │  │  Thread   │  │  Thread   │
       │  Local    │  │  Local    │  │  Local    │
       │  Storage  │  │  Storage  │  │  Storage  │
       │  (TSRM)   │  │  (TSRM)   │  │  (TSRM)   │
       │           │  │           │  │           │
       │ $_GET     │  │ $_GET     │  │ $_GET     │
       │ $_POST    │  │ $_POST    │  │ $_POST    │
       │ $_SERVER  │  │ $_SERVER  │  │ $_SERVER  │
       │ ...       │  │ ...       │  │ ...       │
       └───────────┘  └───────────┘  └───────────┘
```

- **Shared**: OPcache bytecode, JIT compiled code, interned strings
- **Per-thread (TSRM)**: Request state, superglobals, Zend execution state

## Key Technical Decisions

### SAPI Name Override

OPcache disables itself for "embed" SAPI. Solution: change SAPI name to "cli-server":

```rust
// In sapi.rs
static SAPI_NAME: &[u8] = b"cli-server\0";
php_embed_module.name = SAPI_NAME.as_ptr() as *mut c_char;
php_embed_init(...);
```

This enables OPcache and JIT, providing ~84% performance improvement.

### Output Capture

PHP output is captured via stdout redirect to memfd:
1. Create memfd (in-memory file) via `memfd_create`
2. `dup2(memfd_fd, STDOUT_FILENO)` - redirect stdout
3. Execute PHP script
4. `php_request_shutdown()` while stdout still redirected (captures shutdown handler output)
5. Restore stdout via `dup2`
6. Read memfd contents

Using memfd instead of pipes avoids deadlock with large outputs (like phpinfo()).

### Header Capture

PHP headers are captured via custom SAPI `header_handler` callback:
- Intercepts all `header()` calls
- Captures `http_response_code()`
- Thread-local storage for captured headers
- Works even after `exit()` calls

### Instant-based Timing

HeartbeatContext uses `Instant` instead of `SystemTime`:
- Reuses `queued_at` Instant (zero extra syscalls)
- `deadline_ms` stored as milliseconds from start
- `elapsed()` called only when checking deadline

## See Also

- [Middleware](middleware.md) - Middleware system details
- [Internal Server](internal-server.md) - Health checks and metrics
- [Worker Pool](worker-pool.md) - PHP worker pool configuration
- [Distributed Tracing](distributed-tracing.md) - W3C Trace Context
- [Request Heartbeat](request-heartbeat.md) - Timeout extension mechanism
