//! tokio_php - Async PHP web server powered by Rust and Tokio.
//!
//! This crate provides an async HTTP server that executes PHP scripts
//! via the php-embed SAPI. It supports HTTP/1.1, HTTP/2, and HTTPS with TLS 1.3.
//!
//! # Features
//!
//! - **Async I/O**: Built on Tokio for high-performance async networking
//! - **HTTP/2 Support**: Full HTTP/2 with ALPN negotiation over TLS
//! - **Middleware Pipeline**: Composable request/response middleware
//! - **Rate Limiting**: Per-IP rate limiting with fixed window algorithm
//! - **Access Logging**: Structured JSON logging with tracing
//! - **Static File Serving**: With Brotli compression and cache headers
//!
//! # Architecture
//!
//! The server uses a pluggable executor system for script execution:
//!
//! - `ExtExecutor` - Recommended for production, uses FFI for superglobals
//! - `PhpExecutor` - Legacy executor using zend_eval_string
//! - `StubExecutor` - Returns empty responses for benchmarking
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::server::{Server, ServerConfig};
//! use tokio_php::executor::ExtExecutor;
//!
//! let config = ServerConfig::default();
//! let executor = ExtExecutor::new(4)?;
//! let server = Server::new(config, executor)?;
//! server.run().await?;
//! ```

/// Package version from Cargo.toml
pub const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Git commit hash (8 chars) with optional "-dirty" suffix
pub const BUILD_VERSION: &str = env!("BUILD_VERSION");

/// Full version string: "0.1.0 (abc12345)" or "0.1.0 (abc12345-dirty)"
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("BUILD_VERSION"), ")");

pub mod bridge;
pub mod config;
pub mod core;
pub mod executor;
pub mod listener;
pub mod logging;
pub mod middleware;
pub mod profiler;
pub mod server;
pub mod trace_context;
pub mod types;

// Re-exports for convenience
pub use config::Config;
pub use server::{Server, ServerConfig};
