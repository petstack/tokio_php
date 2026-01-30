//! Health checker implementation.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{CheckResult, HealthDetails, HealthStatus, ProbeType};

/// Health checker configuration.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// Queue depth threshold for readiness (0.0-1.0)
    pub max_queue_depth_percent: f64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            max_queue_depth_percent: 0.9,
        }
    }
}

/// Health checker for Kubernetes probes.
///
/// Tracks server state and performs health checks based on probe type.
pub struct HealthChecker {
    /// Flag indicating startup is complete
    startup_complete: Arc<AtomicBool>,
    /// Flag indicating PHP is initialized
    php_initialized: Arc<AtomicBool>,
    /// Server start time for uptime calculation
    start_time: Instant,
    /// Health check configuration
    config: HealthConfig,
    /// Active connections counter (shared with server)
    active_connections: Arc<AtomicUsize>,
    /// Queue depth (current pending requests)
    queue_depth: Arc<AtomicUsize>,
    /// Queue capacity (max pending requests)
    queue_capacity: usize,
    /// Worker count
    worker_count: usize,
}

impl HealthChecker {
    /// Create a new health checker.
    pub fn new(
        worker_count: usize,
        queue_capacity: usize,
        active_connections: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            startup_complete: Arc::new(AtomicBool::new(false)),
            php_initialized: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
            config: HealthConfig::default(),
            active_connections,
            queue_depth: Arc::new(AtomicUsize::new(0)),
            queue_capacity,
            worker_count,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(mut self, config: HealthConfig) -> Self {
        self.config = config;
        self
    }

    /// Mark startup as complete.
    pub fn mark_startup_complete(&self) {
        self.startup_complete.store(true, Ordering::SeqCst);
    }

    /// Mark PHP as initialized.
    pub fn mark_php_initialized(&self) {
        self.php_initialized.store(true, Ordering::SeqCst);
    }

    /// Update queue depth (called by executor).
    pub fn set_queue_depth(&self, depth: usize) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }

