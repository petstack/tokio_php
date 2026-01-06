# Worker Pool

tokio_php uses a multi-threaded worker pool for PHP script execution.

## Architecture

```
┌────────────────────────────────────────────────────────┐
│                     Main Thread                        │
│              (Tokio async runtime)                     │
│                        │                               │
│         ┌──────────────┴──────────────┐                │
│         ▼                             ▼                │
│  ┌─────────────┐              ┌─────────────┐          │
│  │ HTTP Server │              │  Internal   │          │
│  │  (Hyper)    │              │   Server    │          │
│  └──────┬──────┘              └─────────────┘          │
│         │                                              │
│         ▼                                              │
│  ┌──────────────────────────────────────────────────┐  │
│  │           Request Queue (sync_channel)           │  │
│  │         Capacity: workers × 100 (configurable)   │  │
│  └───────────────────────┬──────────────────────────┘  │
└──────────────────────────│─────────────────────────────┘
                           │
         ┌─────────────────┼─────────────────┐
         ▼                 ▼                 ▼
  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
  │  Worker 0    │  │  Worker 1    │  │  Worker N    │
  │  (thread)    │  │  (thread)    │  │  (thread)    │
  │              │  │              │  │              │
  │ PHP ZTS      │  │ PHP ZTS      │  │ PHP ZTS      │
  │ Runtime      │  │ Runtime      │  │ Runtime      │
  └──────────────┘  └──────────────┘  └──────────────┘
```

## Configuration

### Worker Count

```bash
# Auto-detect (use all CPU cores)
PHP_WORKERS=0 docker compose up -d

# Fixed number
PHP_WORKERS=4 docker compose up -d
PHP_WORKERS=8 docker compose up -d
```

| Value | Behavior |
|-------|----------|
| `0` (default) | Auto-detect CPU cores via `num_cpus::get()` |
| `N` | Use exactly N workers |

### Queue Capacity

```bash
# Default: workers × 100
docker compose up -d

# Custom capacity
QUEUE_CAPACITY=500 docker compose up -d
QUEUE_CAPACITY=100 docker compose up -d
```

| Value | Behavior |
|-------|----------|
| `0` (default) | `workers × 100` (e.g., 8 workers = 800 capacity) |
| `N` | Fixed queue size |

## How It Works

### Request Processing

1. **Accept**: Main thread accepts TCP connection
2. **Parse**: HTTP request is parsed (method, headers, body)
3. **Queue**: `ScriptRequest` is sent to bounded queue via `try_send()`
4. **Execute**: Available worker receives request, executes PHP
5. **Respond**: `ScriptResponse` sent back to main thread
6. **Send**: HTTP response sent to client

### Worker Lifecycle

Each worker thread runs a loop:

```rust
loop {
    // Wait for request (mutex-protected shared receiver)
    let work = {
        let guard = rx.lock().unwrap();
        guard.recv()
    };

    match work {
        Ok(WorkerRequest { request, response_tx, queued_at, heartbeat_ctx }) => {
            // Start PHP request
            php_request_startup();

            // Set up stdout capture + execute script
            let (capture, timing) = execute_php_script_start(&request, profiling)?;

            // Call php_request_shutdown WHILE stdout captured
            // (captures shutdown handler output), then finalize
            let response = execute_php_script_finish(capture, timing, profiling, queue_wait_us, php_startup_us)?;

            // Get headers via SAPI handler
            let headers = get_captured_headers();

            // Send response
            response_tx.send(ScriptResponse { body: output, headers, .. })?;
        }
        Err(_) => break, // Channel closed, shutdown
    }
}
```

### PHP ZTS (Thread Safe)

Workers use PHP 8.5/8.4 ZTS (Zend Thread Safety):

- **TSRM**: Thread Safe Resource Manager isolates per-request state
- **Shared OPcache**: Compiled scripts shared across threads
- **Thread-local globals**: `$_GET`, `$_POST`, etc. are per-thread

## Overload Handling

### Queue Full

When the queue reaches capacity:

1. `try_send()` fails immediately (non-blocking)
2. Server returns HTTP 503 Service Unavailable
3. `Retry-After: 1` header suggests retry timing
4. `dropped_requests` metric is incremented

```bash
# Small queue for aggressive rejection
QUEUE_CAPACITY=100 docker compose up -d

# Large queue for more buffering
QUEUE_CAPACITY=5000 docker compose up -d
```

### Monitoring Metrics

```bash
curl http://localhost:9090/metrics

# Example output:
tokio_php_pending_requests 15          # Requests waiting
tokio_php_dropped_requests 0           # Requests rejected (503)
tokio_php_responses_total{status="5xx"} 0
```

## Performance Tuning

### Worker Count

| Scenario | Recommendation |
|----------|----------------|
| CPU-bound PHP | `workers = CPU cores` |
| I/O-bound PHP | `workers = CPU cores × 1.5-2` |
| Memory constrained | Reduce workers |

