//! Self-diagnostics system for tokio_php
//!
//! Provides comprehensive diagnostics about OS limits, runtime performance,
//! and actionable recommendations for optimization.
//!
//! ## Usage
//!
//! Add to internal server routes:
//!
//! ```rust,ignore
//! use tokio_php::diagnostics::{DiagnosticCollector, DiagnosticResponse};
//!
//! async fn diagnostics_handler(
//!     state: Arc<AppState>,
//! ) -> Result<Json<DiagnosticResponse>, StatusCode> {
//!     let collector = DiagnosticCollector::new();
//!
//!     // Gather current metrics from your app state
//!     let metrics = state.metrics.snapshot();
//!
//!     let response = collector.collect(
//!         state.runtime_handle(),
//!         metrics.worker_count,
//!         metrics.busy_workers,
//!         metrics.queue_depth,
//!         metrics.total_requests,
//!         metrics.execution_times_ms,
//!         metrics.wait_times_ms,
//!         metrics.php_memory_per_worker,
//!         metrics.file_cache_size,
//!         metrics.lock_stats.worker_pool_wait_ns,
//!         metrics.lock_stats.worker_pool_hold_ns,
//!         metrics.lock_stats.file_cache_wait_ns,
//!         metrics.lock_stats.file_cache_hold_ns,
//!         metrics.lock_stats.config_wait_ns,
//!         metrics.lock_stats.config_hold_ns,
//!     ).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
//!
//!     Ok(Json(response))
//! }
//! ```

pub mod analyzer;
pub mod collector;
pub mod os;
pub mod recommender;
pub mod runtime;
pub mod types;

pub use collector::DiagnosticCollector;
pub use types::{DiagnosticResponse, PlatformInfo, RuntimeMetrics};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_score_calculation() {
        // This would test the health score algorithm
    }
}
