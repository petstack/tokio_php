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
├── lib.rs               # Library entry point
├── bridge.rs            # Bridge FFI bindings (libtokio_bridge)
├── types.rs             # ScriptRequest/ScriptResponse
├── profiler.rs          # Request timing profiler
├── trace_context.rs     # W3C Trace Context (distributed tracing)
├── logging.rs           # JSON logging setup
│
├── server/              # HTTP server
│   ├── mod.rs           # Server struct, main loop
│   ├── config.rs        # ServerConfig
│   ├── connection.rs    # Connection handling, request processing
│   ├── routing.rs       # URL routing (RouteConfig, RouteResult)
│   ├── file_cache.rs    # LRU file cache (FileType, FileCache)
│   ├── internal.rs      # Internal server (/health, /metrics)
│   ├── access_log.rs    # Structured access logging
│   ├── error_pages.rs   # Custom error pages cache
│   ├── request/         # Request parsing
│   │   ├── mod.rs       # Request module
│   │   ├── parser.rs    # HTTP request parser
│   │   └── multipart.rs # multipart/form-data
│   └── response/        # Response building
│       ├── mod.rs       # Response builder
│       ├── compression.rs # Brotli compression
│       ├── static_file.rs # Static file serving
│       └── streaming.rs # SSE streaming (MeteredChunkFrameStream for metrics)
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

### Routing System

The routing system determines how requests are handled based on `INDEX_FILE` configuration.

#### Components

| Component | File | Description |
|-----------|------|-------------|
| `RouteConfig` | `routing.rs` | Configuration with document_root, index_file, is_php flag |
| `RouteResult` | `routing.rs` | Enum: `Execute(path)`, `Serve(path)`, `NotFound` |
| `FileCache` | `file_cache.rs` | LRU cache (200 entries) for file metadata |
| `FileType` | `file_cache.rs` | Enum: `File`, `Dir` |

#### Routing Modes

| Mode | INDEX_FILE | Behavior |
|------|------------|----------|
| **Traditional** | _(empty)_ | Direct mapping, index.php → index.html → 404 |
| **Framework** | `index.php` | All → index.php, blocks all .php access |
| **SPA** | `index.html` | All → index.html (static), PHP still works |

#### FileCache (LRU)

Reduces filesystem `stat()` syscalls:

```rust
pub struct FileCache {
    entries: RwLock<HashMap<Box<str>, Option<FileType>>>,  // path → type
    order: RwLock<Vec<Box<str>>>,                          // LRU order
    capacity: usize,                                        // 200
}
```

- **Cache hit**: Returns cached `FileType` (File, Dir, or None)
- **Cache miss**: Calls `stat()`, caches result, returns
- **Eviction**: Removes oldest entry when at capacity
- **Thread-safe**: `RwLock` for concurrent access

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
3. `bridge::init_ctx()` - initialize bridge TLS context
4. Set heartbeat callback via `bridge::set_heartbeat()`
5. Set stream finish callback via `bridge::set_stream_finish_callback()`
6. Initialize streaming state via `sapi::init_stream_state()`
7. Set superglobals via FFI or `zend_eval_string`
8. Execute PHP script via `php_execute_script()` or `zend_eval_string`
9. Output streamed via SAPI `ub_write` callback
10. Headers captured via SAPI `header_handler`
11. `php_request_shutdown()` - cleanup
12. `sapi::finalize_stream()` - send End chunk if not finished
13. `bridge::destroy_ctx()` - cleanup bridge context

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
│              Routing (resolve_request)              │
│  1. Decode URI, sanitize path                       │
│  2. Direct INDEX_FILE access? → 404                 │
│  3. INDEX_FILE=*.php and uri=*.php? → 404           │
│  4. Check FileCache (LRU, 200 entries)              │
│  5. File exists → Execute(php) or Serve(static)     │
│  6. INDEX_FILE set → fallback to index file         │
│  7. Not found → 404                                 │
└──────────────────────────┬──────────────────────────┘
                           │ PHP
                           ▼