### Queue Capacity

| Scenario | Recommendation |
|----------|----------------|
| Latency-sensitive | Small queue (100-200) |
| Throughput-focused | Large queue (1000+) |
| Bursty traffic | Match expected burst size |

### Benchmark Results

On 14-core CPU:

| Configuration | Requests/sec | Latency |
|---------------|--------------|---------|
| 1 worker | ~6,000 | 15ms |
| 4 workers | ~16,000 | 6ms |
| 14 workers | ~40,000 | 2.5ms |

Each worker handles ~1,000-3,000 req/s depending on script complexity.

## Comparison: Threads vs Processes

tokio_php uses threads (ZTS) instead of processes (like PHP-FPM):

| Aspect | Threads (tokio_php) | Processes (PHP-FPM) |
|--------|---------------------|---------------------|
| Memory | Shared (lower total) | Copy-on-write |
| Startup | Fast (thread spawn) | Slower (fork) |
| Isolation | TSRM (lower) | Full (higher) |
| OPcache | Shared memory | Shared memory |
| Overhead | TSRM locks | IPC communication |
| Performance | ~20% faster | Baseline |

### Historical Context

Earlier tokio_php versions used processes with Unix socket IPC:

```
Process-based (old):
├── Main process (async server)
├── PHP worker process 0
├── PHP worker process 1
└── PHP worker process N
    └── Unix socket IPC with bincode serialization
```

Thread-based (current) is simpler and faster:
- No IPC overhead (~50µs saved per request)
- No socket connect/disconnect
- Simpler error handling

## Implementation Details

### Worker Pool Creation

```rust
// src/executor/common.rs
pub struct WorkerPool {
    request_tx: mpsc::SyncSender<WorkerRequest>,
    workers: Vec<WorkerThread>,
    worker_count: AtomicUsize,
    queue_capacity: usize,
}

pub struct WorkerRequest {
    pub request: ScriptRequest,
    pub response_tx: oneshot::Sender<Result<ScriptResponse, String>>,
    pub queued_at: Instant,
    pub heartbeat_ctx: Option<Arc<HeartbeatContext>>,  // For timeout extension
}

impl WorkerPool {
    pub fn new<F>(num_workers: usize, name_prefix: &str, worker_fn: F) -> Result<Self, String>
    where
        F: Fn(usize, Arc<Mutex<mpsc::Receiver<WorkerRequest>>>) + Send + Clone + 'static,
    {
        Self::with_queue_capacity(num_workers, name_prefix, num_workers * 100, worker_fn)
    }
}
```

### Heartbeat Context

Requests can extend their timeout deadline using `tokio_request_heartbeat()`:

```rust
// src/executor/common.rs
pub struct HeartbeatContext {
    start: Instant,
    deadline_ms: AtomicU64,
    max_extension_secs: u64,
}

impl HeartbeatContext {
    pub fn heartbeat(&self, secs: u64) -> bool {
        if secs == 0 || secs > self.max_extension_secs {
            return false;
        }
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let new_deadline_ms = elapsed_ms + secs * 1000;
        self.deadline_ms.store(new_deadline_ms, Ordering::Release);
        true
    }
}
```

See [Request Heartbeat](request-heartbeat.md) for PHP usage.

### Request Distribution

Workers compete for requests from shared queue (work stealing pattern):

```rust
fn worker_loop(id: usize, rx: Arc<Mutex<Receiver<WorkerRequest>>>) {
    loop {
        let request = {
            let guard = rx.lock().unwrap();
            guard.recv()
        };

        match request {
            Ok(req) => process_request(req),
            Err(_) => break, // Channel closed
        }
    }
}
```

This provides automatic load balancing - idle workers pick up work first.

## Graceful Shutdown

On shutdown signal (SIGTERM/SIGINT):

1. Stop accepting new connections
2. Wait for in-flight requests (up to `DRAIN_TIMEOUT_SECS`)
3. Drop channel sender (unblocks workers)
4. Join worker threads

```bash
# Configure drain timeout (default: 30 seconds)
DRAIN_TIMEOUT_SECS=30 docker compose up -d

# Graceful stop
docker compose down

# Force kill (not recommended)
docker compose kill
```

See [Graceful Shutdown](graceful-shutdown.md) for Kubernetes integration.

## See Also

- [Configuration](configuration.md) - `PHP_WORKERS`, `QUEUE_CAPACITY` environment variables
- [Request Heartbeat](request-heartbeat.md) - Timeout extension mechanism
- [Configuration](configuration.md#request_timeout) - `REQUEST_TIMEOUT` setting
- [Graceful Shutdown](graceful-shutdown.md) - Kubernetes and deployment guide
- [Architecture](architecture.md) - System design overview
- [Internal Server](internal-server.md) - Metrics and health endpoints
