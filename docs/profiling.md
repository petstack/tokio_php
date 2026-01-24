# Request Profiling

tokio_php includes a built-in request profiler for detailed performance analysis. Profiling is enabled at **compile time** using the `debug-profile` Cargo feature.

## Enabling Profiler

Build with the `debug-profile` feature:

```bash
# Local build
cargo build --release --features debug-profile

# Docker build
CARGO_FEATURES=debug-profile docker compose build

# Docker run
docker compose up -d
```

## How It Works

When built with `debug-profile`:

1. **Single-worker mode** — Server runs with 1 worker for accurate timing (no thread contention)
2. **All requests profiled** — No header required, every request generates a report
3. **Markdown reports** — Detailed reports written to `/tmp/tokio_profile_request_{request_id}.md`

### Startup Warning

When running a debug-profile build, you'll see:

```
⚠️  DEBUG PROFILE BUILD - Single worker mode, not for production
    Profile reports: /tmp/tokio_profile_request_{request_id}.md
```

## Viewing Reports

```bash
# Make a request
curl http://localhost:8080/index.php

# View all reports
docker compose exec tokio_php cat /tmp/tokio_profile_request_*.md

# View most recent report
docker compose exec tokio_php ls -t /tmp/tokio_profile_request_*.md | head -1 | xargs cat
```

## Report Format

Each request generates a markdown file with detailed timing breakdown:

```markdown
# Profile Report: 65bdbab40000-a1b2

**Total: 2.93 ms**

## Request

- Method: GET
- URL: `/index.php?test=123&foo=bar`

## Connection

- HTTP Version: HTTP/1.1
- TLS: none (plain HTTP)

## Request Pipeline

├── Parse Request: 186 µs (6.4%)
│   ├── Headers: 3 µs
│   ├── Query ($_GET): 3 µs
│   ├── Cookies: 0 µs
│   ├── Body Read: 0 µs
│   ├── Body Parse: 0 µs
│   ├── $_SERVER Vars: 42 µs
│   ├── Path Resolve: 65 µs
│   └── File Check: 66 µs
├── Queue Wait: 77 µs (2.6%)
└── PHP Execution: 2.84 ms (97.1%)
    ├── Startup: 196 µs
    ├── Superglobals: 45 µs
    │   ├── FFI Clear: 41 µs
    │   ├── $_SERVER (0 items): 0 µs
    │   ├── $_GET (2 items): 4 µs
    │   ├── $_POST (0 items): 0 µs
    │   ├── $_COOKIE (0 items): 0 µs
    │   ├── $_FILES (0 items): 0 µs
    │   ├── Build Request: 0 µs
    │   └── Init Eval: 8 µs
    ├── Script Execution: 2.48 ms (84.8%)
    ├── Output Capture: 10 µs
    │   ├── Finalize Eval: 10 µs
    │   ├── Stdout Restore: 0 µs
    │   ├── Output Read: 0 µs
    │   └── Output Parse: 0 µs
    └── Shutdown: 110 µs

## Response Pipeline

├── Build Response: 0 µs (0.0%)

## Summary

| Phase | Time | % |
|-------|------|---|
| Parse Request | 186 µs | 6.4% |
| Queue Wait | 77 µs | 2.6% |
| PHP Startup | 196 µs | 6.7% |
| Superglobals | 45 µs | 1.5% |
| Script Execution | 2.48 ms | 84.8% |
| Output Capture | 10 µs | 0.3% |
| PHP Shutdown | 110 µs | 3.8% |
| Response Build | 0 µs | 0.0% |
| **Total** | **2.93 ms** | **100%** |
```

### TLS Requests

For HTTPS requests, the Connection section includes TLS metrics:

```markdown
## Connection

- HTTP Version: HTTP/2.0
- TLS: TLSv1_3 (ALPN: h2)
- TLS Handshake: 10.92 ms
```

## Profile Data Fields

