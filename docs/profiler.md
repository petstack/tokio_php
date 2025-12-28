# Request Profiler

tokio_php includes a built-in request profiler for performance analysis.

## Enabling Profiler

Set `PROFILE=1` environment variable:

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

### Core Timing

| Header | Description |
|--------|-------------|
| `X-Profile-Total-Us` | Total request processing time |
| `X-Profile-Parse-Us` | HTTP request parsing time |
| `X-Profile-Queue-Us` | Time waiting in worker queue |
| `X-Profile-PHP-Startup-Us` | `php_request_startup()` time |
| `X-Profile-Script-Us` | PHP script execution time |
| `X-Profile-Output-Us` | Output buffer capture time |
| `X-Profile-PHP-Shutdown-Us` | `php_request_shutdown()` time |

### Protocol Info

| Header | Description | Values |
|--------|-------------|--------|
| `X-Profile-HTTP-Version` | HTTP protocol version | HTTP/1.0, HTTP/1.1, HTTP/2.0 |

### TLS Metrics (HTTPS only)

| Header | Description | Example |
|--------|-------------|---------|
| `X-Profile-TLS-Handshake-Us` | TLS handshake time | 10916 |
| `X-Profile-TLS-Protocol` | TLS version | TLSv1_2, TLSv1_3 |
| `X-Profile-TLS-ALPN` | ALPN negotiated protocol | h2, http/1.1 |

### FFI Metrics (USE_EXT=1 only)

| Header | Description |
|--------|-------------|
| `X-Profile-FFI-Clear-Us` | Clear superglobals time |
| `X-Profile-FFI-Server-Us` | Set $_SERVER time |
| `X-Profile-FFI-Get-Us` | Set $_GET time |
| `X-Profile-FFI-Post-Us` | Set $_POST time |
| `X-Profile-FFI-Cookie-Us` | Set $_COOKIE time |
| `X-Profile-FFI-Files-Us` | Set $_FILES time |
| `X-Profile-FFI-Build-Us` | Build $_REQUEST time |
| `X-Profile-FFI-Init-Eval-Us` | Init eval time |

## Example Output

### HTTP Request

```bash
$ curl -sI -H "X-Profile: 1" http://localhost:8080/index.php | grep X-Profile

X-Profile-Total-Us: 1735
X-Profile-HTTP-Version: HTTP/1.1
X-Profile-Parse-Us: 45
X-Profile-Queue-Us: 169
X-Profile-PHP-Startup-Us: 205
X-Profile-Script-Us: 124
X-Profile-Output-Us: 91
X-Profile-PHP-Shutdown-Us: 56
```

### HTTPS Request

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

### Typical Time Distribution

```
Total Request (~1700µs)
├── Parse        (~45µs)   2.6%   HTTP parsing
├── Queue        (~170µs)  10%    Wait for worker
├── PHP Startup  (~200µs)  12%    php_request_startup()
├── Script       (~120µs)  7%     PHP code execution
├── Output       (~90µs)   5%     Capture stdout
└── Shutdown     (~55µs)   3%     php_request_shutdown()
    + overhead   (~1020µs) 60%    Context switching, etc.
```

### Bottleneck Identification

| High Metric | Likely Cause | Solution |
|-------------|--------------|----------|
| Queue-Us | Workers overloaded | Increase PHP_WORKERS |
| PHP-Startup-Us | Extensions loading | Reduce loaded extensions |
| Script-Us | Slow PHP code | Optimize code, enable OPcache/JIT |
| Output-Us | Large output | Reduce output size |
| TLS-Handshake-Us | TLS overhead | Use keep-alive connections |

### Superglobals Overhead

When profiling reveals superglobals injection is slow:

```
# Eval-based (default)
X-Profile-Superglobals-Us: 236

# FFI-based (USE_EXT=1)
X-Profile-FFI-Server-Us: 45
X-Profile-FFI-Get-Us: 1
```

FFI shows individual superglobal timing, useful for debugging.

## Benchmarking

### Without Profiling

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
    pub parse_us: u64,
    pub queue_us: u64,
    pub php_startup_us: u64,
    pub script_us: u64,
    pub output_us: u64,
    pub php_shutdown_us: u64,
    pub http_version: String,
    // TLS fields
    pub tls_handshake_us: Option<u64>,
    pub tls_protocol: Option<String>,
    pub tls_alpn: Option<String>,
}
```

### Timing Helpers

```rust
pub fn measure<F, R>(f: F) -> (R, u64)
where
    F: FnOnce() -> R,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed().as_micros() as u64;
    (result, elapsed)
}
```

### Header Generation

```rust
impl ProfileData {
    pub fn to_headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![
            ("X-Profile-Total-Us".into(), self.total_us.to_string()),
            ("X-Profile-HTTP-Version".into(), self.http_version.clone()),
            // ...
        ];

        if let Some(tls_us) = self.tls_handshake_us {
            headers.push(("X-Profile-TLS-Handshake-Us".into(), tls_us.to_string()));
        }

        headers
    }
}
```

## Best Practices

1. **Disable in production**: Profiling adds overhead
2. **Sample requests**: Don't profile every request under load
3. **Compare consistently**: Profile same endpoint multiple times
4. **Watch queue time**: High queue time indicates worker bottleneck
5. **Check TLS impact**: First request has handshake, subsequent don't
