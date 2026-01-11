//! PHP script executor using custom SAPI initialization.
//!
//! This executor provides PHP script execution with custom SAPI callbacks
//! for proper header handling via the sapi module.

use async_trait::async_trait;

use super::common::{self, WorkerPool};
use super::sapi;
use super::{ExecutorError, ScriptExecutor};
use crate::types::{ScriptRequest, ScriptResponse};

// =============================================================================
// PHP Pool
// =============================================================================

struct PhpPool {
    pool: WorkerPool,
}

impl PhpPool {
    fn with_queue_capacity(num_workers: usize, queue_capacity: usize) -> Result<Self, String> {
        // Initialize custom SAPI
        sapi::init()?;

        let pool = if queue_capacity > 0 {
            WorkerPool::with_queue_capacity(num_workers, "php", queue_capacity, |id, rx| {
                common::worker_main_loop(id, rx);
            })?
        } else {
            WorkerPool::new(num_workers, "php", |id, rx| {
                common::worker_main_loop(id, rx);
            })?
        };

        for id in 0..num_workers {
            tracing::debug!("Spawned PHP worker thread {}", id);
        }

        tracing::info!(
            "PHP pool initialized with {} workers, queue capacity {}",
            num_workers,
            pool.queue_capacity()
        );

        Ok(Self { pool })
    }

    async fn execute_request(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        self.pool.execute(request).await
    }

    fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

impl Drop for PhpPool {
    fn drop(&mut self) {
        self.pool.join_all();
        sapi::shutdown();
    }
}

// =============================================================================
// Public Executor Interface
// =============================================================================

/// PHP script executor using custom SAPI initialization.
pub struct PhpExecutor {
    pool: PhpPool,
}

impl PhpExecutor {
    /// Creates a new PHP executor with custom queue capacity.
    /// If queue_capacity is 0, uses default (workers * 100).
    pub fn with_queue_capacity(
        num_workers: usize,
        queue_capacity: usize,
    ) -> Result<Self, ExecutorError> {
        let pool = PhpPool::with_queue_capacity(num_workers, queue_capacity)?;
        Ok(Self { pool })
    }

    /// Returns the number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

#[async_trait]
impl ScriptExecutor for PhpExecutor {
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        self.pool
            .execute_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    fn name(&self) -> &'static str {
        "php"
    }

    fn shutdown(&self) {
        // Pool shutdown handled by Drop
    }
}
