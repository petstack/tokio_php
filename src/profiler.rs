use std::time::Instant;

#[cfg(feature = "debug-profile")]
use std::io::Write;

// Note: Profiling is now controlled by the `debug-profile` compile-time feature.
// When enabled, single-worker mode is enforced and detailed reports are written
// to /tmp/tokio_profile_request_{request_id}.md

/// A skipped action with the reason why it was skipped.
#[derive(Debug, Clone)]
pub struct SkippedAction {
    /// Name of the action that was skipped
    pub action: String,
    /// Reason why the action was skipped
    pub reason: String,
}

impl SkippedAction {
    pub fn new(action: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            reason: reason.into(),
        }
    }
}

/// Route type for the request
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RouteType {
    /// PHP script execution
    #[default]
    Php,
    /// Static file serving
    Static,
    /// Request was routed to index.php via single entry point mode
    IndexRedirect,
    /// Direct access to index file was blocked (404)
    BlockedDirectIndex,
    /// File not found (404)
    NotFound,
    /// SSE streaming request
    Sse,
    /// Rate limited (429)
    RateLimited,
    /// Method not allowed (405)
    MethodNotAllowed,
}

impl RouteType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteType::Php => "php",
            RouteType::Static => "static",
            RouteType::IndexRedirect => "index_redirect",
            RouteType::BlockedDirectIndex => "blocked_direct_index",
            RouteType::NotFound => "not_found",
            RouteType::Sse => "sse",
            RouteType::RateLimited => "rate_limited",
            RouteType::MethodNotAllowed => "method_not_allowed",
        }
    }
}

/// Profile data for a single request
#[derive(Debug, Clone, Default)]
pub struct ProfileData {
    // Total time
    pub total_us: u64,

    // === Request info ===
    pub request_method: String, // HTTP method (GET, POST, etc.)
    pub request_url: String,    // Full request URL (path + query)

    // === Routing decision ===
    pub route_type: RouteType, // Type of request (php, static, index_redirect, etc.)
    pub resolved_path: String, // Final resolved file path
    pub index_file_mode: bool, // Whether single entry point mode is active
    pub file_cache_hit: bool,  // Whether file existence check was a cache hit

    // === Connection & TLS (server.rs) ===
    pub tls_handshake_us: u64, // TLS handshake time (0 for plain HTTP)
    pub http_version: String,  // HTTP/1.0, HTTP/1.1, HTTP/2.0
    pub tls_protocol: String,  // TLS 1.2, TLS 1.3, or empty for plain HTTP
    pub tls_alpn: String,      // ALPN negotiated protocol (h2, http/1.1)

    // === Middleware (Rust) ===
    pub rate_limit_us: u64,     // Rate limiting check
    pub middleware_req_us: u64, // Total request middleware time

    // === Server-side parsing (server.rs) ===
    pub parse_request_us: u64,   // Total parse time
    pub headers_extract_us: u64, // Extract HTTP headers from request
    pub query_parse_us: u64,     // Parse query string ($_GET)
    pub cookies_parse_us: u64,   // Parse cookies
    pub body_read_us: u64,       // Read POST body
    pub body_parse_us: u64,      // Parse POST body (form/multipart)
    pub server_vars_us: u64,     // Build $_SERVER vars
    pub path_resolve_us: u64,    // URL decode + path resolution
    pub file_check_us: u64,      // Path::exists() check
    pub trace_context_us: u64,   // W3C trace context parsing

    // === Executor queue ===
    pub queue_wait_us: u64,   // Time waiting in worker queue
    pub channel_send_us: u64, // Time to send request to worker channel

    // === PHP execution (php.rs) ===
    pub php_startup_us: u64, // php_request_startup()

    // Superglobals breakdown
    pub superglobals_us: u64,       // Total superglobals time
    pub superglobals_build_us: u64, // Build PHP code string (eval mode)
    pub superglobals_eval_us: u64,  // zend_eval_string execution (eval mode)

    // FFI superglobals breakdown (EXECUTOR=ext)
    pub ffi_request_init_us: u64,  // tokio_sapi_request_init()
    pub ffi_clear_us: u64,         // tokio_sapi_clear_superglobals()
    pub ffi_server_us: u64,        // All $_SERVER FFI calls
    pub ffi_server_count: u64,     // Number of $_SERVER entries
    pub ffi_get_us: u64,           // All $_GET FFI calls
    pub ffi_get_count: u64,        // Number of $_GET entries
    pub ffi_post_us: u64,          // All $_POST FFI calls
    pub ffi_post_count: u64,       // Number of $_POST entries
    pub ffi_cookie_us: u64,        // All $_COOKIE FFI calls
    pub ffi_cookie_count: u64,     // Number of $_COOKIE entries
    pub ffi_files_us: u64,         // All $_FILES FFI calls
    pub ffi_files_count: u64,      // Number of $_FILES entries
    pub ffi_build_request_us: u64, // tokio_sapi_build_request()
    pub ffi_init_eval_us: u64,     // INIT_CODE eval (header_remove, ob_start)

