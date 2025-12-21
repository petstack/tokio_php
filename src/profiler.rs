use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Global flag to enable/disable profiling via PROFILE env var
static PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize profiler from environment
pub fn init() {
    let enabled = std::env::var("PROFILE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    PROFILING_ENABLED.store(enabled, Ordering::Relaxed);

    if enabled {
        tracing::info!("Profiler enabled (PROFILE=1)");
    }
}

/// Check if profiling is globally enabled
#[inline]
pub fn is_enabled() -> bool {
    PROFILING_ENABLED.load(Ordering::Relaxed)
}

/// Profile data for a single request
#[derive(Debug, Clone, Default)]
pub struct ProfileData {
    // Total time
    pub total_us: u64,

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
    pub superglobals_build_us: u64,  // Build PHP code string
    pub superglobals_eval_us: u64,   // zend_eval_string execution

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
        vec![
            // Summary
            ("X-Profile-Total-Us".to_string(), self.total_us.to_string()),

            // Parse breakdown
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
        ]
    }

    /// Format as human-readable string (summary only)
    pub fn to_summary(&self) -> String {
        format!(
            "total={}us parse={}us queue={}us php_start={}us globals={}us script={}us output={}us php_end={}us resp={}us",
            self.total_us,
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
