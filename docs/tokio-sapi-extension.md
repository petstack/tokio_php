# tokio_sapi PHP Extension

tokio_php includes an optional PHP extension (`tokio_sapi`) that provides FFI-based superglobals, streaming support, and runtime information.

## Overview

The extension provides:
- **C API**: Functions for setting superglobals directly via FFI
- **PHP functions**: Access to request/worker information, streaming, early response
- **Performance**: Alternative to eval-based superglobals
- **Streaming**: SSE support and chunked transfer encoding

## Enabling the Extension

The extension is enabled by default (`EXECUTOR=ext`):

```bash
# Default - ExtExecutor with tokio_sapi
docker compose up -d

# Disable extension (use legacy PhpExecutor)
EXECUTOR=php docker compose up -d
```

When enabled, `ExtExecutor` is used instead of `PhpExecutor`.

## PHP Functions

### tokio_request_id()

Returns the current request ID (unique per request).

```php
<?php
$id = tokio_request_id();
echo "Request ID: $id"; // Request ID: 42
?>
```

### tokio_worker_id()

Returns the current worker thread ID.

```php
<?php
$worker = tokio_worker_id();
echo "Worker: $worker"; // Worker: 3
?>
```

### tokio_server_info()

Returns server information as an array.

```php
<?php
$info = tokio_server_info();
print_r($info);
// Array (
//     [server] => tokio_php
//     [version] => 0.1.0
//     [sapi] => tokio_sapi
//     [zts] => 1
//     [build] => 0.1.0 (abc12345)
// )
?>
```

### tokio_request_heartbeat()

Extends the request timeout deadline for long-running operations. See [Request Heartbeat](request-heartbeat.md) for full documentation.

```php
<?php
// Extend deadline by 30 seconds (must be <= REQUEST_TIMEOUT)
set_time_limit(30);
$success = tokio_request_heartbeat(30);

if ($success) {
    echo "Deadline extended\n";
}
?>
```

**Parameters:**
- `int $time = 10` - Seconds to extend deadline

**Returns:** `bool` - `true` on success, `false` if timeout disabled or value exceeds limit.

### tokio_finish_request()

Sends the response to the client immediately, but continues executing the script in the background. Analog of `fastcgi_finish_request()` in PHP-FPM.

```php
<?php
// Send response to client
echo "Your request is being processed!\n";
header("X-Status: accepted");

// Client receives response NOW
tokio_finish_request();

// Everything below runs in background (client doesn't wait):
sleep(5);                          // Slow operation
send_email($user, $notification);  // Send notification
log_to_database($analytics);       // Log analytics
cleanup_temp_files();              // Cleanup
?>
```

**Use cases:**
- Webhook handlers that need to respond quickly (within timeout)
- Sending emails/notifications after response
- Background logging and analytics
- Cleanup operations
- Any slow task that shouldn't block the response

**Behavior:**
- All output buffers are flushed to capture the response body
- Headers set before `tokio_finish_request()` are included
- Headers set after are **NOT** sent to client
- Output after `tokio_finish_request()` is **NOT** sent to client
- Script continues executing until completion
- The function is idempotent (calling multiple times has no effect)

**Returns:** `bool` - Always returns `true`.

**Example: Webhook Handler**

```php
<?php
// Respond to webhook within 5 seconds (required by many services)
http_response_code(200);
echo json_encode(['status' => 'accepted']);

tokio_finish_request();  // Webhook service gets 200 OK immediately

// Process webhook payload in background (may take minutes)
$payload = json_decode(file_get_contents('php://input'), true);
process_webhook($payload);  // Slow processing
notify_admins($payload);    // Send notifications
?>
```

### tokio_send_headers()

Sends HTTP headers immediately and enables chunked streaming mode. After calling this function, all output is sent to the client in real-time via `flush()`.

```php
<?php
header('Content-Type: application/json');
header('Cache-Control: no-cache');

// Send headers NOW, enable streaming mode
tokio_send_headers();

// Each output + flush() is sent immediately to client
echo json_encode(['event' => 1, 'time' => time()]) . "\n";
flush();

sleep(2);

echo json_encode(['event' => 2, 'time' => time()]) . "\n";
flush();
?>
```