┌─────────────────────────────────────────────────────┐
│                   Worker Queue                      │
│  - try_send() to bounded channel                    │
│  - Queue full? → 503 Service Unavailable            │
│  - HeartbeatContext for timeout extension           │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│                    PHP Worker                       │
│  1. php_request_startup()                           │
│  2. Init bridge context (request_id, worker_id)     │
│  3. Set superglobals (FFI or eval)                  │
│  4. Set $_SERVER[TRACE_ID], $_SERVER[SPAN_ID]       │
│  5. Execute script                                  │
│     - tokio_finish_request() → response sent        │
│     - tokio_request_heartbeat() → extend timeout    │
│  6. php_request_shutdown()                          │
│  7. Finalize: check bridge finish state             │
│  8. Destroy bridge context                          │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│             Middleware Chain (Response)             │
│  - Custom error pages (4xx/5xx)                     │
│  - Static file caching headers                      │
│  - Brotli compression                               │
│  - Access log complete                              │
└──────────────────────────┬──────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│                 Response Building                   │
│  - Parse PHP headers (Status, Location)             │
│  - Add X-Request-ID, traceparent                    │
│  - Add profiling headers (if enabled)               │
│  - Set Content-Type, Content-Length                 │
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
| `ExtExecutor` | `EXECUTOR=ext` (default) | `php_execute_script()` + FFI | **Real apps (48% faster)** |
| `PhpExecutor` | `EXECUTOR=php` | `zend_eval_string()` | Minimal scripts |
| `StubExecutor` | `EXECUTOR=stub` | No PHP | Benchmarking only |

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

Selection via `EXECUTOR` env var in `main.rs`:
- `EXECUTOR=ext` → ExtExecutor **← production default**
- `EXECUTOR=php` → PhpExecutor
- `EXECUTOR=stub` → StubExecutor

## Request Heartbeat

Long-running PHP scripts can extend their timeout deadline via the bridge:

```
┌───────────────────────────────────────────────────────────────┐
│                     HeartbeatContext                          │
├───────────────────────────────────────────────────────────────┤
│  start: Instant         │ Reused from queued_at               │
│  deadline_ms: AtomicU64 │ Milliseconds from start             │
│  max_extension_secs: u64│ = REQUEST_TIMEOUT                   │
└───────────────────────────────────────────────────────────────┘
         │
         │ Pointer + callback registered in bridge TLS
         │
         ▼
┌─────────────────┐                                     ┌─────────────────┐
│  Async Runtime  │                                     │   PHP Worker    │
│                 │                                     │                 │
│  Sleeps until   │     ┌──────────────────────┐        │  Long-running   │
│  deadline or    │◄────│  libtokio_bridge.so  │◄───────│  script calls   │
│  response ready │     │                      │        │  heartbeat(30)  │
│                 │     │  __thread ctx:       │        │                 │
│  Callback sets  │     │  - heartbeat_ctx     │        │  Bridge invokes │
│  AtomicU64      │     │  - heartbeat_callback│        │  callback via   │
│  deadline_ms    │     └──────────────────────┘        │  shared TLS     │
└─────────────────┘                                     └─────────────────┘
```

Uses `Instant`-based timing (not SystemTime) for minimal syscall overhead.
Bridge provides shared TLS context accessible from both Rust and PHP.

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

## Observability

### Metrics Architecture

The internal server (`/metrics`) exports Prometheus-compatible metrics:

```
┌─────────────────────────────────────────────────────────────────┐
│                      tokio_php Process                           │
├─────────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   RequestMetrics                          │  │
│  │                                                           │  │
│  │  Atomic counters (lock-free):                             │  │
│  │  - requests_total{method}  - responses_total{status}      │  │
│  │  - pending_requests        - dropped_requests             │  │
│  │  - response_time_total_us  - response_count               │  │
│  │                                                           │  │
│  │  SSE metrics:                                             │  │
│  │  - sse_active              - sse_total                    │  │
│  │  - sse_chunks              - sse_bytes                    │  │
│  └───────────────────────────────────────────────────────────┘  │
│                          │                                      │
│                          ▼                                      │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                 Internal Server (:9090)                   │  │
│  │                                                           │  │
│  │  /health  - JSON health status                            │  │
│  │  /metrics - Prometheus format                             │  │
│  │  /config  - Server configuration                          │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Monitoring Stack                              │
│                                                                 │
│  ┌─────────────────┐              ┌─────────────────┐           │
│  │   Prometheus    │──────────────│    Grafana      │           │
│  │   (:9091)       │   scrape     │    (:3000)      │           │
│  │                 │   every 5s   │                 │           │
│  │  - Alerting     │              │  - Dashboards   │           │
│  │  - Recording    │              │  - Alerts       │           │
│  └─────────────────┘              └─────────────────┘           │
└─────────────────────────────────────────────────────────────────┘
```

### SSE Metrics Tracking

