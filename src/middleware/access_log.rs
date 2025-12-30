//! Access logging middleware.
//!
//! Logs HTTP requests in a structured format for analysis.

use crate::core::{Context, Request, Response};

use super::{Middleware, MiddlewareResult};

/// Access log middleware configuration.
#[derive(Clone, Debug)]
pub struct AccessLogConfig {
    /// Whether logging is enabled.
    pub enabled: bool,
    /// Include request body size.
    pub include_request_body: bool,
    /// Include response body size.
    pub include_response_body: bool,
}

impl Default for AccessLogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_request_body: false,
            include_response_body: true,
        }
    }
}

/// Access logging middleware.
///
/// Logs requests using the `tracing` framework with structured data.
/// Log entries are emitted at INFO level with target "access".
pub struct AccessLogMiddleware {
    config: AccessLogConfig,
}

impl AccessLogMiddleware {
    /// Create a new access log middleware.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from configuration.
    pub fn from_config(config: AccessLogConfig) -> Self {
        Self { config }
    }

    /// Create with enabled/disabled state.
    pub fn with_enabled(enabled: bool) -> Self {
        Self {
            config: AccessLogConfig {
                enabled,
                ..Default::default()
            },
        }
    }
}

impl Default for AccessLogMiddleware {
    fn default() -> Self {
        Self {
            config: AccessLogConfig::default(),
        }
    }
}

impl Middleware for AccessLogMiddleware {
    fn name(&self) -> &'static str {
        "access_log"
    }

    fn priority(&self) -> i32 {
        -90 // Run early for requests, late for responses
    }

    fn on_request(&self, req: Request, ctx: &mut Context) -> MiddlewareResult {
        // Store request info in context for logging in on_response
        ctx.set("log_method", req.method().to_string());
        ctx.set("log_path", req.path().to_string());
        if let Some(query) = req.query() {
            ctx.set("log_query", query.to_string());
        }
        if let Some(ua) = req.user_agent() {
            ctx.set("log_ua", ua.to_string());
        }
        if let Some(referer) = req.header("referer") {
            ctx.set("log_referer", referer.to_string());
        }
        if let Some(xff) = req.header("x-forwarded-for") {
            ctx.set("log_xff", xff.to_string());
        }

        MiddlewareResult::Next(req)
    }

    fn on_response(&self, res: Response, ctx: &Context) -> Response {
        if !self.config.enabled {
            return res;
        }

        let method = ctx.get::<String>("log_method").map(|s| s.as_str()).unwrap_or("?");
        let path = ctx.get::<String>("log_path").map(|s| s.as_str()).unwrap_or("?");
        let query = ctx.get::<String>("log_query");
        let ua = ctx.get::<String>("log_ua");
        let referer = ctx.get::<String>("log_referer");
        let xff = ctx.get::<String>("log_xff");

        let status = res.status().as_u16();
        let body_bytes = res.body_len() as u64;
        let duration_ms = ctx.elapsed_ms();

        // Use tracing's structured logging
        tracing::info!(
            target: "access",
            method = method,
            path = path,
            query = query.map(|s| s.as_str()),
            status = status,
            bytes = body_bytes,
            duration_ms = duration_ms,
            ip = %ctx.client_ip,
            ua = ua.map(|s| s.as_str()),
            referer = referer.map(|s| s.as_str()),
            xff = xff.map(|s| s.as_str()),
            request_id = %ctx.request_id,
            trace_id = %ctx.trace_id,
            span_id = %ctx.span_id,
            http = %ctx.http_version,
            "{} {} {}",
            method,
            path,
            status
        );

        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn create_context() -> Context {
        Context::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "trace123".to_string(),
            "span456".to_string(),
        )
    }

    fn create_request() -> Request {
        let mut headers = http::HeaderMap::new();
        headers.insert("user-agent", "test/1.0".parse().unwrap());

        Request::new(
            http::Method::GET,
            "/test?foo=bar".parse().unwrap(),
            headers,
            bytes::Bytes::new(),
        )
    }

    #[test]
    fn test_stores_request_info() {
        let mw = AccessLogMiddleware::new();
        let req = create_request();
        let mut ctx = create_context();

        mw.on_request(req, &mut ctx);

        assert_eq!(ctx.get::<String>("log_method"), Some(&"GET".to_string()));
        assert_eq!(ctx.get::<String>("log_path"), Some(&"/test".to_string()));
        assert_eq!(ctx.get::<String>("log_query"), Some(&"foo=bar".to_string()));
        assert_eq!(ctx.get::<String>("log_ua"), Some(&"test/1.0".to_string()));
    }

    #[test]
    fn test_disabled_does_not_log() {
        let mw = AccessLogMiddleware::with_enabled(false);
        let req = create_request();
        let mut ctx = create_context();

        mw.on_request(req, &mut ctx);
        let res = Response::ok("test");

        // This should not panic even with disabled logging
        let res = mw.on_response(res, &ctx);
        assert_eq!(res.status(), http::StatusCode::OK);
    }

    #[test]
    fn test_from_config() {
        let config = AccessLogConfig {
            enabled: true,
            include_request_body: true,
            include_response_body: true,
        };

        let mw = AccessLogMiddleware::from_config(config);
        assert!(mw.config.enabled);
        assert!(mw.config.include_request_body);
    }
}
