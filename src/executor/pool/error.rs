//! Worker pool error types.

use std::fmt;
use std::time::Duration;

/// Errors that can occur during pool operations.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PoolError {
    /// The request queue is full.
    QueueFull {
        /// Maximum queue capacity.
        capacity: usize,
        /// Current number of pending requests.
        pending: usize,
    },

    /// The request timed out.
    Timeout(Duration),

    /// A worker thread panicked.
    WorkerPanic(String),

    /// The pool has been shut down.
    Shutdown,

    /// The response channel was closed unexpectedly.
    ChannelClosed,

    /// Custom execution error.
    Execution(String),
}

#[allow(dead_code)]
impl PoolError {
    /// Check if this is a queue full error.
    pub fn is_queue_full(&self) -> bool {
        matches!(self, PoolError::QueueFull { .. })
    }

    /// Check if this is a timeout error.
    pub fn is_timeout(&self) -> bool {
        matches!(self, PoolError::Timeout(_))
    }

    /// Check if this is a shutdown error.
    pub fn is_shutdown(&self) -> bool {
        matches!(self, PoolError::Shutdown)
    }

    /// Get the error message for logging.
    pub fn message(&self) -> &str {
        match self {
            PoolError::QueueFull { .. } => "Queue full",
            PoolError::Timeout(_) => "Request timeout",
            PoolError::WorkerPanic(_) => "Worker panic",
            PoolError::Shutdown => "Pool shutdown",
            PoolError::ChannelClosed => "Channel closed",
            PoolError::Execution(msg) => msg,
        }
    }
}

impl fmt::Display for PoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PoolError::QueueFull { capacity, pending } => {
                write!(
                    f,
                    "queue full: {}/{} pending requests",
                    pending, capacity
                )
            }
            PoolError::Timeout(duration) => {
                write!(f, "request timeout after {}s", duration.as_secs())
            }
            PoolError::WorkerPanic(msg) => {
                write!(f, "worker panic: {}", msg)
            }
            PoolError::Shutdown => {
                write!(f, "pool has been shut down")
            }
            PoolError::ChannelClosed => {
                write!(f, "response channel closed unexpectedly")
            }
            PoolError::Execution(msg) => {
                write!(f, "execution error: {}", msg)
            }
        }
    }
}

impl std::error::Error for PoolError {}

impl From<String> for PoolError {
    fn from(msg: String) -> Self {
        PoolError::Execution(msg)
    }
}

impl From<&str> for PoolError {
    fn from(msg: &str) -> Self {
        PoolError::Execution(msg.to_string())
    }
}

/// Result type alias for pool operations.
pub type PoolResult<T> = Result<T, PoolError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_full() {
        let err = PoolError::QueueFull {
            capacity: 100,
            pending: 100,
        };
        assert!(err.is_queue_full());
        assert!(!err.is_timeout());
        assert_eq!(err.message(), "Queue full");
    }

    #[test]
    fn test_timeout() {
        let err = PoolError::Timeout(Duration::from_secs(30));
        assert!(err.is_timeout());
        assert!(!err.is_queue_full());
        assert_eq!(err.message(), "Request timeout");
    }

    #[test]
    fn test_from_string() {
        let err: PoolError = "custom error".into();
        assert!(matches!(err, PoolError::Execution(_)));
        assert!(err.to_string().contains("custom error"));
    }
}
