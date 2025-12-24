//! Alternative PHP executor using SAPI module initialization.
//!
//! This executor provides the same functionality as PhpExecutor but uses
//! a separate SAPI initialization path via the sapi module.

use async_trait::async_trait;

use super::common::{self, WorkerPool};
use super::sapi;
use super::{ExecutorError, ScriptExecutor};
use crate::types::{ScriptRequest, ScriptResponse};

// =============================================================================
// PHP SAPI Pool
// =============================================================================

struct PhpSapiPool {
    pool: WorkerPool,
}

impl PhpSapiPool {
    fn new(num_workers: usize) -> Result<Self, String> {
        // Initialize custom SAPI
        sapi::init()?;

        let pool = WorkerPool::new(num_workers, "php-sapi", |id, rx| {
            common::worker_main_loop(id, rx);
        })?;

        for id in 0..num_workers {
            tracing::debug!("Spawned PHP SAPI worker thread {}", id);
        }

        tracing::info!("PHP SAPI pool initialized with {} workers", num_workers);

        Ok(Self { pool })
    }

    async fn execute_request(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        self.pool.execute(request).await
    }

    fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

impl Drop for PhpSapiPool {
    fn drop(&mut self) {
        self.pool.join_all();
        sapi::shutdown();
    }
}

// =============================================================================
// Public Executor Interface
// =============================================================================

/// PHP script executor using custom SAPI initialization.
pub struct PhpSapiExecutor {
    pool: PhpSapiPool,
}

impl PhpSapiExecutor {
    /// Creates a new PHP SAPI executor with the specified number of worker threads.
    pub fn new(num_workers: usize) -> Result<Self, ExecutorError> {
        let pool = PhpSapiPool::new(num_workers)?;
        Ok(Self { pool })
    }

    /// Returns the number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

#[async_trait]
impl ScriptExecutor for PhpSapiExecutor {
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        self.pool
            .execute_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    fn name(&self) -> &'static str {
        "php-sapi"
    }

    fn shutdown(&self) {
        // Pool shutdown handled by Drop
    }
}
