use std::time::Instant;

// Note: Global profiling state has been moved to config::MiddlewareConfig.profile.
// The profiling check is now done at the connection layer using the config value
// combined with the X-Profile: 1 request header.

/// Profile data for a single request
#[derive(Debug, Clone, Default)]
pub struct ProfileData {
    // Total time
    pub total_us: u64,

    // === Connection & TLS (server.rs) ===
    pub tls_handshake_us: u64,       // TLS handshake time (0 for plain HTTP)
    pub http_version: String,        // HTTP/1.0, HTTP/1.1, HTTP/2.0
    pub tls_protocol: String,        // TLS 1.2, TLS 1.3, or empty for plain HTTP
    pub tls_alpn: String,            // ALPN negotiated protocol (h2, http/1.1)

    // === Server-side parsing (server.rs) ===
    pub parse_request_us: u64,       // Total parse time
    pub headers_extract_us: u64,     // Extract HTTP headers from request
    pub query_parse_us: u64,         // Parse query string ($_GET)
    pub cookies_parse_us: u64,       // Parse cookies
    pub body_read_us: u64,           // Read POST body
    pub body_parse_us: u64,          // Parse POST body (form/multipart)
    pub server_vars_us: u64,         // Build $_SERVER vars
    pub path_resolve_us: u64,        // URL decode + path resolution
    pub file_check_us: u64,          // Path::exists() check

    // === Executor queue ===
    pub queue_wait_us: u64,          // Time waiting in worker queue

    // === PHP execution (php.rs) ===
    pub php_startup_us: u64,         // php_request_startup()

    // Superglobals breakdown
    pub superglobals_us: u64,        // Total superglobals time
    pub superglobals_build_us: u64,  // Build PHP code string (eval mode)
    pub superglobals_eval_us: u64,   // zend_eval_string execution (eval mode)

    // FFI superglobals breakdown (USE_EXT=1)
    pub ffi_request_init_us: u64,    // tokio_sapi_request_init()
    pub ffi_clear_us: u64,           // tokio_sapi_clear_superglobals()
    pub ffi_server_us: u64,          // All $_SERVER FFI calls
    pub ffi_server_count: u64,       // Number of $_SERVER entries
    pub ffi_get_us: u64,             // All $_GET FFI calls
    pub ffi_get_count: u64,          // Number of $_GET entries
    pub ffi_post_us: u64,            // All $_POST FFI calls
    pub ffi_post_count: u64,         // Number of $_POST entries
    pub ffi_cookie_us: u64,          // All $_COOKIE FFI calls
    pub ffi_cookie_count: u64,       // Number of $_COOKIE entries
    pub ffi_files_us: u64,           // All $_FILES FFI calls
    pub ffi_files_count: u64,        // Number of $_FILES entries
    pub ffi_build_request_us: u64,   // tokio_sapi_build_request()
    pub ffi_init_eval_us: u64,       // INIT_CODE eval (header_remove, ob_start)

    // I/O setup
    pub memfd_setup_us: u64,         // memfd_create + stdout redirect

    // Script execution
    pub script_exec_us: u64,         // php_execute_script

    // Output capture breakdown
    pub output_capture_us: u64,      // Total output capture time
    pub finalize_eval_us: u64,       // FINALIZE_CODE eval (flush + headers)
    pub stdout_restore_us: u64,      // Restore stdout
    pub output_read_us: u64,         // Read from memfd
    pub output_parse_us: u64,        // Parse body + headers from output

    pub php_shutdown_us: u64,        // php_request_shutdown()

    // === Response building (server.rs) ===
    pub response_build_us: u64,      // Build HTTP response
}

impl ProfileData {
    /// Convert to HTTP header format
    pub fn to_headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![
            // Summary
            ("X-Profile-Total-Us".to_string(), self.total_us.to_string()),

            // Connection & TLS
            ("X-Profile-HTTP-Version".to_string(), self.http_version.clone()),
        ];

        // Only include TLS headers if TLS was used
        if self.tls_handshake_us > 0 {
            headers.push(("X-Profile-TLS-Handshake-Us".to_string(), self.tls_handshake_us.to_string()));
        }
        if !self.tls_protocol.is_empty() {
            headers.push(("X-Profile-TLS-Protocol".to_string(), self.tls_protocol.clone()));
        }
        if !self.tls_alpn.is_empty() {
            headers.push(("X-Profile-TLS-ALPN".to_string(), self.tls_alpn.clone()));
        }

