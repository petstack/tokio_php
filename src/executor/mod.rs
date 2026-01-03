mod stub;

#[cfg(feature = "php")]
mod common;

#[cfg(feature = "php")]
mod php;

#[cfg(feature = "php")]
pub mod sapi;

#[cfg(feature = "php")]
mod ext;

use async_trait::async_trait;

pub use stub::StubExecutor;

#[cfg(feature = "php")]
pub use php::PhpExecutor;

#[cfg(feature = "php")]
pub use ext::ExtExecutor;

#[cfg(feature = "php")]
pub use common::QUEUE_FULL_ERROR;

#[cfg(feature = "php")]
pub use common::REQUEST_TIMEOUT_ERROR;

use crate::types::{ScriptRequest, ScriptResponse};

/// Error type for script execution.
#[derive(Debug, Clone)]
pub struct ExecutorError {
    pub message: String,
}

impl ExecutorError {
    /// Returns true if this error indicates the worker queue is full.
    #[cfg(feature = "php")]
    pub fn is_queue_full(&self) -> bool {
        self.message == QUEUE_FULL_ERROR
    }

    #[cfg(not(feature = "php"))]
    pub fn is_queue_full(&self) -> bool {
        false
    }

    /// Returns true if this error indicates a request timeout.
    #[cfg(feature = "php")]
    pub fn is_timeout(&self) -> bool {
        self.message == REQUEST_TIMEOUT_ERROR
    }

    #[cfg(not(feature = "php"))]
    pub fn is_timeout(&self) -> bool {
        false
    }
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ExecutorError {}

impl From<String> for ExecutorError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for ExecutorError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

/// Trait for script execution backends.
///
/// This trait defines the interface for executing scripts (PHP, stubs, etc.).
/// Implementations must be thread-safe and async-compatible.
///
/// # SOLID Principles
/// - **S**ingle Responsibility: Each executor handles only script execution
/// - **O**pen/Closed: New executors can be added without modifying existing code
/// - **L**iskov Substitution: All executors are interchangeable via this trait
/// - **I**nterface Segregation: Minimal interface with only essential methods
/// - **D**ependency Inversion: Server depends on this abstraction, not concrete implementations
#[async_trait]
pub trait ScriptExecutor: Send + Sync {
    /// Executes a script with the given request data.
    ///
    /// # Arguments
    /// * `request` - The script request containing path, params, headers, etc.
    ///
    /// # Returns
    /// * `Ok(ScriptResponse)` - The execution result with body and headers
    /// * `Err(ExecutorError)` - If execution failed
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError>;

    /// Returns the name of this executor for logging purposes.
    fn name(&self) -> &'static str;

    /// Shuts down the executor, releasing any resources.
    fn shutdown(&self) {}

    /// Returns true if this executor should skip file existence checks.
    /// Stub executors return true for maximum performance.
    fn skip_file_check(&self) -> bool {
        false
    }
}