    // I/O setup
    pub memfd_setup_us: u64, // memfd_create + stdout redirect

    // Script execution
    pub script_exec_us: u64, // php_execute_script

    // Output capture breakdown
    pub output_capture_us: u64, // Total output capture time
    pub finalize_eval_us: u64,  // FINALIZE_CODE eval (flush + headers)
    pub stdout_restore_us: u64, // Restore stdout
    pub output_read_us: u64,    // Read from memfd
    pub output_parse_us: u64,   // Parse body + headers from output

    pub php_shutdown_us: u64, // php_request_shutdown()

    // === Response building (server.rs) ===
    pub response_build_us: u64,  // Build HTTP response from ScriptResponse
    pub compression_us: u64,     // Brotli compression time
    pub compression_ratio: f32,  // Compression ratio (compressed/original)
    pub middleware_resp_us: u64, // Total response middleware time
    pub headers_build_us: u64,   // Building response headers
    pub body_collect_us: u64,    // Collecting body from stream

    // === Streaming early response ===
    pub early_finish: bool, // True if response was sent via tokio_finish_request()

    // === Static file serving ===
    pub static_file_us: u64,   // Static file read time (non-PHP)
    pub static_file_size: u64, // Static file size in bytes

    // === Middleware timing (individual) ===
    pub mw_static_cache_us: u64, // Static cache middleware (response)
    pub mw_error_pages_us: u64,  // Error pages middleware (response)
    pub mw_access_log_us: u64,   // Access log middleware (response)

    // === SAPI Callback timing ===
    // Output (ub_write callback)
    pub sapi_ub_write_us: u64,    // Total time in ub_write callback
    pub sapi_ub_write_count: u64, // Number of ub_write calls
    pub sapi_ub_write_bytes: u64, // Total bytes written via ub_write

    // Header handling
    pub sapi_header_handler_us: u64, // Total time in header_handler callback
    pub sapi_header_handler_count: u64, // Number of header() calls from PHP
    pub sapi_send_headers_us: u64,   // Time in send_headers callback

    // Flush
    pub sapi_flush_us: u64,    // Total time in flush callback
    pub sapi_flush_count: u64, // Number of flush() calls

    // POST data reading
    pub sapi_read_post_us: u64,    // Total time reading POST data
    pub sapi_read_post_bytes: u64, // Bytes read via read_post callback

    // Lifecycle callbacks
    pub sapi_activate_us: u64,   // Time in sapi_activate (request start)
    pub sapi_deactivate_us: u64, // Time in sapi_deactivate (request end)

    // === Streaming ===
    pub stream_chunk_count: u64, // Number of chunks sent
    pub stream_chunk_bytes: u64, // Total bytes sent via streaming

    // === Routing ===
    pub routing_decision_us: u64,   // Routing logic decision time
    pub file_cache_lookup_us: u64,  // File cache lookup time
    pub file_cache_hit_count: u64,  // Number of cache hits during request
    pub file_cache_miss_count: u64, // Number of cache misses during request

    // === Context management ===
    pub context_init_us: u64,    // Request context initialization
    pub context_cleanup_us: u64, // Request context cleanup

    // === Skipped actions with reasons ===
    pub skipped_actions: Vec<SkippedAction>,
}

impl ProfileData {
    /// Add a skipped action with its reason.
    pub fn skip(&mut self, action: impl Into<String>, reason: impl Into<String>) {
        self.skipped_actions
            .push(SkippedAction::new(action, reason));
    }

    /// Convert to HTTP header format
    pub fn to_headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![
            // Summary
            ("X-Profile-Total-Us".to_string(), self.total_us.to_string()),
            // Routing
            (
                "X-Profile-Route-Type".to_string(),
                self.route_type.as_str().to_string(),
            ),
            (
                "X-Profile-File-Cache-Hit".to_string(),
                if self.file_cache_hit { "1" } else { "0" }.to_string(),
            ),
            // Connection & TLS
            (
                "X-Profile-HTTP-Version".to_string(),
                self.http_version.clone(),
            ),
        ];

        // Only include TLS headers if TLS was used
        if self.tls_handshake_us > 0 {
            headers.push((
                "X-Profile-TLS-Handshake-Us".to_string(),
                self.tls_handshake_us.to_string(),
            ));
        }
        if !self.tls_protocol.is_empty() {
            headers.push((
                "X-Profile-TLS-Protocol".to_string(),
                self.tls_protocol.clone(),
            ));
        }
        if !self.tls_alpn.is_empty() {
            headers.push(("X-Profile-TLS-ALPN".to_string(), self.tls_alpn.clone()));
        }

