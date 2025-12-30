//! Generic worker pool infrastructure.
//!
//! This module provides a reusable thread pool implementation for
//! executing blocking tasks asynchronously.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                      ThreadPool                            │
//! ├────────────────────────────────────────────────────────────┤
//! │  ┌─────────┐    ┌─────────┐    ┌─────────┐                 │
//! │  │ Worker1 │    │ Worker2 │    │ Worker3 │  ...            │
//! │  └────┬────┘    └────┬────┘    └────┬────┘                 │
//! │       │              │              │                      │
//! │       └──────────────┴──────────────┘                      │
//! │                      │                                     │
//! │              ┌───────▼───────┐                             │
//! │              │  mpsc channel │  (bounded queue)            │
//! │              └───────┬───────┘                             │
//! │                      │                                     │
//! │              ┌───────▼───────┐                             │
//! │              │    execute()  │  (async interface)          │
//! │              └───────────────┘                             │
//! └────────────────────────────────────────────────────────────┘
//! ```

mod error;
mod thread;

// Re-exports - allow unused since this is a library API
#[allow(unused_imports)]
pub use error::{PoolError, PoolResult};
#[allow(unused_imports)]
pub use thread::ThreadPool;

use std::time::Duration;

/// Trait for worker pool implementations.
///
/// This trait defines the interface for executing tasks on a pool of workers.
/// Implementations must be thread-safe and support async execution.
#[allow(dead_code)]
pub trait WorkerPool: Send + Sync {
    /// The request type sent to workers.
    type Request: Send + 'static;

    /// The response type returned by workers.
    type Response: Send + 'static;

    /// Execute a request on the worker pool.
    ///
    /// Returns the response or an error if execution failed.
    fn execute(
        &self,
        request: Self::Request,
    ) -> impl std::future::Future<Output = PoolResult<Self::Response>> + Send;

    /// Execute a request with a timeout.
    ///
    /// Returns `PoolError::Timeout` if the deadline is exceeded.
    fn execute_with_timeout(
        &self,
        request: Self::Request,
        timeout: Duration,
    ) -> impl std::future::Future<Output = PoolResult<Self::Response>> + Send;

    /// Returns the number of worker threads.
    fn worker_count(&self) -> usize;

    /// Returns the maximum queue capacity.
    fn queue_capacity(&self) -> usize;

    /// Returns the current number of pending requests in the queue.
    fn pending_count(&self) -> usize;

    /// Gracefully shutdown the pool.
    fn shutdown(&self);
}

/// Statistics about pool performance.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct PoolStats {
    /// Total requests processed.
    pub total_requests: u64,
    /// Requests that timed out.
    pub timeouts: u64,
    /// Requests rejected due to full queue.
    pub rejected: u64,
    /// Average queue wait time in microseconds.
    pub avg_queue_wait_us: u64,
    /// Average execution time in microseconds.
    pub avg_exec_time_us: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_error_display() {
        let err = PoolError::QueueFull {
            capacity: 100,
            pending: 100,
        };
        assert!(err.to_string().contains("100"));

        let err = PoolError::Timeout(Duration::from_secs(30));
        assert!(err.to_string().contains("30"));
    }
}