SSE connections use `MeteredChunkFrameStream` wrapper for accurate metrics:

```rust
// src/server/response/streaming.rs
pub struct MeteredChunkFrameStream {
    inner: ChunkFrameStream,
    metrics: Arc<RequestMetrics>,
}

impl Stream for MeteredChunkFrameStream {
    fn poll_next(...) -> Poll<Option<...>> {
        // On each chunk: metrics.sse_chunk_sent(bytes)
    }
}

impl Drop for MeteredChunkFrameStream {
    fn drop(&mut self) {
        // On stream end: metrics.sse_connection_ended()
    }
}
```

Metrics lifecycle:
1. `sse_connection_started()` - when SSE stream created
2. `sse_chunk_sent(bytes)` - on each chunk sent (in `poll_next`)
3. `sse_connection_ended()` - when stream dropped (in `Drop`)

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `tokio_php_uptime_seconds` | gauge | Server uptime |
| `tokio_php_requests_per_second` | gauge | Lifetime average RPS |
| `tokio_php_response_time_avg_seconds` | gauge | Average response time |
| `tokio_php_active_connections` | gauge | Active HTTP connections |
| `tokio_php_pending_requests` | gauge | Queue depth |
| `tokio_php_dropped_requests` | counter | Queue overflow count |
| `tokio_php_requests_total{method}` | counter | Requests by method |
| `tokio_php_responses_total{status}` | counter | Responses by status |
| `tokio_php_sse_active_connections` | gauge | Active SSE streams |
| `tokio_php_sse_connections_total` | counter | Total SSE connections |
| `tokio_php_sse_chunks_total` | counter | Total SSE chunks sent |
| `tokio_php_sse_bytes_total` | counter | Total SSE bytes sent |
| `node_load1/5/15` | gauge | System load average |
| `node_memory_*` | gauge | System memory stats |

See [Observability](observability.md) for Grafana setup and PromQL examples.

## SSE Streaming

Server-Sent Events (SSE) support allows PHP scripts to stream data to clients in real-time.

### Detection Methods

SSE streaming is enabled automatically in two ways:

1. **Client Accept header** (explicit): `Accept: text/event-stream`
2. **PHP Content-Type header** (auto-detect): `header('Content-Type: text/event-stream')`

The auto-detect method allows PHP scripts to enable SSE without requiring special client headers.

### Request Flow

```
┌──────────────────────────────────────────────────────────────────────┐
│                    SSE Request Flow (Auto-Detect)                    │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Client                    Server                      PHP Worker    │
│    │                         │                              │        │
│    │  GET /sse.php           │                              │        │
│    │ ───────────────────────►│                              │        │
│    │                         │                              │        │
│    │                         │  Create StreamingChannel     │        │
│    │                         │  set_stream_callback(ctx,cb) │        │
│    │                         │ ────────────────────────────►│        │
│    │                         │                              │        │
│    │                         │  header('Content-Type:       │        │
│    │                         │  text/event-stream');        │        │
│    │                         │                              │        │
│    │                         │  SAPI header_handler detects │        │
│    │                         │  → try_enable_streaming()    │        │
│    │                         │ ◄────────────────────────────│        │
│    │                         │                              │        │
│    │  HTTP 200               │  First chunk detected        │        │
│    │  Content-Type:          │  → switch to streaming mode  │        │
│    │  text/event-stream      │                              │        │
│    │ ◄───────────────────────│                              │        │
│    │                         │                              │        │
│    │                         │     echo "data: ...\n\n";    │        │
│    │                         │     flush();                 │        │
│    │                         │                              │        │
│    │                         │  SAPI flush handler:         │        │
│    │                         │  - Flush PHP output buffers  │        │
│    │                         │  - send_chunk(data)          │        │
│    │                         │ ◄────────────────────────────│        │
│    │  data: ...              │                              │        │
│    │ ◄───────────────────────│                              │        │
│    │                         │                              │        │
│    │         ... repeat for each flush() ...                │        │
│    │                         │                              │        │
│    │                         │  Script ends                 │        │
│    │                         │  end_stream()                │        │
│    │                         │ ◄────────────────────────────│        │
│    │  (connection closes)    │                              │        │
│    │ ◄───────────────────────│                              │        │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### How It Works

**Explicit SSE (Accept header):**
1. Server detects `Accept: text/event-stream` header
2. Calls `bridge::enable_streaming()` immediately
3. Returns streaming response

**Auto-detect SSE (Content-Type header):**
1. Server creates streaming channel for ALL PHP requests
2. Calls `bridge::set_stream_callback()` (callback set but streaming not enabled)
3. PHP script calls `header('Content-Type: text/event-stream')`
4. SAPI `header_handler` detects Content-Type and calls `try_enable_streaming()`
5. First `flush()` sends first chunk via callback
6. Server detects first chunk → switches to streaming response mode

**Common flow after streaming enabled:**
1. PHP calls `flush()` → SAPI flush handler sends chunks via callback
2. Callback pushes to `mpsc::channel`
3. Hyper polls channel and sends frames to client
4. `bridge::end_stream()` called when script ends

### Usage in PHP

```php
<?php
// Auto-detect mode: just set header
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');