        // Middleware (Rust-side)
        if self.rate_limit_us > 0 {
            headers.push((
                "X-Profile-RateLimit-Us".to_string(),
                self.rate_limit_us.to_string(),
            ));
        }
        if self.middleware_req_us > 0 {
            headers.push((
                "X-Profile-Middleware-Req-Us".to_string(),
                self.middleware_req_us.to_string(),
            ));
        }

        // Parse breakdown
        headers.extend([
            (
                "X-Profile-Parse-Us".to_string(),
                self.parse_request_us.to_string(),
            ),
            (
                "X-Profile-Parse-Headers-Us".to_string(),
                self.headers_extract_us.to_string(),
            ),
            (
                "X-Profile-Parse-Query-Us".to_string(),
                self.query_parse_us.to_string(),
            ),
            (
                "X-Profile-Parse-Cookies-Us".to_string(),
                self.cookies_parse_us.to_string(),
            ),
            (
                "X-Profile-Parse-Body-Read-Us".to_string(),
                self.body_read_us.to_string(),
            ),
            (
                "X-Profile-Parse-Body-Parse-Us".to_string(),
                self.body_parse_us.to_string(),
            ),
            (
                "X-Profile-Parse-ServerVars-Us".to_string(),
                self.server_vars_us.to_string(),
            ),
            (
                "X-Profile-Parse-Path-Us".to_string(),
                self.path_resolve_us.to_string(),
            ),
            (
                "X-Profile-Parse-FileCheck-Us".to_string(),
                self.file_check_us.to_string(),
            ),
        ]);

        if self.trace_context_us > 0 {
            headers.push((
                "X-Profile-Parse-TraceCtx-Us".to_string(),
                self.trace_context_us.to_string(),
            ));
        }

        // Queue
        headers.extend([(
            "X-Profile-Queue-Us".to_string(),
            self.queue_wait_us.to_string(),
        )]);

        if self.channel_send_us > 0 {
            headers.push((
                "X-Profile-Channel-Send-Us".to_string(),
                self.channel_send_us.to_string(),
            ));
        }

        headers.extend([
            // PHP startup
            (
                "X-Profile-PHP-Startup-Us".to_string(),
                self.php_startup_us.to_string(),
            ),
            // Superglobals breakdown
            (
                "X-Profile-Superglobals-Us".to_string(),
                self.superglobals_us.to_string(),
            ),
            (
                "X-Profile-Superglobals-Build-Us".to_string(),
                self.superglobals_build_us.to_string(),
            ),
            (
                "X-Profile-Superglobals-Eval-Us".to_string(),
                self.superglobals_eval_us.to_string(),
            ),
        ]);

        // FFI-specific headers (only when EXECUTOR=ext and values are non-zero)
        if self.ffi_request_init_us > 0 || self.ffi_clear_us > 0 {
            headers.extend([
                (
                    "X-Profile-FFI-Request-Init-Us".to_string(),
                    self.ffi_request_init_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Clear-Us".to_string(),
                    self.ffi_clear_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Server-Us".to_string(),
                    self.ffi_server_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Server-Count".to_string(),
                    self.ffi_server_count.to_string(),
                ),
                (
                    "X-Profile-FFI-Get-Us".to_string(),
                    self.ffi_get_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Get-Count".to_string(),
                    self.ffi_get_count.to_string(),
                ),
                (
                    "X-Profile-FFI-Post-Us".to_string(),
                    self.ffi_post_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Post-Count".to_string(),
                    self.ffi_post_count.to_string(),
                ),
                (
                    "X-Profile-FFI-Cookie-Us".to_string(),
                    self.ffi_cookie_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Cookie-Count".to_string(),
                    self.ffi_cookie_count.to_string(),
                ),
                (
                    "X-Profile-FFI-Files-Us".to_string(),
                    self.ffi_files_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Files-Count".to_string(),
                    self.ffi_files_count.to_string(),
                ),
                (
                    "X-Profile-FFI-Build-Request-Us".to_string(),
                    self.ffi_build_request_us.to_string(),
                ),
                (
                    "X-Profile-FFI-Init-Eval-Us".to_string(),
                    self.ffi_init_eval_us.to_string(),
                ),
            ]);
        }