**Parameters:**
- `int $status = 200` - HTTP status code to send

**Returns:** `bool` - `true` on success, `false` if headers already sent or invalid status code.

**Behavior:**
- Disables PHP output buffering
- Enables `Transfer-Encoding: chunked` (no `Content-Length`)
- Sends headers immediately via `sapi_send_headers()`
- Enables implicit flush for subsequent output

**Use cases:**
- Long-polling responses
- JSON streaming (NDJSON)
- Progress updates
- Any Content-Type that needs real-time output

### tokio_stream_flush()

Flushes output buffer and sends data to client immediately in SSE streaming mode.

```php
<?php
// SSE streaming example
// Client sends: Accept: text/event-stream

while ($has_data) {
    $data = json_encode(['time' => date('H:i:s'), 'value' => rand()]);
    echo "data: $data\n\n";
    tokio_stream_flush();  // Send immediately to client
    sleep(1);
}
?>
```

**Returns:** `bool` - `true` on success, `false` if streaming mode is not enabled.

**Note:** With the SAPI flush handler installed, standard `flush()` also works for streaming. This function is kept for explicit streaming control and backward compatibility.

### tokio_is_streaming()

Check if streaming mode is enabled for the current request.

```php
<?php
if (tokio_is_streaming()) {
    // Running in SSE mode
    echo "data: streaming mode active\n\n";
    tokio_stream_flush();
}
?>
```

**Returns:** `bool` - `true` if streaming mode is enabled.

### tokio_async_call()

Placeholder for future async PHP-to-Rust calls (not yet implemented).

```php
<?php
// Future: Call Rust async functions from PHP
$result = tokio_async_call('http_get', 'https://api.example.com');
?>
```

## $_SERVER Variables

The extension adds additional `$_SERVER` variables:

```php
<?php
// Request identification
echo $_SERVER['TOKIO_REQUEST_ID'];          // Current request ID
echo $_SERVER['TOKIO_WORKER_ID'];           // Current worker ID

// Server build version with git commit hash
echo $_SERVER['TOKIO_SERVER_BUILD_VERSION']; // "0.1.0 (abc12345)" or "0.1.0 (abc12345-dirty)"
?>
```

**Note:** Heartbeat functionality uses the bridge library's TLS context instead of `$_SERVER` variables. See [Bridge Architecture](#bridge-architecture) for details.

## C API Reference

The extension exposes C functions for Rust FFI:

### Request Lifecycle

```c
// Initialize request state (returns SUCCESS/FAILURE)
int tokio_sapi_request_init(uint64_t request_id);

// Shutdown request (frees thread-local context)
void tokio_sapi_request_shutdown(void);

// Clear all superglobals
void tokio_sapi_clear_superglobals(void);

// Initialize superglobal caches (call once before batch operations)
void tokio_sapi_init_superglobals(void);

// Build $_REQUEST from $_GET and $_POST
void tokio_sapi_build_request(void);

// Initialize request state (headers, output buffering)
void tokio_sapi_init_request_state(void);
```

### Superglobal Setters

```c
// Set single variable
void tokio_sapi_set_server_var(const char *key, size_t key_len,
                                const char *value, size_t value_len);
void tokio_sapi_set_get_var(const char *key, size_t key_len,
                             const char *value, size_t value_len);
void tokio_sapi_set_post_var(const char *key, size_t key_len,
                              const char *value, size_t value_len);
void tokio_sapi_set_cookie_var(const char *key, size_t key_len,
                                const char *value, size_t value_len);

// Batch set - returns number of variables set
// Buffer format: [key_len:u32][key\0][val_len:u32][val]...
int tokio_sapi_set_server_vars_batch(const char *buffer, size_t buffer_len, size_t count);
int tokio_sapi_set_get_vars_batch(const char *buffer, size_t buffer_len, size_t count);
int tokio_sapi_set_post_vars_batch(const char *buffer, size_t buffer_len, size_t count);
int tokio_sapi_set_cookie_vars_batch(const char *buffer, size_t buffer_len, size_t count);

// Ultra-batch - set ALL superglobals in one call
// Performs: clear, init caches, set all vars, build $_REQUEST, init request state
void tokio_sapi_set_all_superglobals(
    const char *server_buf, size_t server_len, size_t server_count,
    const char *get_buf, size_t get_len, size_t get_count,
    const char *post_buf, size_t post_len, size_t post_count,
    const char *cookie_buf, size_t cookie_len, size_t cookie_count);

// File upload (strings are null-terminated)
void tokio_sapi_set_files_var(const char *field, size_t field_len,
                               const char *name, const char *type,
                               const char *tmp_name, int error, size_t size);

// POST body for php://input
void tokio_sapi_set_post_data(const char *data, size_t len);
```

