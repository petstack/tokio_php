//! Executor configuration.

use super::parse::env_or;
use super::ConfigError;
use std::num::NonZeroUsize;

/// Executor type selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExecutorType {
    /// Stub executor - returns empty responses (for benchmarking).
    Stub,
    /// PHP executor using zend_eval_string (legacy).
    Php,
    /// Ext executor using php_execute_script with FFI superglobals (default, recommended).
    #[default]
    Ext,
}

/// Executor configuration loaded from environment.
///
/// All values are pre-computed at construction time for zero-cost access.
#[derive(Clone, Debug)]
pub struct ExecutorConfig {
    /// Executor type to use.
    pub executor_type: ExecutorType,
    /// Resolved worker count (never zero).
    worker_count: NonZeroUsize,
    /// Resolved queue capacity (never zero).
    queue_capacity: NonZeroUsize,
}

impl ExecutorConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        let executor_type = Self::parse_executor_type();
        let worker_count = Self::parse_worker_count()?;
        let queue_capacity = Self::parse_queue_capacity(worker_count)?;

        Ok(Self {
            executor_type,
            worker_count,
            queue_capacity,
        })
    }

    /// Get worker count (pre-computed, zero-cost).
    #[inline]
    pub fn worker_count(&self) -> usize {
        self.worker_count.get()
    }

    /// Get queue capacity (pre-computed, zero-cost).
    #[inline]
    pub fn queue_capacity(&self) -> usize {
        self.queue_capacity.get()
    }

    fn parse_executor_type() -> ExecutorType {
        match env_or("EXECUTOR", "ext").to_lowercase().as_str() {
            "stub" => ExecutorType::Stub,
            "php" => ExecutorType::Php,
            _ => ExecutorType::Ext, // "ext" or any other value defaults to Ext
        }
    }

    fn parse_worker_count() -> Result<NonZeroUsize, ConfigError> {
        // Debug profile: force single worker for accurate profiling
        #[cfg(feature = "debug-profile")]
        {
            Ok(NonZeroUsize::new(1).unwrap())
        }

        #[cfg(not(feature = "debug-profile"))]
        {
            let raw = env_or("PHP_WORKERS", "0");
            let workers: usize = raw.parse().map_err(|e| ConfigError::Parse {
                key: "PHP_WORKERS".into(),
                value: raw,
                error: format!("{e}"),
            })?;

            // Resolve 0 to CPU count
            let count = if workers == 0 {
                num_cpus::get()
            } else {
                workers
            };

            NonZeroUsize::new(count).ok_or_else(|| ConfigError::Invalid {
                key: "PHP_WORKERS".into(),
                message: "worker count cannot be zero".into(),
            })
        }
    }

    fn parse_queue_capacity(workers: NonZeroUsize) -> Result<NonZeroUsize, ConfigError> {
        let raw = env_or("QUEUE_CAPACITY", "0");
        let capacity: usize = raw.parse().map_err(|e| ConfigError::Parse {
            key: "QUEUE_CAPACITY".into(),
            value: raw,
            error: format!("{e}"),
        })?;

        // Resolve 0 to workers * 100
        let count = if capacity == 0 {
            workers.get() * 100
        } else {
            capacity
        };

        NonZeroUsize::new(count).ok_or_else(|| ConfigError::Invalid {
            key: "QUEUE_CAPACITY".into(),
            message: "queue capacity cannot be zero".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_type_default_is_ext() {
        assert_eq!(ExecutorType::default(), ExecutorType::Ext);
    }

    #[test]
    fn test_worker_count_explicit() {
        let config = ExecutorConfig {
            executor_type: ExecutorType::Ext,
            worker_count: NonZeroUsize::new(4).unwrap(),
            queue_capacity: NonZeroUsize::new(400).unwrap(),
        };
        assert_eq!(config.worker_count(), 4);
    }

    #[test]
    fn test_queue_capacity_explicit() {
        let config = ExecutorConfig {
            executor_type: ExecutorType::Ext,
            worker_count: NonZeroUsize::new(4).unwrap(),
            queue_capacity: NonZeroUsize::new(500).unwrap(),
        };
        assert_eq!(config.queue_capacity(), 500);
    }

    #[test]
    fn test_queue_capacity_derived() {
        let config = ExecutorConfig {
            executor_type: ExecutorType::Ext,
            worker_count: NonZeroUsize::new(4).unwrap(),
            queue_capacity: NonZeroUsize::new(400).unwrap(), // 4 * 100
        };
        assert_eq!(config.queue_capacity(), 400);
    }
}
