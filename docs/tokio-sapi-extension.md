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
//     [version] => 0.1.0
//     [workers] => 8
//     [queue_capacity] => 800
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
// Initialize request state
void tokio_sapi_request_init(uint64_t request_id, uint32_t worker_id);

// Finalize request
void tokio_sapi_request_finish(void);

// Clear all superglobals
void tokio_sapi_clear_superglobals(void);

// Build $_REQUEST from $_GET and $_POST
void tokio_sapi_build_request(void);
```

### Superglobal Setters

```c
// Set single variable
void tokio_sapi_set_server_var(const char *name, size_t name_len,
                                const char *value, size_t value_len);
void tokio_sapi_set_get_var(const char *name, size_t name_len,
                             const char *value, size_t value_len);
void tokio_sapi_set_post_var(const char *name, size_t name_len,
                              const char *value, size_t value_len);
void tokio_sapi_set_cookie_var(const char *name, size_t name_len,
                                const char *value, size_t value_len);

// Batch set (key\0value\0key\0value\0...)
void tokio_sapi_set_server_vars_batch(const char *data, size_t len, size_t count);
void tokio_sapi_set_get_vars_batch(const char *data, size_t len, size_t count);
void tokio_sapi_set_post_vars_batch(const char *data, size_t len, size_t count);
void tokio_sapi_set_cookie_vars_batch(const char *data, size_t len, size_t count);

// File upload
void tokio_sapi_set_files_var(const char *field_name, size_t field_name_len,
                               const char *file_name, size_t file_name_len,
                               const char *file_type, size_t file_type_len,
                               const char *tmp_name, size_t tmp_name_len,
                               int error, size_t size);

// POST body for php://input
void tokio_sapi_set_post_data(const char *data, size_t len,
                               const char *content_type, size_t ct_len);
```

### Output Capture

```c
// Start capturing output
void tokio_sapi_start_output_capture(void);

// Get captured output (caller must free)
char* tokio_sapi_get_output(size_t *len);

// Get captured headers (caller must free)
char* tokio_sapi_get_headers(size_t *len);
```

## Performance Comparison

| Executor | Superglobals Time | Throughput |
|----------|-------------------|------------|
| Eval-based (default) | 1-7 µs | ~40,700 req/s |
| FFI-based (USE_EXT=1) | 45-55 µs | ~40,475 req/s |

FFI is ~2% slower overall but provides:
- Individual timing per superglobal
- More direct PHP API access
- Foundation for future optimizations

## Implementation

### Extension Structure

```
ext/
├── tokio_sapi.h      # Header file with API declarations
├── tokio_sapi.c      # Implementation (~500 lines)
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
    pub fn tokio_sapi_request_init(request_id: u64, worker_id: u32);
    pub fn tokio_sapi_set_server_var(
        name: *const c_char, name_len: usize,
        value: *const c_char, value_len: usize
    );
    // ...
}
```

### Batch API

For performance, batch API serializes multiple key-value pairs:

```rust
fn set_server_vars(vars: &[(&str, &str)]) {
    let mut buffer = Vec::new();
    for (key, value) in vars {
        buffer.extend(key.as_bytes());
        buffer.push(0);
        buffer.extend(value.as_bytes());
        buffer.push(0);
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

The extension uses thread-local storage for per-request state:

```c
// Thread-local request state
static __thread uint64_t current_request_id = 0;
static __thread uint32_t current_worker_id = 0;
```

PHP ZTS (TSRM) handles superglobal isolation between threads.

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
curl -H "X-Profile: 1" http://localhost:8080/index.php | grep FFI

# X-Profile-FFI-Clear-Us: 1
# X-Profile-FFI-Server-Us: 45
# X-Profile-FFI-Get-Us: 1
# X-Profile-FFI-Build-Us: 2
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
