//! Middleware pipeline for request/response processing.
//!
//! This module provides a composable middleware system for handling HTTP requests
//! and responses. Middleware can:
//! - Inspect and modify incoming requests
//! - Short-circuit the pipeline and return early responses
//! - Modify outgoing responses
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::middleware::{Middleware, MiddlewareResult, MiddlewareChain};
//! use tokio_php::core::{Request, Response, Context};
//!
//! struct LoggingMiddleware;
//!
//! impl Middleware for LoggingMiddleware {
//!     fn name(&self) -> &'static str { "logging" }
//!
//!     fn on_request(&self, req: Request, ctx: &mut Context) -> MiddlewareResult {
//!         println!("Request: {} {}", req.method(), req.path());
//!         MiddlewareResult::Next(req)
//!     }
//!
//!     fn on_response(&self, res: Response, ctx: &Context) -> Response {
//!         println!("Response: {}", res.status());
//!         res
//!     }
//! }
//!
//! let chain = MiddlewareChain::new()
//!     .with(LoggingMiddleware);
//! ```

mod chain;

pub mod access_log;
pub mod compression;
pub mod error_pages;
pub mod rate_limit;
pub mod static_cache;

pub use chain::MiddlewareChain;

use crate::core::{Context, Request, Response};

/// Result of middleware request processing.
#[derive(Debug)]
pub enum MiddlewareResult {
    /// Continue to the next middleware with the (possibly modified) request.
    Next(Request),
    /// Stop the middleware chain and return this response immediately.
    Stop(Response),
}

impl MiddlewareResult {
    /// Check if this result continues the chain.
    pub fn is_next(&self) -> bool {
        matches!(self, MiddlewareResult::Next(_))
    }

    /// Check if this result stops the chain.
    pub fn is_stop(&self) -> bool {
        matches!(self, MiddlewareResult::Stop(_))
    }

    /// Unwrap the request if this is a Next result.
    pub fn into_request(self) -> Option<Request> {
        match self {
            MiddlewareResult::Next(req) => Some(req),
            MiddlewareResult::Stop(_) => None,
        }
    }

    /// Unwrap the response if this is a Stop result.
    pub fn into_response(self) -> Option<Response> {
        match self {
            MiddlewareResult::Next(_) => None,
            MiddlewareResult::Stop(res) => Some(res),
        }
    }
}

/// Trait for implementing middleware.
///
/// Middleware processes requests before they reach the handler and responses
/// after the handler returns. The pipeline executes `on_request` in order
/// and `on_response` in reverse order.
///
/// # Lifecycle
///
/// ```text
/// Request → MW1.on_request → MW2.on_request → Handler
///                                                ↓
/// Response ← MW1.on_response ← MW2.on_response ←─┘
/// ```
///
/// # Implementation Notes
///
/// - `on_request` can modify the request or short-circuit with a response
/// - `on_response` can modify the response (e.g., add headers, compress)
/// - Context allows middleware to communicate (e.g., set values for later use)
pub trait Middleware: Send + Sync {
    /// Unique name for this middleware (used for logging/debugging).
    fn name(&self) -> &'static str;

    /// Priority for ordering in the chain.
    /// Lower values execute first for requests, last for responses.
    /// Default is 0.
    ///
    /// Suggested priority ranges:
    /// - -100..-50: Security (rate limiting, auth)
    /// - -50..0: Logging, tracing
    /// - 0..50: Request modification
    /// - 50..100: Response modification (compression, error pages)
    fn priority(&self) -> i32 {
        0
    }

    /// Process an incoming request.
    ///
    /// Return `MiddlewareResult::Next(req)` to continue the chain,
    /// or `MiddlewareResult::Stop(res)` to short-circuit with a response.
    fn on_request(&self, req: Request, _ctx: &mut Context) -> MiddlewareResult {
        MiddlewareResult::Next(req)
    }

    /// Process an outgoing response.
    ///
    /// Called in reverse order after the handler returns.
    fn on_response(&self, res: Response, _ctx: &Context) -> Response {
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    struct TestMiddleware {
        name: &'static str,
        priority: i32,
    }

    impl Middleware for TestMiddleware {
        fn name(&self) -> &'static str {
            self.name
        }

        fn priority(&self) -> i32 {
            self.priority
        }
    }

    #[test]
    fn test_middleware_result_next() {
        let req = Request::new(
            http::Method::GET,
            "/test".parse().unwrap(),
            http::HeaderMap::new(),
            bytes::Bytes::new(),
        );
        let result = MiddlewareResult::Next(req);

        assert!(result.is_next());
        assert!(!result.is_stop());
    }

    #[test]
    fn test_middleware_result_stop() {
        let res = Response::ok("stopped");
        let result = MiddlewareResult::Stop(res);

        assert!(!result.is_next());
        assert!(result.is_stop());
    }

    #[test]
    fn test_middleware_default_implementations() {
        let mw = TestMiddleware {
            name: "test",
            priority: 0,
        };

        assert_eq!(mw.name(), "test");
        assert_eq!(mw.priority(), 0);

        let req = Request::new(
            http::Method::GET,
            "/test".parse().unwrap(),
            http::HeaderMap::new(),
            bytes::Bytes::new(),
        );
        let mut ctx = Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        );

        // Default on_request passes through
        let result = mw.on_request(req, &mut ctx);
        assert!(result.is_next());

        // Default on_response passes through
        let res = Response::ok("test");
        let res = mw.on_response(res, &ctx);
        assert_eq!(res.status(), http::StatusCode::OK);
    }
}
