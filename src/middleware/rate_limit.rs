//! Rate limiting middleware.
//!
//! Per-IP rate limiting using a fixed window algorithm.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::config::MiddlewareConfig;
use crate::core::{Context, Request, Response};

use super::{Middleware, MiddlewareResult};

/// Per-IP request counter for a time window.
#[derive(Debug)]
struct IpCounter {
    count: u64,
    window_start: Instant,
}

/// Rate limiter state.
pub struct RateLimiter {
    counters: RwLock<HashMap<IpAddr, IpCounter>>,
    limit: u64,
    window: Duration,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(limit: u64, window_secs: u64) -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            limit,
            window: Duration::from_secs(window_secs),
        }
    }

    /// Check if a request from the given IP is allowed.
    /// Returns (allowed, remaining, reset_after_secs).
    pub fn check(&self, ip: IpAddr) -> (bool, u64, u64) {
        let now = Instant::now();

        // Fast path: read lock to check existing counter
        {
            let counters = self.counters.read().unwrap();
            if let Some(counter) = counters.get(&ip) {
                let elapsed = now.duration_since(counter.window_start);
                if elapsed < self.window {
                    if counter.count >= self.limit {
                        let reset_after = (self.window - elapsed).as_secs().max(1);
                        return (false, 0, reset_after);
                    }
                }
            }
        }

        // Slow path: write lock to update counter
        let mut counters = self.counters.write().unwrap();
        let counter = counters.entry(ip).or_insert(IpCounter {
            count: 0,
            window_start: now,
        });

        let elapsed = now.duration_since(counter.window_start);
        if elapsed >= self.window {
            // Window expired, reset
            counter.count = 1;
            counter.window_start = now;
            (true, self.limit - 1, self.window.as_secs())
        } else if counter.count < self.limit {
            // Within limit
            counter.count += 1;
            let remaining = self.limit - counter.count;
            let reset_after = (self.window - elapsed).as_secs().max(1);
            (true, remaining, reset_after)
        } else {
            // Limit exceeded
            let reset_after = (self.window - elapsed).as_secs().max(1);
            (false, 0, reset_after)
        }
    }
}

/// Rate limiting middleware.
///
/// Limits requests per IP address using a fixed window algorithm.
/// Returns 429 Too Many Requests when limit is exceeded.
pub struct RateLimitMiddleware {
    limiter: RateLimiter,
    limit: u64,
}

impl RateLimitMiddleware {
    /// Create a new rate limit middleware.
    pub fn new(limit: u64, window_secs: u64) -> Self {
        Self {
            limiter: RateLimiter::new(limit, window_secs),
            limit,
        }
    }

    /// Create from middleware configuration.
    /// Returns None if rate limiting is not configured.
    pub fn from_config(config: &MiddlewareConfig) -> Option<Self> {
        config.rate_limit.map(|limit| Self::new(limit, config.rate_window))
    }
}

impl Middleware for RateLimitMiddleware {
    fn name(&self) -> &'static str {
        "rate_limit"
    }

    fn priority(&self) -> i32 {
        -100 // Run very early to reject requests before expensive processing
    }

    fn on_request(&self, req: Request, ctx: &mut Context) -> MiddlewareResult {
        let (allowed, remaining, reset) = self.limiter.check(ctx.client_ip);

        // Always set rate limit headers
        ctx.set_response_header("X-RateLimit-Limit", self.limit);
        ctx.set_response_header("X-RateLimit-Remaining", remaining);
        ctx.set_response_header("X-RateLimit-Reset", reset);

        if allowed {
            MiddlewareResult::Next(req)
        } else {
            tracing::debug!(
                ip = %ctx.client_ip,
                limit = self.limit,
                reset = reset,
                "rate limit exceeded"
            );

            let res = Response::builder()
                .status(http::StatusCode::TOO_MANY_REQUESTS)
                .header("Retry-After", reset.to_string())
                .header("X-RateLimit-Limit", self.limit.to_string())
                .header("X-RateLimit-Remaining", "0")
                .header("X-RateLimit-Reset", reset.to_string())
                .body("Too Many Requests")
                .build();

            MiddlewareResult::Stop(res)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn create_context(ip: IpAddr) -> Context {
        Context::new(ip, "trace".to_string(), "span".to_string())
    }

    fn create_request() -> Request {
        Request::new(
            http::Method::GET,
            "/test".parse().unwrap(),
            http::HeaderMap::new(),
            bytes::Bytes::new(),
        )
    }

    #[test]
    fn test_allows_under_limit() {
        let mw = RateLimitMiddleware::new(5, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        for i in 0..5 {
            let req = create_request();
            let mut ctx = create_context(ip);
            let result = mw.on_request(req, &mut ctx);
            assert!(result.is_next(), "Request {} should be allowed", i);
        }
    }

    #[test]
    fn test_blocks_over_limit() {
        let mw = RateLimitMiddleware::new(3, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // Use up the limit
        for _ in 0..3 {
            let req = create_request();
            let mut ctx = create_context(ip);
            let result = mw.on_request(req, &mut ctx);
            assert!(result.is_next());
        }

        // Next request should be blocked
        let req = create_request();
        let mut ctx = create_context(ip);
        let result = mw.on_request(req, &mut ctx);
        assert!(result.is_stop());

        if let MiddlewareResult::Stop(res) = result {
            assert_eq!(res.status(), http::StatusCode::TOO_MANY_REQUESTS);
            assert!(res.header("retry-after").is_some());
        }
    }

    #[test]
    fn test_different_ips_separate_limits() {
        let mw = RateLimitMiddleware::new(2, 60);
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        // IP1 uses its limit
        for _ in 0..2 {
            let req = create_request();
            let mut ctx = create_context(ip1);
            assert!(mw.on_request(req, &mut ctx).is_next());
        }
        let req = create_request();
        let mut ctx = create_context(ip1);
        assert!(mw.on_request(req, &mut ctx).is_stop());

        // IP2 still has its own limit
        for _ in 0..2 {
            let req = create_request();
            let mut ctx = create_context(ip2);
            assert!(mw.on_request(req, &mut ctx).is_next());
        }
    }

    #[test]
    fn test_sets_rate_limit_headers() {
        let mw = RateLimitMiddleware::new(10, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        let req = create_request();
        let mut ctx = create_context(ip);
        mw.on_request(req, &mut ctx);

        let headers = ctx.response_headers();
        assert_eq!(headers.get("X-RateLimit-Limit"), Some(&"10".to_string()));
        assert!(headers.contains_key("X-RateLimit-Remaining"));
        assert!(headers.contains_key("X-RateLimit-Reset"));
    }

    #[test]
    fn test_from_config() {
        let config = MiddlewareConfig {
            rate_limit: Some(100),
            rate_window: 120,
            access_log: false,
            profile: false,
        };

        let mw = RateLimitMiddleware::from_config(&config);
        assert!(mw.is_some());

        let config_disabled = MiddlewareConfig {
            rate_limit: None,
            rate_window: 60,
            access_log: false,
            profile: false,
        };

        let mw = RateLimitMiddleware::from_config(&config_disabled);
        assert!(mw.is_none());
    }
}
