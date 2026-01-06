//! Core types for HTTP request/response handling.
//!
//! This module provides the fundamental types used throughout the middleware
//! pipeline and request handlers:
//!
//! - [`Request`] - HTTP request abstraction
//! - [`Response`] - HTTP response abstraction with builder pattern
//! - [`Context`] - Request context for middleware communication
//! - [`Error`] - Core error types
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::core::{Request, Response, Context};
//!
//! fn handle_request(req: &Request, ctx: &mut Context) -> Response {
//!     // Set response header via context
//!     ctx.set_response_header("X-Custom", "value");
//!
//!     // Return response
//!     Response::ok("Hello, World!")
//! }
//! ```

mod context;
mod error;
mod request;
mod response;

pub use context::{generate_span_id, generate_trace_id, Context, ContextBuilder, HttpVersion};
pub use error::{Error, Result};
pub use request::Request;
pub use response::{Response, ResponseBuilder};
