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
    // Wait for request
    let request = rx.recv()?;

    // PHP request lifecycle
    php_request_startup();

    // Set superglobals
    inject_superglobals(&request);

    // Execute script
    execute_file(&request.script_path);

    // Capture output
    let output = capture_output();

    // Capture headers
    let headers = capture_headers();

    php_request_shutdown();

    // Send response
    tx.send(ScriptResponse { output, headers, status })?;
}
```

### PHP ZTS (Thread Safe)

Workers use PHP 8.4 ZTS (Zend Thread Safety):

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
    tx: mpsc::SyncSender<WorkerRequest>,
    handles: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    pub fn new(worker_count: usize, queue_capacity: usize) -> Self {
        let capacity = if queue_capacity == 0 {
            worker_count * 100
        } else {
            queue_capacity
        };

        let (tx, rx) = mpsc::sync_channel(capacity);
        let rx = Arc::new(Mutex::new(rx));

        let handles: Vec<_> = (0..worker_count)
            .map(|id| {
                let rx = Arc::clone(&rx);
                thread::spawn(move || worker_loop(id, rx))
            })
            .collect();

        Self { tx, handles }
    }
}
```

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

On shutdown signal (SIGTERM):

1. Stop accepting new connections
2. Finish in-flight requests
3. Drop channel sender (unblocks workers)
4. Join worker threads

```bash
# Graceful stop
docker compose down

# Force kill (not recommended)
docker compose kill
```