while ($hasData) {
    $event = json_encode(['time' => date('H:i:s')]);
    echo "data: $event\n\n";
    flush();  // Standard PHP flush() - triggers streaming
    sleep(1);
}
```

Key points:
- Standard `flush()` works via SAPI flush handler
- No custom functions needed
- Works without special client headers (auto-detect via Content-Type)
- Works with EventSource API in browsers

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

## Bridge Architecture

The bridge (`libtokio_bridge.so`) solves TLS (Thread-Local Storage) isolation between Rust and PHP:

```
┌─────────────────────────────────────────────────────────────────┐
│                      tokio_php (Rust)                           │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                  libtokio_bridge.so                       │  │
│  │                                                           │  │
│  │  static __thread tokio_bridge_ctx_t *tls_ctx;             │  │
│  │                                                           │  │
│  │  Context per request:                                     │  │
│  │  - request_id, worker_id                                  │  │
│  │  - is_finished, output_offset, response_code              │  │
│  │  - heartbeat_ctx, heartbeat_callback                      │  │
│  │  - finish_ctx, finish_callback (legacy)                   │  │
│  │  - stream_finish_ctx, stream_finish_callback (new)        │  │
│  │  - is_streaming, stream_ctx, stream_callback (SSE)        │  │
│  │                                                           │  │
│  └───────────────────────────────────────────────────────────┘  │
│         ↑                                     ↑                 │
│         │                                     │                 │
│    Rust FFI                           PHP Extension             │
│  (src/bridge.rs)                    (ext/tokio_sapi.c)          │
│                                                                 │
│  - bridge::init_ctx()              - tokio_finish_request()     │
│  - bridge::set_heartbeat()         - tokio_request_heartbeat()  │
│  - bridge::get_finish_info()                                    │
│  - bridge::destroy_ctx()                                        │
└─────────────────────────────────────────────────────────────────┘
```

**Why a shared library?**

Without the bridge, Rust (statically linked) and PHP extension (dynamically loaded by libphp.so) have separate TLS storage:
- Rust cannot read values set by PHP (`tokio_finish_request()`)
- PHP cannot invoke callbacks registered by Rust (`heartbeat`)

The shared library provides a single TLS context accessible to both.

See [tokio_sapi Extension](tokio-sapi-extension.md#bridge-architecture) for implementation details.

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

### Output Capture (Streaming)

PHP output is captured via the SAPI `ub_write` callback for real-time streaming:

1. `sapi::init_stream_state(tx)` - initialize streaming channel
2. Execute PHP script
3. Each `echo`/`print` triggers `ub_write` callback
4. `ub_write` sends `ResponseChunk::Headers` on first output (if not sent)
5. `ub_write` sends `ResponseChunk::Body` for each output chunk
6. `php_request_shutdown()` - cleanup
7. `sapi::finalize_stream()` sends `ResponseChunk::End`

Benefits of streaming over memfd:
- Output sent to client immediately (no buffering)
- Enables `tokio_finish_request()` for early response
- Supports SSE without special handling
- PHP's output buffering (`ob_*`) still works (data flows through when flushed)

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

- [Docker](docker.md) - Dockerfiles, build targets, deployment
- [Middleware](middleware.md) - Middleware system details
- [Internal Server](internal-server.md) - Health checks and metrics
- [Observability](observability.md) - Monitoring stack, Grafana, OpenTelemetry
- [Worker Pool](worker-pool.md) - PHP worker pool configuration
- [Distributed Tracing](distributed-tracing.md) - W3C Trace Context
- [Request Heartbeat](request-heartbeat.md) - Timeout extension mechanism
- [SSE Streaming](sse-streaming.md) - Server-Sent Events support
- [tokio_sapi Extension](tokio-sapi-extension.md) - Bridge architecture and PHP functions
