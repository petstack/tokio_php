# tokio_sapi PHP Extension

tokio_php includes an optional PHP extension (`tokio_sapi`) that provides FFI-based superglobals and runtime information.

## Overview

The extension provides:
- **C API**: Functions for setting superglobals directly via FFI
- **PHP functions**: Access to request/worker information
- **Performance**: Alternative to eval-based superglobals

## Enabling the Extension

```bash
USE_EXT=1 docker compose up -d
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

// Heartbeat (internal, used by tokio_request_heartbeat())
echo $_SERVER['TOKIO_HEARTBEAT_CTX'];       // Hex pointer to context
echo $_SERVER['TOKIO_HEARTBEAT_MAX_SECS'];  // Max extension (= REQUEST_TIMEOUT)
echo $_SERVER['TOKIO_HEARTBEAT_CALLBACK'];  // Hex pointer to callback
?>
```

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

## Performance Comparison

ExtExecutor is **2x faster** than PhpExecutor due to different script execution methods:

| Executor | Method | RPS (index.php) | RPS (bench.php) |
|----------|--------|-----------------|-----------------|
| **ExtExecutor** (USE_EXT=1) | `php_execute_script()` | **33,677** | **37,911** |
| PhpExecutor (default) | `zend_eval_string()` | 16,208 | 19,555 |

**Why ExtExecutor is faster:**

1. **`php_execute_script()`** - Native PHP file execution, fully optimized for OPcache/JIT
2. **FFI superglobals** - Direct C calls to set `$_GET`, `$_POST`, `$_SERVER`, etc.
3. **No parsing overhead** - PhpExecutor re-parses wrapper code on every request

**Production recommendation:**
```bash
USE_EXT=1 docker compose up -d
```

## Implementation

### Extension Structure

```
ext/
├── tokio_sapi.h      # Header file with API declarations
├── tokio_sapi.c      # Implementation (~900 lines)
└── config.m4         # phpize configuration
```

### Building

The extension is built automatically in the Docker image:

```dockerfile
# In Dockerfile
RUN cd /app/ext && \
    phpize84 && \
    ./configure --with-php-config=/usr/bin/php-config84 && \
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
// src/executor/ext_ffi.rs
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

## Thread Safety

The extension uses C11 `__thread` for thread-local storage (not PHP module globals):

```c
// Thread-local request context
static __thread tokio_request_context *tls_request_ctx = NULL;
static __thread uint64_t tls_request_id = 0;

// Heartbeat context for request timeout extension
static __thread void *tls_heartbeat_ctx = NULL;
static __thread uint64_t tls_heartbeat_max_secs = 0;
static __thread tokio_heartbeat_fn_t tls_heartbeat_callback = NULL;

// Cached superglobal array pointers (avoids repeated lookups)
static __thread zval *cached_superglobal_arrs[6] = {NULL};
```

PHP ZTS (TSRM) handles superglobal isolation between threads. Using `__thread` instead of PHP module globals ensures proper thread-local storage when called from external (Rust) worker threads.

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

With `PROFILE=1`, FFI timing is included:

```bash
curl -sI -H "X-Profile: 1" http://localhost:8080/index.php | grep FFI

# X-Profile-FFI-Request-Init-Us: 5
# X-Profile-FFI-Clear-Us: 1
# X-Profile-FFI-Server-Us: 45
# X-Profile-FFI-Get-Us: 1
# X-Profile-FFI-Build-Request-Us: 2
```

## Limitations

- `tokio_async_call()` is not yet implemented
- Session handler not implemented
- php://input requires explicit `tokio_sapi_set_post_data()` call

## Future Plans

1. **Async PHP-to-Rust calls**: Enable PHP to call async Rust functions
2. **Session handler**: Implement `$_SESSION` via shared memory
3. **Output streaming**: Direct output capture without stdout redirect
4. **Performance optimization**: Reduce FFI overhead further

## See Also

- [Configuration](configuration.md) - `USE_EXT` environment variable
- [Superglobals](superglobals.md) - PHP superglobals support
- [Request Heartbeat](request-heartbeat.md) - `tokio_request_heartbeat()` documentation
- [Architecture](architecture.md) - Executor system overview
- [Profiling](profiling.md) - Performance profiling headers