        headers.extend([
            // I/O setup
            (
                "X-Profile-Memfd-Setup-Us".to_string(),
                self.memfd_setup_us.to_string(),
            ),
            // Script
            (
                "X-Profile-Script-Us".to_string(),
                self.script_exec_us.to_string(),
            ),
            // Output breakdown
            (
                "X-Profile-Output-Us".to_string(),
                self.output_capture_us.to_string(),
            ),
            (
                "X-Profile-Output-Finalize-Us".to_string(),
                self.finalize_eval_us.to_string(),
            ),
            (
                "X-Profile-Output-Restore-Us".to_string(),
                self.stdout_restore_us.to_string(),
            ),
            (
                "X-Profile-Output-Read-Us".to_string(),
                self.output_read_us.to_string(),
            ),
            (
                "X-Profile-Output-Parse-Us".to_string(),
                self.output_parse_us.to_string(),
            ),
            // Shutdown
            (
                "X-Profile-PHP-Shutdown-Us".to_string(),
                self.php_shutdown_us.to_string(),
            ),
            // Response
            (
                "X-Profile-Response-Us".to_string(),
                self.response_build_us.to_string(),
            ),
        ]);

        // Response-side details
        if self.compression_us > 0 {
            headers.push((
                "X-Profile-Compression-Us".to_string(),
                self.compression_us.to_string(),
            ));
            headers.push((
                "X-Profile-Compression-Ratio".to_string(),
                format!("{:.2}", self.compression_ratio),
            ));
        }
        if self.middleware_resp_us > 0 {
            headers.push((
                "X-Profile-Middleware-Resp-Us".to_string(),
                self.middleware_resp_us.to_string(),
            ));
        }
        if self.headers_build_us > 0 {
            headers.push((
                "X-Profile-Headers-Build-Us".to_string(),
                self.headers_build_us.to_string(),
            ));
        }
        if self.body_collect_us > 0 {
            headers.push((
                "X-Profile-Body-Collect-Us".to_string(),
                self.body_collect_us.to_string(),
            ));
        }

        // Static file serving
        if self.static_file_us > 0 {
            headers.push((
                "X-Profile-Static-File-Us".to_string(),
                self.static_file_us.to_string(),
            ));
            headers.push((
                "X-Profile-Static-File-Size".to_string(),
                self.static_file_size.to_string(),
            ));
        }

        // SAPI callback timing
        if self.sapi_ub_write_count > 0 {
            headers.push((
                "X-Profile-SAPI-UbWrite-Us".to_string(),
                self.sapi_ub_write_us.to_string(),
            ));
            headers.push((
                "X-Profile-SAPI-UbWrite-Count".to_string(),
                self.sapi_ub_write_count.to_string(),
            ));
            headers.push((
                "X-Profile-SAPI-UbWrite-Bytes".to_string(),
                self.sapi_ub_write_bytes.to_string(),
            ));
        }
        if self.sapi_header_handler_count > 0 {
            headers.push((
                "X-Profile-SAPI-Header-Us".to_string(),
                self.sapi_header_handler_us.to_string(),
            ));
            headers.push((
                "X-Profile-SAPI-Header-Count".to_string(),
                self.sapi_header_handler_count.to_string(),
            ));
        }
        if self.sapi_send_headers_us > 0 {
            headers.push((
                "X-Profile-SAPI-SendHeaders-Us".to_string(),
                self.sapi_send_headers_us.to_string(),
            ));
        }
        if self.sapi_flush_count > 0 {
            headers.push((
                "X-Profile-SAPI-Flush-Us".to_string(),
                self.sapi_flush_us.to_string(),
            ));
            headers.push((
                "X-Profile-SAPI-Flush-Count".to_string(),
                self.sapi_flush_count.to_string(),
            ));
        }
        if self.sapi_read_post_bytes > 0 {
            headers.push((
                "X-Profile-SAPI-ReadPost-Us".to_string(),
                self.sapi_read_post_us.to_string(),
            ));
            headers.push((
                "X-Profile-SAPI-ReadPost-Bytes".to_string(),
                self.sapi_read_post_bytes.to_string(),
            ));
        }
        if self.sapi_activate_us > 0 || self.sapi_deactivate_us > 0 {
            headers.push((
                "X-Profile-SAPI-Activate-Us".to_string(),
                self.sapi_activate_us.to_string(),
            ));
            headers.push((
                "X-Profile-SAPI-Deactivate-Us".to_string(),
                self.sapi_deactivate_us.to_string(),
            ));
        }

        // Streaming stats
        if self.stream_chunk_count > 0 {
            headers.push((
                "X-Profile-Stream-Chunks".to_string(),
                self.stream_chunk_count.to_string(),
            ));
            headers.push((
                "X-Profile-Stream-Bytes".to_string(),
                self.stream_chunk_bytes.to_string(),
            ));
        }

