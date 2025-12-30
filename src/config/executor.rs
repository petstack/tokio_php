//! Executor configuration.

use super::parse::{env_bool, env_or};
use super::ConfigError;

/// Executor type selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutorType {
    /// Stub executor - returns empty responses (for benchmarking).
    Stub,
    /// PHP executor using zend_eval_string (legacy).
    Php,
    /// Ext executor using php_execute_script with FFI superglobals (recommended).
    Ext,
}

impl Default for ExecutorType {
    fn default() -> Self {
        Self::Ext
    }
}

/// Executor configuration loaded from environment.
#[derive(Clone, Debug)]
pub struct ExecutorConfig {
    /// Executor type to use.
    pub executor_type: ExecutorType,
    /// Number of worker threads (0 = auto-detect from CPU cores).
    pub workers: usize,
    /// Queue capacity for pending requests (0 = workers * 100).
    pub queue_capacity: usize,
}

impl ExecutorConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        // Determine executor type
        let use_stub = env_bool("USE_STUB", false);
        let use_ext = env_bool("USE_EXT", false);

        let executor_type = if use_stub {
            ExecutorType::Stub
        } else if use_ext {
            ExecutorType::Ext
        } else {
            ExecutorType::Php
        };

        // Parse worker count
        let workers: usize = env_or("PHP_WORKERS", "0")
            .parse()
            .map_err(|e| ConfigError::Parse {
                key: "PHP_WORKERS".into(),
                value: env_or("PHP_WORKERS", "0"),
                error: format!("{}", e),
            })?;

        // Parse queue capacity
        let queue_capacity: usize = env_or("QUEUE_CAPACITY", "0")
            .parse()
            .map_err(|e| ConfigError::Parse {
                key: "QUEUE_CAPACITY".into(),
                value: env_or("QUEUE_CAPACITY", "0"),
                error: format!("{}", e),
            })?;

        Ok(Self {
            executor_type,
            workers,
            queue_capacity,
        })
    }

    /// Get actual worker count (resolves 0 to CPU count).
    pub fn worker_count(&self) -> usize {
        if self.workers == 0 {
            num_cpus::get()
        } else {
            self.workers
        }
    }

    /// Get actual queue capacity (resolves 0 to workers * 100).
    pub fn actual_queue_capacity(&self) -> usize {
        if self.queue_capacity == 0 {
            self.worker_count() * 100
        } else {
            self.queue_capacity
        }
    }
}
