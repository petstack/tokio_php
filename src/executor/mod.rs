//! Script execution backends for tokio_php.
//!
//! This module provides pluggable executors for script execution.
//! The server uses the [`ScriptExecutor`] trait to abstract over different
//! execution backends.
//!
//! # Available Executors
//!
//! | Executor | Feature | Description |
//! |----------|---------|-------------|
//! | [`ExtExecutor`] | `php` | **Recommended.** Uses FFI for superglobals, best performance |
//! | [`PhpExecutor`] | `php` | Legacy executor using `zend_eval_string` |
//! | [`StubExecutor`] | - | Returns empty responses, useful for benchmarking |
//!
//! # Performance Comparison
//!
//! For real applications using superglobals (`$_GET`, `$_POST`, `$_SERVER`),
//! [`ExtExecutor`] is ~48% faster than [`PhpExecutor`] due to FFI batch API.
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::executor::{ScriptExecutor, ExtExecutor};
//! use tokio_php::types::ScriptRequest;
//!
//! // Create executor with 4 worker threads
//! let executor = ExtExecutor::new(4, 400)?;
//!
//! // Execute a script
//! let request = ScriptRequest::new("/var/www/html/index.php");
//! let response = executor.execute(request).await?;
//! ```
//!
//! # Worker Pool Architecture
//!
//! Both [`ExtExecutor`] and [`PhpExecutor`] use a multi-threaded worker pool:
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐
//! │   Request   │────▶│ mpsc channel │────▶│   Worker    │
//! └─────────────┘     └──────────────┘     │   Thread    │
//!                            │             │ (PHP TSRM)  │
//!                            │             └─────────────┘
//!                            │             ┌─────────────┐
//!                            └────────────▶│   Worker    │
//!                                          │   Thread    │
//!                                          └─────────────┘
//! ```
//!
//! Each worker thread has its own PHP context via TSRM (Thread Safe Resource Manager).

mod stub;

#[cfg(feature = "php")]
mod common;

// Legacy executors (require C extension) - only when NOT using tokio-sapi
#[cfg(all(feature = "php", not(feature = "tokio-sapi")))]
mod php;

#[cfg(all(feature = "php", not(feature = "tokio-sapi")))]
pub mod sapi;

#[cfg(all(feature = "php", not(feature = "tokio-sapi")))]
mod ext;

// Pure Rust SAPI executor (no C extension dependency)
#[cfg(feature = "tokio-sapi")]
mod sapi_executor;

use async_trait::async_trait;

pub use stub::StubExecutor;

// Legacy executors (require C extension)
#[cfg(all(feature = "php", not(feature = "tokio-sapi")))]
pub use php::PhpExecutor;

#[cfg(all(feature = "php", not(feature = "tokio-sapi")))]
pub use ext::ExtExecutor;

// ResponseChunk re-export
#[cfg(all(feature = "php", not(feature = "tokio-sapi")))]
pub use sapi::ResponseChunk;

#[cfg(feature = "tokio-sapi")]
pub use crate::sapi::ResponseChunk;

// Pure Rust SAPI executor
#[cfg(feature = "tokio-sapi")]
pub use sapi_executor::SapiExecutor;

#[cfg(feature = "php")]
pub use common::QUEUE_FULL_ERROR;

#[cfg(feature = "php")]
pub use common::REQUEST_TIMEOUT_ERROR;

#[cfg(feature = "php")]
pub use common::ExecuteResult;

use crate::server::response::StreamChunk;
use crate::types::{ScriptRequest, ScriptResponse};

/// Default buffer size for streaming channels.
pub const DEFAULT_STREAM_BUFFER_SIZE: usize = 100;

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

    /// Executes a streaming script (SSE).
    ///
    /// Returns immediately with a receiver for streaming chunks.
    /// The PHP script sends chunks via `tokio_stream_flush()`.
    ///
    /// Default implementation returns an error (not supported).
    async fn execute_streaming(
        &self,
        _request: ScriptRequest,
        _buffer_size: usize,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>, ExecutorError> {
        Err(ExecutorError::from(
            "Streaming not supported by this executor",
        ))
    }

    /// Executes a request with automatic SSE detection.
    ///
    /// Similar to `execute()`, but also detects when PHP dynamically enables
    /// SSE by setting `Content-Type: text/event-stream` header.
    ///
    /// Returns `ExecuteResult::Normal` for regular responses, or
    /// `ExecuteResult::Streaming` when SSE is auto-detected.
    ///
    /// Default implementation just calls `execute()` and wraps in `Normal`.
    #[cfg(feature = "php")]
    async fn execute_with_auto_sse(
        &self,
        request: ScriptRequest,
    ) -> Result<ExecuteResult, ExecutorError> {
        self.execute(request)
            .await
            .map(|r| ExecuteResult::Normal(Box::new(r)))
    }
}
