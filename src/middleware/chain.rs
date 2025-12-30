//! Middleware chain for composing multiple middleware.

use std::sync::Arc;

use super::{Middleware, MiddlewareResult};
use crate::core::{Context, Request, Response};

/// A chain of middleware that processes requests and responses in order.
///
/// Middleware are executed in priority order for requests (lowest first)
/// and in reverse order for responses.
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    /// Create a new empty middleware chain.
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Add a middleware to the chain.
    ///
    /// Middleware are automatically sorted by priority.
    pub fn add<M: Middleware + 'static>(mut self, middleware: M) -> Self {
        self.middlewares.push(Arc::new(middleware));
        self.middlewares.sort_by_key(|m| m.priority());
        self
    }

    /// Add a middleware wrapped in Arc to the chain.
    pub fn add_arc(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(middleware);
        self.middlewares.sort_by_key(|m| m.priority());
        self
    }

    /// Get the number of middleware in the chain.
    pub fn len(&self) -> usize {
        self.middlewares.len()
    }

    /// Check if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.middlewares.is_empty()
    }

    /// Get middleware names in execution order.
    pub fn names(&self) -> Vec<&'static str> {
        self.middlewares.iter().map(|m| m.name()).collect()
    }

    /// Process a request through all middleware.
    ///
    /// Returns `MiddlewareResult::Next(req)` if all middleware passed,
    /// or `MiddlewareResult::Stop(res)` if any middleware short-circuited.
    pub fn process_request(&self, mut req: Request, ctx: &mut Context) -> MiddlewareResult {
        for mw in &self.middlewares {
            match mw.on_request(req, ctx) {
                MiddlewareResult::Next(r) => req = r,
                MiddlewareResult::Stop(res) => {
                    tracing::debug!(
                        middleware = mw.name(),
                        status = %res.status(),
                        "middleware short-circuited request"
                    );
                    return MiddlewareResult::Stop(res);
                }
            }
        }
        MiddlewareResult::Next(req)
    }

    /// Process a response through all middleware in reverse order.
    pub fn process_response(&self, mut res: Response, ctx: &Context) -> Response {
        for mw in self.middlewares.iter().rev() {
            res = mw.on_response(res, ctx);
        }
        res
    }

    /// Process a complete request/response cycle.
    ///
    /// This is a convenience method that:
    /// 1. Runs `process_request`
    /// 2. If not short-circuited, calls the handler
    /// 3. Runs `process_response` on the result
    pub fn process<F>(&self, req: Request, ctx: &mut Context, handler: F) -> Response
    where
        F: FnOnce(Request, &mut Context) -> Response,
    {
        // Process request through middleware
        let req = match self.process_request(req, ctx) {
            MiddlewareResult::Next(req) => req,
            MiddlewareResult::Stop(res) => {
                // Still run response middleware on short-circuit
                return self.process_response(res, ctx);
            }
        };

        // Call the handler
        let res = handler(req, ctx);

        // Process response through middleware (reverse order)
        self.process_response(res, ctx)
    }

    /// Async version of process for use with async handlers.
    pub async fn process_async<F, Fut>(
        &self,
        req: Request,
        ctx: &mut Context,
        handler: F,
    ) -> Response
    where
        F: FnOnce(Request, &mut Context) -> Fut,
        Fut: std::future::Future<Output = Response>,
    {
        // Process request through middleware
        let req = match self.process_request(req, ctx) {
            MiddlewareResult::Next(req) => req,
            MiddlewareResult::Stop(res) => {
                return self.process_response(res, ctx);
            }
        };

        // Call the async handler
        let res = handler(req, ctx).await;

        // Process response through middleware
        self.process_response(res, ctx)
    }
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MiddlewareChain {
    fn clone(&self) -> Self {
        Self {
            middlewares: self.middlewares.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingMiddleware {
        name: &'static str,
        priority: i32,
        request_count: AtomicU32,
        response_count: AtomicU32,
    }

    impl CountingMiddleware {
        fn new(name: &'static str, priority: i32) -> Self {
            Self {
                name,
                priority,
                request_count: AtomicU32::new(0),
                response_count: AtomicU32::new(0),
            }
        }
    }

    impl Middleware for CountingMiddleware {
        fn name(&self) -> &'static str {
            self.name
        }

        fn priority(&self) -> i32 {
            self.priority
        }

        fn on_request(&self, req: Request, _ctx: &mut Context) -> MiddlewareResult {
            self.request_count.fetch_add(1, Ordering::SeqCst);
            MiddlewareResult::Next(req)
        }

        fn on_response(&self, res: Response, _ctx: &Context) -> Response {
            self.response_count.fetch_add(1, Ordering::SeqCst);
            res
        }
    }

    struct BlockingMiddleware;

    impl Middleware for BlockingMiddleware {
        fn name(&self) -> &'static str {
            "blocker"
        }

        fn priority(&self) -> i32 {
            -50 // High priority to block early
        }

        fn on_request(&self, _req: Request, _ctx: &mut Context) -> MiddlewareResult {
            MiddlewareResult::Stop(Response::too_many_requests(60))
        }
    }

    fn create_test_context() -> Context {
        Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace".to_string(),
            "span".to_string(),
        )
    }

    fn create_test_request() -> Request {
        Request::new(
            http::Method::GET,
            "/test".parse().unwrap(),
            http::HeaderMap::new(),
            bytes::Bytes::new(),
        )
    }

    #[test]
    fn test_empty_chain() {
        let chain = MiddlewareChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn test_add_middleware() {
        let chain = MiddlewareChain::new()
            .add(CountingMiddleware::new("first", 0))
            .add(CountingMiddleware::new("second", 10));

        assert_eq!(chain.len(), 2);
        assert!(!chain.is_empty());
    }

    #[test]
    fn test_priority_ordering() {
        let chain = MiddlewareChain::new()
            .add(CountingMiddleware::new("low", 100))
            .add(CountingMiddleware::new("high", -100))
            .add(CountingMiddleware::new("medium", 0));

        let names = chain.names();
        assert_eq!(names, vec!["high", "medium", "low"]);
    }

    #[test]
    fn test_process_request_pass_through() {
        let chain = MiddlewareChain::new()
            .add(CountingMiddleware::new("first", 0))
            .add(CountingMiddleware::new("second", 10));

        let req = create_test_request();
        let mut ctx = create_test_context();

        let result = chain.process_request(req, &mut ctx);
        assert!(result.is_next());
    }

    #[test]
    fn test_process_request_short_circuit() {
        let chain = MiddlewareChain::new()
            .add(CountingMiddleware::new("after", 0))
            .add(BlockingMiddleware);

        let req = create_test_request();
        let mut ctx = create_test_context();

        let result = chain.process_request(req, &mut ctx);
        assert!(result.is_stop());

        if let MiddlewareResult::Stop(res) = result {
            assert_eq!(res.status(), http::StatusCode::TOO_MANY_REQUESTS);
        }
    }

    #[test]
    fn test_process_response() {
        struct HeaderMiddleware {
            header_name: &'static str,
            header_value: &'static str,
        }

        impl Middleware for HeaderMiddleware {
            fn name(&self) -> &'static str {
                "header"
            }

            fn on_response(&self, res: Response, _ctx: &Context) -> Response {
                res.with_header(self.header_name, self.header_value)
            }
        }

        let chain = MiddlewareChain::new()
            .add(HeaderMiddleware {
                header_name: "x-first",
                header_value: "1",
            })
            .add(HeaderMiddleware {
                header_name: "x-second",
                header_value: "2",
            });

        let res = Response::ok("test");
        let ctx = create_test_context();

        let res = chain.process_response(res, &ctx);
        assert_eq!(res.header("x-first"), Some("1"));
        assert_eq!(res.header("x-second"), Some("2"));
    }

    #[test]
    fn test_process_full_cycle() {
        let chain = MiddlewareChain::new().add(CountingMiddleware::new("counter", 0));

        let req = create_test_request();
        let mut ctx = create_test_context();

        let res = chain.process(req, &mut ctx, |_req, _ctx| Response::ok("handled"));

        assert_eq!(res.status(), http::StatusCode::OK);
        assert_eq!(res.body().as_ref(), b"handled");
    }

    #[test]
    fn test_clone() {
        let chain = MiddlewareChain::new().add(CountingMiddleware::new("test", 0));

        let cloned = chain.clone();
        assert_eq!(chain.len(), cloned.len());
        assert_eq!(chain.names(), cloned.names());
    }
}
