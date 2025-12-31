# Request Profiling

tokio_php includes a built-in request profiler for detailed performance analysis.

## Enabling Profiler

Set `PROFILE=1` environment variable (see [Configuration](configuration.md)):

```bash
PROFILE=1 docker compose up -d
```

## Usage

Send requests with `X-Profile: 1` header to get timing data:

```bash
# HTTP profiling
curl -sI -H "X-Profile: 1" http://localhost:8080/index.php

# HTTPS profiling (includes TLS metrics)
curl -sIk -H "X-Profile: 1" https://localhost:8443/index.php
```

## Profile Headers

All times are in microseconds (µs).

### Summary

| Header | Description |
|--------|-------------|
| `X-Profile-Total-Us` | Total request processing time |
| `X-Profile-HTTP-Version` | HTTP protocol version (HTTP/1.0, HTTP/1.1, HTTP/2.0) |

### TLS Metrics (HTTPS only)

| Header | Description |
|--------|-------------|
| `X-Profile-TLS-Handshake-Us` | TLS handshake time |
| `X-Profile-TLS-Protocol` | TLS version (TLSv1_2, TLSv1_3) |
| `X-Profile-TLS-ALPN` | ALPN negotiated protocol (h2, http/1.1) |

### Parse Breakdown

| Header | Description |
|--------|-------------|
| `X-Profile-Parse-Us` | Total parse time |
| `X-Profile-Parse-Headers-Us` | Extract HTTP headers |
| `X-Profile-Parse-Query-Us` | Parse query string ($_GET) |
| `X-Profile-Parse-Cookies-Us` | Parse cookies |
| `X-Profile-Parse-Body-Read-Us` | Read POST body |
| `X-Profile-Parse-Body-Parse-Us` | Parse POST body (form/multipart) |
| `X-Profile-Parse-ServerVars-Us` | Build $_SERVER vars |
| `X-Profile-Parse-Path-Us` | URL decode + path resolution |
| `X-Profile-Parse-FileCheck-Us` | Path::exists() check |

### Queue & PHP Startup

| Header | Description |
|--------|-------------|
| `X-Profile-Queue-Us` | Time waiting in worker queue |
| `X-Profile-PHP-Startup-Us` | php_request_startup() time |

### Superglobals

| Header | Description |
|--------|-------------|
| `X-Profile-Superglobals-Us` | Total superglobals injection time |
| `X-Profile-Superglobals-Build-Us` | Build PHP code string (eval mode) |
| `X-Profile-Superglobals-Eval-Us` | zend_eval_string execution (eval mode) |

### FFI Metrics (USE_EXT=1 only)

When using ExtExecutor, detailed FFI timing is available:

| Header | Description |
|--------|-------------|
| `X-Profile-FFI-Request-Init-Us` | tokio_sapi_request_init() |
| `X-Profile-FFI-Clear-Us` | tokio_sapi_clear_superglobals() |
| `X-Profile-FFI-Server-Us` | All $_SERVER FFI calls |
| `X-Profile-FFI-Server-Count` | Number of $_SERVER entries |
| `X-Profile-FFI-Get-Us` | All $_GET FFI calls |
| `X-Profile-FFI-Get-Count` | Number of $_GET entries |
| `X-Profile-FFI-Post-Us` | All $_POST FFI calls |
| `X-Profile-FFI-Post-Count` | Number of $_POST entries |
| `X-Profile-FFI-Cookie-Us` | All $_COOKIE FFI calls |
| `X-Profile-FFI-Cookie-Count` | Number of $_COOKIE entries |
| `X-Profile-FFI-Files-Us` | All $_FILES FFI calls |
| `X-Profile-FFI-Files-Count` | Number of $_FILES entries |
| `X-Profile-FFI-Build-Request-Us` | tokio_sapi_build_request() |
| `X-Profile-FFI-Init-Eval-Us` | Init eval (header_remove, ob_start) |

### Script Execution

| Header | Description |
|--------|-------------|
| `X-Profile-Memfd-Setup-Us` | memfd_create + stdout redirect |
| `X-Profile-Script-Us` | php_execute_script time |

### Output Capture

| Header | Description |
|--------|-------------|
| `X-Profile-Output-Us` | Total output capture time |
| `X-Profile-Output-Finalize-Us` | Finalize eval (flush + headers) |
| `X-Profile-Output-Restore-Us` | Restore stdout |
| `X-Profile-Output-Read-Us` | Read from memfd |
| `X-Profile-Output-Parse-Us` | Parse body + headers from output |

### Shutdown & Response

| Header | Description |
|--------|-------------|
| `X-Profile-PHP-Shutdown-Us` | php_request_shutdown() time |
| `X-Profile-Response-Us` | Build HTTP response |

