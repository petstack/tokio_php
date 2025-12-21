use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Global flag to enable/disable profiling via PROFILE env var
static PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize profiler from environment
pub fn init() {
    let enabled = std::env::var("PROFILE")
        .map(|v| v == "1" || v.to_lowercase() == "true")
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
    pub total_us: u64,
    pub parse_request_us: u64,
    pub queue_wait_us: u64,
    pub php_startup_us: u64,
    pub superglobals_us: u64,
    pub script_exec_us: u64,
    pub output_capture_us: u64,
    pub php_shutdown_us: u64,
    pub response_build_us: u64,
}

impl ProfileData {
    /// Convert to HTTP header format
    pub fn to_headers(&self) -> Vec<(String, String)> {
        vec![
            ("X-Profile-Total-Us".to_string(), self.total_us.to_string()),
            ("X-Profile-Parse-Us".to_string(), self.parse_request_us.to_string()),
            ("X-Profile-Queue-Us".to_string(), self.queue_wait_us.to_string()),
            ("X-Profile-PHP-Startup-Us".to_string(), self.php_startup_us.to_string()),
            ("X-Profile-Superglobals-Us".to_string(), self.superglobals_us.to_string()),
            ("X-Profile-Script-Us".to_string(), self.script_exec_us.to_string()),
            ("X-Profile-Output-Us".to_string(), self.output_capture_us.to_string()),
            ("X-Profile-PHP-Shutdown-Us".to_string(), self.php_shutdown_us.to_string()),
            ("X-Profile-Response-Us".to_string(), self.response_build_us.to_string()),
        ]
    }

    /// Format as human-readable string
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