### Header Access

```c
// Get number of captured headers
int tokio_sapi_get_header_count(void);

// Get header name by index (0-based)
const char* tokio_sapi_get_header_name(int index);

// Get header value by index (0-based)
const char* tokio_sapi_get_header_value(int index);

// Get HTTP response code
int tokio_sapi_get_response_code(void);

// Add a header (called internally or from Rust)
void tokio_sapi_add_header(const char *name, size_t name_len,
                           const char *value, size_t value_len, int replace);

// Set response code
void tokio_sapi_set_response_code(int code);
```

### Script Execution

```c
// Execute PHP script (returns SUCCESS/FAILURE)
int tokio_sapi_execute_script(const char *path);
```

### Finish Request API

```c
// Check if tokio_finish_request() was called
int tokio_sapi_is_request_finished(void);

// Get byte offset where output should be truncated
size_t tokio_sapi_get_finished_offset(void);

// Get header count at finish time
int tokio_sapi_get_finished_header_count(void);

// Get response code at finish time
int tokio_sapi_get_finished_response_code(void);
```

### SAPI Flush Handler

```c
// Called by PHP's flush() function
// When streaming mode is enabled, sends new output to client
void tokio_sapi_flush(void *server_context);
```

## SAPI Lifecycle

PHP extensions have standard lifecycle callbacks. In tokio_php, the lifecycle is managed differently because Rust controls request timing.

### Standard PHP SAPI Callbacks

```
┌─────────────────────────────────────────────────────────────────┐
│                     PHP Extension Lifecycle                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  MINIT (Module Init)     - Once when extension loads             │
│       │                                                          │
│       ▼                                                          │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    REQUEST LOOP                          │    │
│  │                                                          │    │
│  │  RINIT (activate)    - Start of each request             │    │
│  │       │                                                  │    │
│  │       ▼                                                  │    │
│  │  PHP Script Execution                                    │    │
│  │       │                                                  │    │
│  │       ▼                                                  │    │
│  │  RSHUTDOWN (deactivate) - End of each request            │    │
│  │                                                          │    │
│  └─────────────────────────────────────────────────────────┘    │
│       │                                                          │
│       ▼                                                          │
│  MSHUTDOWN (Module Shutdown) - When extension unloads            │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### tokio_sapi Implementation

In `tokio_sapi.c`, the standard PHP hooks are minimal:

```c
PHP_RINIT_FUNCTION(tokio_sapi)   // activate
{
    return SUCCESS;  // Empty - Rust manages lifecycle
}

PHP_RSHUTDOWN_FUNCTION(tokio_sapi)  // deactivate
{
    /* Don't free context here - Rust manages lifecycle */
    return SUCCESS;
}
```

**Why?** Rust controls when requests start/end through its own FFI functions.

### Rust-Managed Request Lifecycle

```
┌─────────────────────────────────────────────────────────────────┐
│                 tokio_php Request Lifecycle                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Rust Worker Thread                                              │
│  ───────────────────────────────────────────────────────────    │
│                                                                  │
│  1. set_request_data()           // Set data BEFORE startup      │
│         │                                                        │
│         ▼                                                        │
│  2. php_request_startup()        // PHP init (RINIT called)      │
│         │                         // → register_server_variables │
│         │                         // → $_SERVER populated        │
│         ▼                                                        │
│  3. tokio_sapi_request_init()    // FFI: init TLS context        │
│         │                                                        │
│         ▼                                                        │
│  4. tokio_sapi_set_all_superglobals()  // Batch set variables    │
│         │                                                        │
│         ▼                                                        │
│  5. tokio_sapi_execute_script()  // Execute PHP script           │
│         │                                                        │
│         ▼                                                        │
│  6. tokio_sapi_request_shutdown() // FFI: cleanup TLS context    │
│         │                          // → Reset superglobal cache  │
│         ▼                                                        │
│  7. php_request_shutdown()       // PHP cleanup (RSHUTDOWN)      │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Key Points