## Example Output

### HTTP Request

```bash
$ curl -sI -H "X-Profile: 1" http://localhost:8080/index.php | grep X-Profile

X-Profile-Total-Us: 1735
X-Profile-HTTP-Version: HTTP/1.1
X-Profile-Parse-Us: 45
X-Profile-Queue-Us: 169
X-Profile-PHP-Startup-Us: 205
X-Profile-Superglobals-Us: 85
X-Profile-Script-Us: 124
X-Profile-Output-Us: 91
X-Profile-PHP-Shutdown-Us: 56
X-Profile-Response-Us: 12
```

### HTTPS Request with HTTP/2

```bash
$ curl -sIk -H "X-Profile: 1" https://localhost:8443/index.php | grep X-Profile

X-Profile-Total-Us: 228
X-Profile-HTTP-Version: HTTP/2.0
X-Profile-TLS-Handshake-Us: 10916
X-Profile-TLS-Protocol: TLSv1_3
X-Profile-TLS-ALPN: h2
X-Profile-Parse-Us: 32
X-Profile-Queue-Us: 45
X-Profile-PHP-Startup-Us: 198
X-Profile-Script-Us: 115
X-Profile-Output-Us: 85
X-Profile-PHP-Shutdown-Us: 52
```

## Understanding the Metrics

### Bottleneck Identification

| High Metric | Likely Cause | Solution |
|-------------|--------------|----------|
| Queue-Us | Workers overloaded | Increase PHP_WORKERS |
| PHP-Startup-Us | Extensions loading | Reduce loaded extensions |
| Superglobals-Us | Eval overhead | Use USE_EXT=1 (FFI mode) |
| Script-Us | Slow PHP code | Optimize code, enable OPcache/JIT |
| Output-Us | Large output | Reduce output size |
| TLS-Handshake-Us | TLS overhead | Use keep-alive connections |

### Comparing Eval vs FFI Mode

```bash
# Eval-based (default)
X-Profile-Superglobals-Us: 236

# FFI-based (USE_EXT=1) - shows individual timing
X-Profile-FFI-Server-Us: 45
X-Profile-FFI-Server-Count: 25
X-Profile-FFI-Get-Us: 1
X-Profile-FFI-Get-Count: 2
```

FFI mode provides granular timing for each superglobal, useful for debugging.

## Benchmarking

### Without Profiling (Recommended for Benchmarks)

```bash
# Disable profiling for accurate benchmarks
PROFILE=0 docker compose up -d

wrk -t4 -c100 -d10s http://localhost:8080/index.php
```

### With Profiling

Profiling adds ~5-10% overhead due to timing measurements:

```bash
PROFILE=1 docker compose up -d

# Profile 10 requests
for i in {1..10}; do
  curl -sI -H "X-Profile: 1" http://localhost:8080/index.php | grep X-Profile-Total
done
```

## Implementation

### ProfileData Structure

```rust
// src/profiler.rs
pub struct ProfileData {
    pub total_us: u64,

    // Connection & TLS
    pub tls_handshake_us: u64,
    pub http_version: String,
    pub tls_protocol: String,
    pub tls_alpn: String,

    // Parse breakdown
    pub parse_request_us: u64,
    pub headers_extract_us: u64,
    pub query_parse_us: u64,
    // ... more fields

    // PHP execution
    pub php_startup_us: u64,
    pub superglobals_us: u64,
    pub script_exec_us: u64,
    pub output_capture_us: u64,
    pub php_shutdown_us: u64,

    // Response
    pub response_build_us: u64,
}
```

### Timer Helper

```rust
pub struct Timer {
    start: Instant,
    last: Instant,
}

impl Timer {
    pub fn new() -> Self { /* ... */ }

    /// Mark a phase and return elapsed microseconds since last mark
    pub fn mark(&mut self) -> u64 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_micros() as u64;
        self.last = now;
        elapsed
    }

    /// Get total elapsed microseconds since start
    pub fn total(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}
```

## Best Practices

1. **Disable in production** - Profiling adds overhead
2. **Sample requests** - Don't profile every request under load
3. **Compare consistently** - Profile same endpoint multiple times
4. **Watch queue time** - High queue time indicates worker bottleneck
5. **Check TLS impact** - First request has handshake, subsequent don't
6. **Use FFI mode** - `USE_EXT=1` provides detailed superglobal timing

## See Also

- [Configuration](configuration.md) - PROFILE environment variable
- [Internal Server](internal-server.md) - Prometheus metrics for monitoring
- [Worker Pool](worker-pool.md) - Queue and worker configuration
- [HTTP/2 & TLS](http2-tls.md) - TLS configuration
- [Architecture](architecture.md) - System overview
