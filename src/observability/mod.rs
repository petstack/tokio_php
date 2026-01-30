//! Observability module for tracing, metrics, and logging.
//!
//! Provides OpenTelemetry integration for distributed tracing
//! and Prometheus metrics export.
//!
//! # Features
//!
//! - **OpenTelemetry Tracing**: Distributed tracing with W3C Trace Context propagation
//! - **Prometheus Metrics**: Comprehensive metrics following RED methodology
//! - **Structured Logging**: Correlation with traces and spans
//!
//! # Usage
//!
//! ## OpenTelemetry (requires `otel` feature)
//!
//! ```rust,ignore
//! use tokio_php::observability::{init_tracing, shutdown_tracing, OtelConfig};
//!
//! let config = OtelConfig::from_env();
//! init_tracing(&config)?;
//!
//! // ... run server ...
//!
//! shutdown_tracing();
//! ```
//!
//! ## Prometheus Metrics
//!
//! ```rust,ignore
//! use tokio_php::observability::Metrics;
//!
//! let metrics = Metrics::new()?;
//! metrics.record_http_request("GET", "/api/users", 200, 0.05, 100, 1500);
//! println!("{}", metrics.export());
//! ```

pub mod metrics;

#[cfg(feature = "otel")]
pub mod otel;

#[cfg(feature = "otel")]
pub mod tracing_middleware;

// Re-exports
pub use metrics::Metrics;

#[cfg(feature = "otel")]
pub use otel::{init_tracing, shutdown_tracing, OtelConfig};