    /// Get queue depth receiver for updates.
    pub fn queue_depth_ref(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.queue_depth)
    }

    /// Check if startup is complete.
    pub fn is_startup_complete(&self) -> bool {
        self.startup_complete.load(Ordering::Relaxed)
    }

    /// Check if PHP is initialized.
    pub fn is_php_initialized(&self) -> bool {
        self.php_initialized.load(Ordering::Relaxed)
    }

    /// Perform health check based on probe type.
    pub fn check(&self, probe: ProbeType) -> HealthStatus {
        match probe {
            ProbeType::Liveness => self.check_liveness(),
            ProbeType::Readiness => self.check_readiness(),
            ProbeType::Startup => self.check_startup(),
        }
    }

    /// Liveness probe: Is the process functioning?
    ///
    /// Checks:
    /// - PHP initialized
    /// - Workers alive
    fn check_liveness(&self) -> HealthStatus {
        let start = Instant::now();

        // Check 1: PHP initialized
        let php_ok = self.php_initialized.load(Ordering::Relaxed);
        let php_check = if php_ok {
            CheckResult::pass("php_initialized", start.elapsed().as_millis() as u64)
        } else {
            CheckResult::fail(
                "php_initialized",
                "PHP not initialized",
                start.elapsed().as_millis() as u64,
            )
        };

        // Check 2: Workers exist (basic sanity check)
        let workers_ok = self.worker_count > 0;
        let workers_check = if workers_ok {
            CheckResult::pass("workers_alive", start.elapsed().as_millis() as u64)
        } else {
            CheckResult::fail(
                "workers_alive",
                "No workers configured",
                start.elapsed().as_millis() as u64,
            )
        };

        let all_pass = php_check.is_pass() && workers_check.is_pass();

        if all_pass {
            HealthStatus::healthy()
                .with_check(php_check)
                .with_check(workers_check)
        } else {
            HealthStatus::unhealthy("Liveness check failed")
                .with_check(php_check)
                .with_check(workers_check)
        }
    }

    /// Readiness probe: Can we serve traffic?
    ///
    /// Checks:
    /// - Startup complete
    /// - Queue not full
    fn check_readiness(&self) -> HealthStatus {
        let start = Instant::now();

        // Check 1: Startup complete
        let startup_ok = self.startup_complete.load(Ordering::Relaxed);
        let startup_check = if startup_ok {
            CheckResult::pass("startup_complete", start.elapsed().as_millis() as u64)
        } else {
            CheckResult::fail(
                "startup_complete",
                "Startup in progress",
                start.elapsed().as_millis() as u64,
            )
        };

        if !startup_ok {
            return HealthStatus::not_ready("Startup not complete").with_check(startup_check);
        }

        // Check 2: Queue not full
        let queue_depth = self.queue_depth.load(Ordering::Relaxed);
        let queue_percent = if self.queue_capacity > 0 {
            queue_depth as f64 / self.queue_capacity as f64
        } else {
            0.0
        };
        let queue_ok = queue_percent < self.config.max_queue_depth_percent;

        let queue_check = if queue_ok {
            CheckResult::pass("queue_capacity", start.elapsed().as_millis() as u64)
        } else {
            CheckResult::warn(
                "queue_capacity",
                format!(
                    "{}/{} ({:.1}%)",
                    queue_depth,
                    self.queue_capacity,
                    queue_percent * 100.0
                ),
                start.elapsed().as_millis() as u64,
            )
        };

        // Build details
        let details = HealthDetails {
            uptime_seconds: self.start_time.elapsed().as_secs(),
            version: env!("CARGO_PKG_VERSION"),
            workers: self.worker_count,
            queue_depth,
            queue_capacity: self.queue_capacity,
            active_connections: self.active_connections.load(Ordering::Relaxed),
        };

        // Readiness passes even with queue warning (only fails on startup incomplete)
        HealthStatus::healthy()
            .with_check(startup_check)
            .with_check(queue_check)
            .with_details(details)
    }

    /// Startup probe: Has initialization finished?
    ///
    /// Checks:
    /// - PHP initialized
    /// - Workers ready (startup complete)
    fn check_startup(&self) -> HealthStatus {
        let php_ok = self.php_initialized.load(Ordering::Relaxed);
        let startup_ok = self.startup_complete.load(Ordering::Relaxed);

        let php_check = if php_ok {
            CheckResult::pass("php_initialized", 0)
        } else {
            CheckResult::pending("php_initialized")
        };

        let workers_check = if startup_ok {
            CheckResult::pass("workers_ready", 0)
        } else {
            CheckResult::pending("workers_ready")
        };

        let all_pass = php_ok && startup_ok;

        if all_pass {
            let details = HealthDetails {
                uptime_seconds: self.start_time.elapsed().as_secs(),
                version: env!("CARGO_PKG_VERSION"),
                workers: self.worker_count,
                queue_depth: self.queue_depth.load(Ordering::Relaxed),
                queue_capacity: self.queue_capacity,
                active_connections: self.active_connections.load(Ordering::Relaxed),
            };

            HealthStatus::healthy()
                .with_check(php_check)
                .with_check(workers_check)
                .with_details(details)
        } else {
            HealthStatus::not_ready("Initialization in progress")
                .with_check(php_check)
                .with_check(workers_check)
        }
    }

    /// Get server uptime.
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liveness_healthy() {
        let checker = HealthChecker::new(4, 400, Arc::new(AtomicUsize::new(0)));
        checker.mark_php_initialized();

        let status = checker.check(ProbeType::Liveness);
        assert!(status.is_healthy());
        assert_eq!(status.checks.len(), 2);
    }

    #[test]
    fn test_liveness_unhealthy_php_not_initialized() {
        let checker = HealthChecker::new(4, 400, Arc::new(AtomicUsize::new(0)));

        let status = checker.check(ProbeType::Liveness);
        assert!(!status.is_healthy());
        assert_eq!(status.status, "unhealthy");
    }

    #[test]
    fn test_readiness_not_ready_startup_incomplete() {
        let checker = HealthChecker::new(4, 400, Arc::new(AtomicUsize::new(0)));
        checker.mark_php_initialized();
        // Don't mark startup complete

        let status = checker.check(ProbeType::Readiness);
        assert!(!status.is_healthy());
        assert_eq!(status.status, "not_ready");
    }

    #[test]
    fn test_readiness_healthy() {
        let checker = HealthChecker::new(4, 400, Arc::new(AtomicUsize::new(0)));
        checker.mark_php_initialized();
        checker.mark_startup_complete();

        let status = checker.check(ProbeType::Readiness);
        assert!(status.is_healthy());
        assert!(status.details.is_some());
    }

    #[test]
    fn test_startup_not_ready() {
        let checker = HealthChecker::new(4, 400, Arc::new(AtomicUsize::new(0)));

        let status = checker.check(ProbeType::Startup);
        assert!(!status.is_healthy());
        assert_eq!(status.status, "not_ready");
    }

    #[test]
    fn test_startup_healthy() {
        let checker = HealthChecker::new(4, 400, Arc::new(AtomicUsize::new(0)));
        checker.mark_php_initialized();
        checker.mark_startup_complete();

        let status = checker.check(ProbeType::Startup);
        assert!(status.is_healthy());
    }
}