| Field | Description |
|-------|-------------|
| `total_us` | Total request time (microseconds) |
| `request_method` | HTTP method (GET, POST, etc.) |
| `request_url` | Full request URL (path + query) |
| `http_version` | HTTP protocol version |
| `tls_handshake_us` | TLS handshake time (HTTPS only) |
| `tls_protocol` | TLS version (TLSv1_2, TLSv1_3) |
| `tls_alpn` | ALPN negotiated protocol (h2, http/1.1) |
| `parse_request_us` | Request parsing time |
| `headers_extract_us` | HTTP headers extraction |
| `query_parse_us` | Query string parsing ($_GET) |
| `cookies_parse_us` | Cookie parsing |
| `body_read_us` | Request body read time |
| `body_parse_us` | Body parsing (form/multipart) |
| `server_vars_us` | $_SERVER variables building |
| `path_resolve_us` | URL decode + path resolution |
| `file_check_us` | File existence check |
| `queue_wait_us` | Worker queue wait time |
| `php_startup_us` | php_request_startup() time |
| `superglobals_us` | Total superglobals injection time |
| `ffi_clear_us` | FFI superglobals clear |
| `ffi_server_us` | $_SERVER FFI calls |
| `ffi_get_us` | $_GET FFI calls |
| `ffi_post_us` | $_POST FFI calls |
| `ffi_cookie_us` | $_COOKIE FFI calls |
| `ffi_files_us` | $_FILES FFI calls |
| `ffi_build_request_us` | tokio_sapi_build_request() |
| `ffi_init_eval_us` | Init eval (header_remove, ob_start) |
| `script_exec_us` | PHP script execution time |
| `output_capture_us` | Total output capture time |
| `output_finalize_us` | Finalize eval (flush + headers) |
| `output_stdout_restore_us` | Stdout restore time |
| `output_read_us` | Output read time |
| `output_parse_us` | Output parsing time |
| `php_shutdown_us` | php_request_shutdown() time |
| `response_build_us` | HTTP response building time |
| `compression_us` | Brotli compression time |

## Understanding the Metrics

### Bottleneck Identification

| High Metric | Likely Cause | Solution |
|-------------|--------------|----------|
| Queue Wait | Workers overloaded | N/A (single-worker in debug mode) |
| PHP Startup | Extensions loading | Reduce loaded extensions |
| Superglobals | Many variables | Reduce $_SERVER entries |
| Script Execution | Slow PHP code | Optimize code, enable OPcache/JIT |
| Output Capture | Large output | Reduce output size |
| TLS Handshake | TLS overhead | Use keep-alive connections |

### Typical Distribution

For a well-optimized application:

| Phase | Expected % |
|-------|------------|
| Parse Request | 2-5% |
| Queue Wait | < 5% |
| PHP Startup | 5-15% |
| Superglobals | 1-5% |
| Script Execution | 60-85% |
| Output Capture | < 1% |
| PHP Shutdown | 3-10% |
| Response Build | < 1% |

If Script Execution is < 50%, there's optimization potential in the server overhead.

## Production vs Debug Builds

| Aspect | Production | Debug Profile |
|--------|------------|---------------|
| Workers | Auto (CPU cores) | **1 (forced)** |
| Profiling | Disabled | **Always on** |
| Output | None | `/tmp/tokio_profile_request_*.md` |
| Overhead | None | ~5-10% per request |
| Use case | Production traffic | Performance analysis |

**Important:** Never use debug-profile builds in production:
- Single-threaded = limited throughput
- File writes for every request = disk I/O overhead
- Timing overhead affects measurements slightly

## Benchmarking

### Without Profiling (Recommended)

```bash
# Production build for accurate benchmarks
docker compose build
docker compose up -d

wrk -t4 -c100 -d10s http://localhost:8080/index.php
```

### With Profiling (Analysis)

```bash
# Debug build for timing analysis
CARGO_FEATURES=debug-profile docker compose build
docker compose up -d

# Run a few requests
for i in {1..10}; do
  curl -s http://localhost:8080/index.php > /dev/null
done

# Analyze reports
docker compose exec tokio_php sh -c 'cat /tmp/tokio_profile_request_*.md'
```

## Implementation

### ProfileData Structure

```rust
// src/profiler.rs
pub struct ProfileData {
    // Request info
    pub request_method: String,
    pub request_url: String,

    // Total time
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

impl ProfileData {
    /// Generate markdown report with tree structure
    #[cfg(feature = "debug-profile")]
    pub fn to_markdown_report(&self, request_id: &str) -> String;

    /// Write report to /tmp/tokio_profile_request_{id}.md
    #[cfg(feature = "debug-profile")]
    pub fn write_report(&self, request_id: &str);
}
```

### Conditional Compilation

Profiling code is completely removed from production builds:

```rust
// Only compiled with debug-profile feature
#[cfg(feature = "debug-profile")]
if let Some(ref profile) = resp.profile {
    profile.write_report(request_id);
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

1. **Use for analysis only** — Profile when investigating performance issues
2. **Run multiple requests** — Single requests have variance; look at patterns
3. **Compare before/after** — Profile before and after optimizations
4. **Check script vs overhead** — If PHP execution is < 60%, investigate server overhead
5. **Profile with realistic data** — Use production-like request payloads
6. **Clean up reports** — Delete old reports: `rm /tmp/tokio_profile_request_*.md`

## See Also

- [Configuration](configuration.md) - Environment variables reference
- [tokio_sapi Extension](tokio-sapi-extension.md) - FFI superglobals implementation
- [Internal Server](internal-server.md) - Prometheus metrics for runtime monitoring
- [Worker Pool](worker-pool.md) - Worker configuration
- [Architecture](architecture.md) - System overview