1. **Timing**: Rust sets request data BEFORE `php_request_startup()` so SAPI callbacks can access it
2. **TLS Ownership**: Thread-local storage is managed by extension, not PHP's TSRM
3. **Bridge Library**: Shared TLS context between Rust (static) and PHP extension (dynamic)
4. **Cache Reset**: Superglobal array pointers are reset after `php_request_shutdown()` to prevent use-after-free

### Thread-Local Storage

All request state uses C11 `__thread` storage:

```c
// ext/tokio_sapi.c
static __thread tokio_request_context *tls_request_ctx = NULL;
static __thread uint64_t tls_request_id = 0;
static __thread zval *cached_superglobal_arrs[6] = {NULL};
```

This provides:
- Per-worker-thread isolation
- No locks between threads
- Proper ZTS compatibility

## Performance Comparison

Performance depends on script complexity:

| Script | PhpExecutor | ExtExecutor | Difference |
|--------|-------------|-------------|------------|
| bench.php (minimal) | **22,821** RPS | 20,420 RPS | PhpExecutor +12% |
| index.php (superglobals) | 17,119 RPS | **25,307** RPS | **ExtExecutor +48%** |

*Benchmark: 14 workers, OPcache+JIT, wrk -t4 -c100 -d10s*

**ExtExecutor is faster for real apps** because:

1. **FFI batch API** - Sets all `$_SERVER` vars in one C call
2. **`php_execute_script()`** - Native PHP execution, fully OPcache/JIT optimized
3. **No string parsing** - PhpExecutor builds and parses PHP code every request

**PhpExecutor is faster for minimal scripts** because:

1. **No extension overhead** - tokio_sapi adds ~100µs per request init/shutdown
2. **Simple eval** - For tiny scripts, `zend_eval_string()` is very fast

**Production recommendation:** Most apps use superglobals, so ExtExecutor is recommended.
```bash
docker compose up -d  # EXECUTOR=ext by default
```

## Implementation

### Extension Structure

```
ext/
├── bridge/
│   ├── bridge.h      # Bridge API declarations
│   ├── bridge.c      # Bridge implementation (TLS context)
│   └── Makefile      # Build libtokio_bridge.so
├── tokio_sapi.h      # Extension header with API declarations
├── tokio_sapi.c      # Extension implementation (~1000 lines)
└── config.m4         # phpize configuration
```

### Building

The extension is built automatically in the Docker image:

```dockerfile
# In Dockerfile

# 1. Build bridge library first
WORKDIR /app/ext/bridge
RUN make && make install

# 2. Build PHP extension
WORKDIR /app/ext
RUN phpize && \
    ./configure --enable-tokio_sapi LDFLAGS="-L/usr/local/lib -ltokio_bridge" && \
    make && \
    make install
```

### Loading

The extension is loaded via php.ini:

```ini
extension=tokio_sapi.so
```

## Rust FFI Bindings

The Rust side uses FFI to call extension functions:

```rust
// src/executor/ext.rs
extern "C" {
    pub fn tokio_sapi_request_init(request_id: u64) -> c_int;
    pub fn tokio_sapi_request_shutdown();
    pub fn tokio_sapi_set_server_var(
        key: *const c_char, key_len: usize,
        value: *const c_char, value_len: usize
    );
    // ...
}
```

### Batch API

For performance, batch API serializes multiple key-value pairs with length prefixes:

```rust
fn set_server_vars(vars: &[(&str, &str)]) {
    let mut buffer = Vec::new();
    for (key, value) in vars {
        // Key length (u32, includes null terminator)
        buffer.extend(&((key.len() + 1) as u32).to_ne_bytes());
        buffer.extend(key.as_bytes());
        buffer.push(0);
        // Value length (u32)
        buffer.extend(&(value.len() as u32).to_ne_bytes());
        buffer.extend(value.as_bytes());
    }

    unsafe {
        tokio_sapi_set_server_vars_batch(
            buffer.as_ptr() as *const c_char,
            buffer.len(),
            vars.len()
        );
    }
}
```