        // Parse breakdown
        headers.extend([
            ("X-Profile-Parse-Us".to_string(), self.parse_request_us.to_string()),
            ("X-Profile-Parse-Headers-Us".to_string(), self.headers_extract_us.to_string()),
            ("X-Profile-Parse-Query-Us".to_string(), self.query_parse_us.to_string()),
            ("X-Profile-Parse-Cookies-Us".to_string(), self.cookies_parse_us.to_string()),
            ("X-Profile-Parse-Body-Read-Us".to_string(), self.body_read_us.to_string()),
            ("X-Profile-Parse-Body-Parse-Us".to_string(), self.body_parse_us.to_string()),
            ("X-Profile-Parse-ServerVars-Us".to_string(), self.server_vars_us.to_string()),
            ("X-Profile-Parse-Path-Us".to_string(), self.path_resolve_us.to_string()),
            ("X-Profile-Parse-FileCheck-Us".to_string(), self.file_check_us.to_string()),

            // Queue
            ("X-Profile-Queue-Us".to_string(), self.queue_wait_us.to_string()),

            // PHP startup
            ("X-Profile-PHP-Startup-Us".to_string(), self.php_startup_us.to_string()),

            // Superglobals breakdown
            ("X-Profile-Superglobals-Us".to_string(), self.superglobals_us.to_string()),
            ("X-Profile-Superglobals-Build-Us".to_string(), self.superglobals_build_us.to_string()),
            ("X-Profile-Superglobals-Eval-Us".to_string(), self.superglobals_eval_us.to_string()),
        ]);

        // FFI-specific headers (only when USE_EXT=1 and values are non-zero)
        if self.ffi_request_init_us > 0 || self.ffi_clear_us > 0 {
            headers.extend([
                ("X-Profile-FFI-Request-Init-Us".to_string(), self.ffi_request_init_us.to_string()),
                ("X-Profile-FFI-Clear-Us".to_string(), self.ffi_clear_us.to_string()),
                ("X-Profile-FFI-Server-Us".to_string(), self.ffi_server_us.to_string()),
                ("X-Profile-FFI-Server-Count".to_string(), self.ffi_server_count.to_string()),
                ("X-Profile-FFI-Get-Us".to_string(), self.ffi_get_us.to_string()),
                ("X-Profile-FFI-Get-Count".to_string(), self.ffi_get_count.to_string()),
                ("X-Profile-FFI-Post-Us".to_string(), self.ffi_post_us.to_string()),
                ("X-Profile-FFI-Post-Count".to_string(), self.ffi_post_count.to_string()),
                ("X-Profile-FFI-Cookie-Us".to_string(), self.ffi_cookie_us.to_string()),
                ("X-Profile-FFI-Cookie-Count".to_string(), self.ffi_cookie_count.to_string()),
                ("X-Profile-FFI-Files-Us".to_string(), self.ffi_files_us.to_string()),
                ("X-Profile-FFI-Files-Count".to_string(), self.ffi_files_count.to_string()),
                ("X-Profile-FFI-Build-Request-Us".to_string(), self.ffi_build_request_us.to_string()),
                ("X-Profile-FFI-Init-Eval-Us".to_string(), self.ffi_init_eval_us.to_string()),
            ]);
        }

        headers.extend([
            // I/O setup
            ("X-Profile-Memfd-Setup-Us".to_string(), self.memfd_setup_us.to_string()),

            // Script
            ("X-Profile-Script-Us".to_string(), self.script_exec_us.to_string()),

            // Output breakdown
            ("X-Profile-Output-Us".to_string(), self.output_capture_us.to_string()),
            ("X-Profile-Output-Finalize-Us".to_string(), self.finalize_eval_us.to_string()),
            ("X-Profile-Output-Restore-Us".to_string(), self.stdout_restore_us.to_string()),
            ("X-Profile-Output-Read-Us".to_string(), self.output_read_us.to_string()),
            ("X-Profile-Output-Parse-Us".to_string(), self.output_parse_us.to_string()),

            // Shutdown
            ("X-Profile-PHP-Shutdown-Us".to_string(), self.php_shutdown_us.to_string()),

            // Response
            ("X-Profile-Response-Us".to_string(), self.response_build_us.to_string()),
        ]);

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
        Self { start: now, last: now }
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