        // Routing stats
        if self.routing_decision_us > 0 {
            headers.push((
                "X-Profile-Routing-Us".to_string(),
                self.routing_decision_us.to_string(),
            ));
        }
        if self.file_cache_hit_count > 0 || self.file_cache_miss_count > 0 {
            headers.push((
                "X-Profile-FileCache-Hits".to_string(),
                self.file_cache_hit_count.to_string(),
            ));
            headers.push((
                "X-Profile-FileCache-Misses".to_string(),
                self.file_cache_miss_count.to_string(),
            ));
        }

        // Context management
        if self.context_init_us > 0 || self.context_cleanup_us > 0 {
            headers.push((
                "X-Profile-Context-Init-Us".to_string(),
                self.context_init_us.to_string(),
            ));
            headers.push((
                "X-Profile-Context-Cleanup-Us".to_string(),
                self.context_cleanup_us.to_string(),
            ));
        }

        headers
    }

    /// Format as human-readable string (summary only)
    pub fn to_summary(&self) -> String {
        let tls_info = if self.tls_handshake_us > 0 {
            format!(" tls={}us", self.tls_handshake_us)
        } else {
            String::new()
        };

        format!(
            "total={}us{} http={} parse={}us queue={}us php_start={}us globals={}us script={}us output={}us php_end={}us resp={}us",
            self.total_us,
            tls_info,
            self.http_version,
            self.parse_request_us,
            self.queue_wait_us,
            self.php_startup_us,
            self.superglobals_us,
            self.script_exec_us,
            self.output_capture_us,
            self.php_shutdown_us,
            self.response_build_us
        )
    }

    /// Generate detailed markdown report with tree structure.
    ///
    /// Only available with `debug-profile` feature.
    #[cfg(feature = "debug-profile")]
    pub fn to_markdown_report(&self, request_id: &str) -> String {
        let mut report = String::with_capacity(4096);

        // Helper for percentage calculation
        let pct = |us: u64| -> f64 {
            if self.total_us > 0 {
                (us as f64 / self.total_us as f64) * 100.0
            } else {
                0.0
            }
        };

        // Helper for formatting time
        let fmt_time = |us: u64| -> String {
            if us >= 1_000_000 {
                format!("{:.2} s", us as f64 / 1_000_000.0)
            } else if us >= 1_000 {
                format!("{:.2} ms", us as f64 / 1_000.0)
            } else {
                format!("{} µs", us)
            }
        };

        // Header
        report.push_str(&format!("# Profile Report: {}\n\n", request_id));
        report.push_str(&format!("**Total: {}**\n\n", fmt_time(self.total_us)));

        // Request info
        report.push_str("## Request\n\n");
        report.push_str(&format!("- Method: {}\n", self.request_method));
        report.push_str(&format!("- URL: `{}`\n", self.request_url));
        report.push('\n');

        // Routing decision
        report.push_str("## Routing\n\n");
        report.push_str(&format!("- Route Type: `{}`\n", self.route_type.as_str()));
        if !self.resolved_path.is_empty() {
            report.push_str(&format!("- Resolved Path: `{}`\n", self.resolved_path));
        }
        report.push_str(&format!(
            "- Index File Mode: {}\n",
            if self.index_file_mode {
                "enabled"
            } else {
                "disabled"
            }
        ));
        report.push_str(&format!(
            "- File Cache Hit: {}\n",
            if self.file_cache_hit {
                "yes"
            } else {
                "no (filesystem check)"
            }
        ));
        report.push('\n');

        // Connection info
        report.push_str("## Connection\n\n");
        report.push_str(&format!("- HTTP Version: {}\n", self.http_version));
        if self.tls_handshake_us > 0 {
            report.push_str(&format!(
                "- TLS Handshake: {}\n",
                fmt_time(self.tls_handshake_us)
            ));
            report.push_str(&format!("- TLS Protocol: {}\n", self.tls_protocol));
            if !self.tls_alpn.is_empty() {
                report.push_str(&format!("- TLS ALPN: {}\n", self.tls_alpn));
            }
        } else {
            report.push_str("- TLS: none (plain HTTP)\n");
        }
        report.push('\n');

        // Request pipeline
        report.push_str("## Request Pipeline\n\n");
        report.push_str("```\n");

        // Middleware
        if self.rate_limit_us > 0 || self.middleware_req_us > 0 {
            let middleware_total = self.rate_limit_us + self.middleware_req_us;
            report.push_str(&format!(
                "├── Middleware: {} ({:.1}%)\n",
                fmt_time(middleware_total),
                pct(middleware_total)
            ));
            if self.rate_limit_us > 0 {
                report.push_str(&format!(
                    "│   └── Rate Limit: {}\n",
                    fmt_time(self.rate_limit_us)
                ));
            }
        }

        // Parse request
        report.push_str(&format!(
            "├── Parse Request: {} ({:.1}%)\n",
            fmt_time(self.parse_request_us),
            pct(self.parse_request_us)
        ));
        report.push_str(&format!(
            "│   ├── Headers: {}\n",
            fmt_time(self.headers_extract_us)
        ));
        report.push_str(&format!(
            "│   ├── Query ($_GET): {}\n",
            fmt_time(self.query_parse_us)
        ));
        report.push_str(&format!(
            "│   ├── Cookies: {}\n",
            fmt_time(self.cookies_parse_us)
        ));
        report.push_str(&format!(
            "│   ├── Body Read: {}\n",
            fmt_time(self.body_read_us)
        ));
        report.push_str(&format!(
            "│   ├── Body Parse: {}\n",
            fmt_time(self.body_parse_us)
        ));
        report.push_str(&format!(
            "│   ├── $_SERVER Vars: {}\n",
            fmt_time(self.server_vars_us)
        ));
        report.push_str(&format!(
            "│   ├── Path Resolve: {}\n",
            fmt_time(self.path_resolve_us)
        ));
        report.push_str(&format!(
            "│   └── File Check: {}\n",
            fmt_time(self.file_check_us)
        ));
        if self.trace_context_us > 0 {
            report.push_str(&format!(
                "│       └── Trace Context: {}\n",
                fmt_time(self.trace_context_us)
            ));
        }

        // Queue
        report.push_str(&format!(
            "├── Queue Wait: {} ({:.1}%)\n",
            fmt_time(self.queue_wait_us),
            pct(self.queue_wait_us)
        ));
        if self.channel_send_us > 0 {
            report.push_str(&format!(
                "│   └── Channel Send: {}\n",
                fmt_time(self.channel_send_us)
            ));
        }

        // PHP execution
        let php_total = self.php_startup_us
            + self.superglobals_us
            + self.memfd_setup_us
            + self.script_exec_us
            + self.output_capture_us
            + self.php_shutdown_us;
        report.push_str(&format!(
            "└── PHP Execution: {} ({:.1}%)\n",
            fmt_time(php_total),
            pct(php_total)
        ));
        report.push_str(&format!(
            "    ├── Startup: {}\n",
            fmt_time(self.php_startup_us)
        ));

        // Superglobals
        report.push_str(&format!(
            "    ├── Superglobals: {}\n",
            fmt_time(self.superglobals_us)
        ));

        // FFI breakdown (if used)
        if self.ffi_clear_us > 0 || self.ffi_server_us > 0 {
            report.push_str(&format!(
                "    │   ├── FFI Clear: {}\n",
                fmt_time(self.ffi_clear_us)
            ));
            report.push_str(&format!(
                "    │   ├── $_SERVER ({} items): {}\n",
                self.ffi_server_count,
                fmt_time(self.ffi_server_us)
            ));
            report.push_str(&format!(
                "    │   ├── $_GET ({} items): {}\n",
                self.ffi_get_count,
                fmt_time(self.ffi_get_us)
            ));
            report.push_str(&format!(
                "    │   ├── $_POST ({} items): {}\n",
                self.ffi_post_count,
                fmt_time(self.ffi_post_us)
            ));
            report.push_str(&format!(
                "    │   ├── $_COOKIE ({} items): {}\n",
                self.ffi_cookie_count,
                fmt_time(self.ffi_cookie_us)
            ));
            report.push_str(&format!(
                "    │   ├── $_FILES ({} items): {}\n",
                self.ffi_files_count,
                fmt_time(self.ffi_files_us)
            ));
            report.push_str(&format!(
                "    │   ├── Build Request: {}\n",
                fmt_time(self.ffi_build_request_us)
            ));
            report.push_str(&format!(
                "    │   └── Init Eval: {}\n",
                fmt_time(self.ffi_init_eval_us)
            ));
        } else if self.superglobals_build_us > 0 || self.superglobals_eval_us > 0 {
            // Eval mode
            report.push_str(&format!(
                "    │   ├── Build Code: {}\n",
                fmt_time(self.superglobals_build_us)
            ));
            report.push_str(&format!(
                "    │   └── Eval: {}\n",
                fmt_time(self.superglobals_eval_us)
            ));
        }

        // Memfd setup
        if self.memfd_setup_us > 0 {
            report.push_str(&format!(
                "    ├── Memfd Setup: {}\n",
                fmt_time(self.memfd_setup_us)
            ));
        }

        // Script execution
        report.push_str(&format!(
            "    ├── Script Execution: {} ({:.1}%)\n",
            fmt_time(self.script_exec_us),
            pct(self.script_exec_us)
        ));

        // Output capture
        report.push_str(&format!(
            "    ├── Output Capture: {}\n",
            fmt_time(self.output_capture_us)
        ));
        report.push_str(&format!(
            "    │   ├── Finalize Eval: {}\n",
            fmt_time(self.finalize_eval_us)
        ));
        report.push_str(&format!(
            "    │   ├── Stdout Restore: {}\n",
            fmt_time(self.stdout_restore_us)
        ));
        report.push_str(&format!(
            "    │   ├── Output Read: {}\n",
            fmt_time(self.output_read_us)
        ));
        report.push_str(&format!(
            "    │   └── Output Parse: {}\n",
            fmt_time(self.output_parse_us)
        ));

        // Shutdown
        report.push_str(&format!(
            "    └── Shutdown: {}\n",
            fmt_time(self.php_shutdown_us)
        ));

        report.push_str("```\n\n");

        // Response pipeline
        report.push_str("## Response Pipeline\n\n");
        report.push_str("```\n");
        report.push_str(&format!(
            "├── Build Response: {} ({:.1}%)\n",
            fmt_time(self.response_build_us),
            pct(self.response_build_us)
        ));

        if self.compression_us > 0 {
            report.push_str(&format!(
                "├── Compression (Brotli): {} (ratio: {:.0}%)\n",
                fmt_time(self.compression_us),
                self.compression_ratio * 100.0
            ));
        }

        // Individual middleware timing
        let has_middleware = self.middleware_resp_us > 0
            || self.mw_static_cache_us > 0
            || self.mw_error_pages_us > 0
            || self.mw_access_log_us > 0;

        if has_middleware {
            report.push_str(&format!(
                "└── Middleware Response: {}\n",
                fmt_time(self.middleware_resp_us)
            ));
            if self.mw_static_cache_us > 0 {
                report.push_str(&format!(
                    "    ├── Static Cache: {}\n",
                    fmt_time(self.mw_static_cache_us)
                ));
            }
            if self.mw_error_pages_us > 0 {
                report.push_str(&format!(
                    "    ├── Error Pages: {}\n",
                    fmt_time(self.mw_error_pages_us)
                ));
            }
            if self.mw_access_log_us > 0 {
                report.push_str(&format!(
                    "    └── Access Log: {}\n",
                    fmt_time(self.mw_access_log_us)
                ));
            }
        }
        report.push_str("```\n\n");

        // SAPI Callbacks section (if any callbacks were profiled)
        let has_sapi_timing = self.sapi_ub_write_count > 0
            || self.sapi_header_handler_count > 0
            || self.sapi_flush_count > 0
            || self.sapi_read_post_bytes > 0
            || self.sapi_activate_us > 0;

        if has_sapi_timing {
            report.push_str("## SAPI Callbacks\n\n");
            report.push_str("```\n");

            // Lifecycle
            if self.sapi_activate_us > 0 || self.sapi_deactivate_us > 0 {
                report.push_str(&format!(
                    "├── Lifecycle: {}\n",
                    fmt_time(self.sapi_activate_us + self.sapi_deactivate_us)
                ));
                report.push_str(&format!(
                    "│   ├── Activate: {}\n",
                    fmt_time(self.sapi_activate_us)
                ));
                report.push_str(&format!(
                    "│   └── Deactivate: {}\n",
                    fmt_time(self.sapi_deactivate_us)
                ));
            }

            // Output (ub_write)
            if self.sapi_ub_write_count > 0 {
                report.push_str(&format!(
                    "├── Output (ub_write): {} ({} calls, {} bytes)\n",
                    fmt_time(self.sapi_ub_write_us),
                    self.sapi_ub_write_count,
                    self.sapi_ub_write_bytes
                ));
            }

            // Header handling
            if self.sapi_header_handler_count > 0 {
                report.push_str(&format!(
                    "├── Headers: {} ({} calls)\n",
                    fmt_time(self.sapi_header_handler_us),
                    self.sapi_header_handler_count
                ));
                if self.sapi_send_headers_us > 0 {
                    report.push_str(&format!(
                        "│   └── Send Headers: {}\n",
                        fmt_time(self.sapi_send_headers_us)
                    ));
                }
            }

            // Flush
            if self.sapi_flush_count > 0 {
                report.push_str(&format!(
                    "├── Flush: {} ({} calls)\n",
                    fmt_time(self.sapi_flush_us),
                    self.sapi_flush_count
                ));
            }

            // POST data reading
            if self.sapi_read_post_bytes > 0 {
                report.push_str(&format!(
                    "└── Read POST: {} ({} bytes)\n",
                    fmt_time(self.sapi_read_post_us),
                    self.sapi_read_post_bytes
                ));
            }

            report.push_str("```\n\n");
        }

        // Streaming section (if used)
        if self.stream_chunk_count > 0 {
            report.push_str("## Streaming\n\n");
            report.push_str(&format!("- Chunks sent: {}\n", self.stream_chunk_count));
            report.push_str(&format!("- Total bytes: {}\n", self.stream_chunk_bytes));
            report.push('\n');
        }

        // Context management (if timing available)
        if self.context_init_us > 0 || self.context_cleanup_us > 0 {
            report.push_str("## Context Management\n\n");
            report.push_str(&format!("- Init: {}\n", fmt_time(self.context_init_us)));
            report.push_str(&format!(
                "- Cleanup: {}\n",
                fmt_time(self.context_cleanup_us)
            ));
            report.push('\n');
        }

        // Static file (if applicable)
        if self.static_file_us > 0 {
            report.push_str("## Static File\n\n");
            report.push_str(&format!("- Read Time: {}\n", fmt_time(self.static_file_us)));
            report.push_str(&format!("- File Size: {} bytes\n", self.static_file_size));
            report.push('\n');
        }

        // Early finish flag
        if self.early_finish {
            report.push_str("## Notes\n\n");
            report.push_str("- Response was sent early via `tokio_finish_request()`\n");
            report.push('\n');
        }

        // Skipped Actions section
        if !self.skipped_actions.is_empty() {
            report.push_str("## Skipped Actions\n\n");
            report.push_str("The following actions were not executed and why:\n\n");
            report.push_str("| Action | Reason |\n");
            report.push_str("|--------|--------|\n");
            for action in &self.skipped_actions {
                report.push_str(&format!("| {} | {} |\n", action.action, action.reason));
            }
            report.push('\n');
        }

        // Summary table
        report.push_str("## Summary\n\n");
        report.push_str("| Phase | Time | % |\n");
        report.push_str("|-------|------|---|\n");
        report.push_str(&format!(
            "| Parse Request | {} | {:.1}% |\n",
            fmt_time(self.parse_request_us),
            pct(self.parse_request_us)
        ));
        report.push_str(&format!(
            "| Queue Wait | {} | {:.1}% |\n",
            fmt_time(self.queue_wait_us),
            pct(self.queue_wait_us)
        ));
        report.push_str(&format!(
            "| PHP Startup | {} | {:.1}% |\n",
            fmt_time(self.php_startup_us),
            pct(self.php_startup_us)
        ));
        report.push_str(&format!(
            "| Superglobals | {} | {:.1}% |\n",
            fmt_time(self.superglobals_us),
            pct(self.superglobals_us)
        ));
        report.push_str(&format!(
            "| Script Execution | {} | {:.1}% |\n",
            fmt_time(self.script_exec_us),
            pct(self.script_exec_us)
        ));
        report.push_str(&format!(
            "| Output Capture | {} | {:.1}% |\n",
            fmt_time(self.output_capture_us),
            pct(self.output_capture_us)
        ));
        report.push_str(&format!(
            "| PHP Shutdown | {} | {:.1}% |\n",
            fmt_time(self.php_shutdown_us),
            pct(self.php_shutdown_us)
        ));
        report.push_str(&format!(
            "| Response Build | {} | {:.1}% |\n",
            fmt_time(self.response_build_us),
            pct(self.response_build_us)
        ));

        // Add SAPI callbacks timing if available
        let sapi_total = self.sapi_ub_write_us
            + self.sapi_header_handler_us
            + self.sapi_send_headers_us
            + self.sapi_flush_us
            + self.sapi_read_post_us;
        if sapi_total > 0 {
            report.push_str(&format!(
                "| SAPI Callbacks | {} | {:.1}% |\n",
                fmt_time(sapi_total),
                pct(sapi_total)
            ));
        }

        report.push_str(&format!(
            "| **Total** | **{}** | **100%** |\n",
            fmt_time(self.total_us)
        ));

        report
    }

    /// Write profile report to /tmp/tokio_profile_request_{request_id}.md
    ///
    /// Only available with `debug-profile` feature.
    #[cfg(feature = "debug-profile")]
    pub fn write_report(&self, request_id: &str) {
        let report = self.to_markdown_report(request_id);
        let path = format!("/tmp/tokio_profile_request_{}.md", request_id);

        match std::fs::File::create(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(report.as_bytes()) {
                    tracing::error!("Failed to write profile report to {}: {}", path, e);
                } else {
                    tracing::debug!("Profile report written to {}", path);
                }
            }
            Err(e) => {
                tracing::error!("Failed to create profile report file {}: {}", path, e);
            }
        }
    }
}

/// Timer helper for measuring phases
pub struct Timer {
    start: Instant,
    last: Instant,
}

impl Timer {
    #[inline]
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
        }
    }

    /// Mark a phase and return elapsed microseconds since last mark
    #[inline]
    pub fn mark(&mut self) -> u64 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_micros() as u64;
        self.last = now;
        elapsed
    }

    /// Get total elapsed microseconds since start
    #[inline]
    pub fn total(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}