## Bridge Architecture

The extension uses a shared library (`libtokio_bridge.so`) to solve TLS (Thread-Local Storage) isolation between Rust and PHP:

```
┌─────────────────────────────────────────────────────────┐
│                    tokio_php (Rust binary)              │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │              libtokio_bridge.so                  │   │
│  │                                                  │   │
│  │  __thread bridge_ctx;   // Shared TLS           │   │
│  │  - request_id, worker_id                        │   │
│  │  - finish_request state                         │   │
│  │  - heartbeat callback                           │   │
│  │                                                  │   │
│  └─────────────────────────────────────────────────┘   │
│        ↑                              ↑                 │
│        │                              │                 │
│   Rust FFI                     PHP Extension            │
│   (dlopen)                     (dlopen via PHP)         │
└─────────────────────────────────────────────────────────┘
```

**Why a shared library?**

Without the shared library, Rust (statically linked) and PHP extension (dynamically loaded) have separate TLS storage. This means:
- Rust can't read values set by PHP
- PHP can't access callbacks set by Rust

The shared library provides a single TLS context that both can access.

### Bridge Files

```
ext/
├── bridge/
│   ├── bridge.h      # Public API
│   ├── bridge.c      # Implementation
│   └── Makefile      # Build libtokio_bridge.so
├── tokio_sapi.c      # PHP extension (uses bridge)
├── tokio_sapi.h
└── config.m4
```

## Thread Safety

The extension uses the bridge library for shared thread-local storage:

```c
// Bridge provides shared TLS context (ext/bridge/bridge.c)
static __thread tokio_bridge_ctx_t *tls_ctx = NULL;

// tokio_sapi.c uses bridge for finish_request, heartbeat
tokio_bridge_mark_finished(offset, headers, code);
tokio_bridge_send_heartbeat(secs);
```

PHP ZTS (TSRM) handles superglobal isolation between threads. The bridge library ensures proper thread-local storage when called from external (Rust) worker threads.

## Debugging

### Check Extension is Loaded

```php
<?php
if (function_exists('tokio_request_id')) {
    echo "tokio_sapi extension loaded\n";
    echo "Request ID: " . tokio_request_id() . "\n";
}
?>
```

### Extension Info

```php
<?php
phpinfo();
// Look for "tokio_sapi" section
?>
```

### FFI Profiling

When built with `debug-profile` feature, FFI timing is included in profile reports:

```bash
# Build with profiling
CARGO_FEATURES=debug-profile docker compose build
docker compose up -d

# Make request and view report
curl http://localhost:8080/index.php
docker compose exec tokio_php cat /tmp/tokio_profile_request_*.md
```

**FFI timing in profile reports:**

| Field | Description |
|-------|-------------|
| `FFI Clear` | Superglobals clear time |
| `$_SERVER (N items)` | $_SERVER set time and count |
| `$_GET (N items)` | $_GET set time and count |
| `$_POST (N items)` | $_POST set time and count |
| `$_COOKIE (N items)` | $_COOKIE set time and count |
| `$_FILES (N items)` | $_FILES set time and count |
| `Build Request` | $_REQUEST build time |
| `Init Eval` | Init eval time |

See [Profiling](profiling.md) for full report format.

## Limitations

- `tokio_async_call()` is not yet implemented
- Session handler not implemented (`$_SESSION` requires session handler)
- php://input requires explicit `tokio_sapi_set_post_data()` call

## Future Plans

1. **HTTP 103 Early Hints**: Pending [hyper support](https://github.com/hyperium/hyper/issues/2426)
2. **Async PHP-to-Rust calls**: Enable PHP to call async Rust functions
3. **Session handler**: Implement `$_SESSION` via shared memory or Redis
4. **Performance optimization**: Reduce bridge overhead further

## See Also

- [Configuration](configuration.md) - `EXECUTOR` environment variable
- [Superglobals](superglobals.md) - PHP superglobals support
- [Request Heartbeat](request-heartbeat.md) - `tokio_request_heartbeat()` documentation
- [SSE Streaming](sse-streaming.md) - Server-Sent Events streaming
- [Architecture](architecture.md) - Executor system overview
- [Profiling](profiling.md) - Compile-time profiling with markdown reports
