//! Per-IP rate limiting with fixed window algorithm.

// Note: Global state has been removed. Rate limiter is now configured via
// Server::with_rate_limiter() method using values from config::MiddlewareConfig.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Per-IP request counter for a time window.
#[derive(Debug)]
struct IpCounter {
    count: u64,
    window_start: Instant,
}

/// Fixed-window rate limiter.
pub struct RateLimiter {
    counters: RwLock<HashMap<IpAddr, IpCounter>>,
    limit: u64,
    window: Duration,
}

/// Result of rate limit check.
pub struct RateLimitResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Seconds until window resets.
    pub reset_after: u64,
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

    /// Get the rate limit value.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Get the window duration in seconds.
    pub fn window_secs(&self) -> u64 {
        self.window.as_secs()
    }

    /// Check if a request from the given IP is allowed.
    /// Returns rate limit status with remaining quota and reset time.
    pub fn check(&self, ip: IpAddr) -> RateLimitResult {
        let now = Instant::now();

        // Fast path: read lock to check existing counter
        {
            let counters = self.counters.read().unwrap();
            if let Some(counter) = counters.get(&ip) {
                let elapsed = now.duration_since(counter.window_start);
                if elapsed < self.window {
                    // Window still active
                    if counter.count >= self.limit {
                        let reset_after = (self.window - elapsed).as_secs();
                        return RateLimitResult {
                            allowed: false,
                            reset_after: reset_after.max(1),
                        };
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
            RateLimitResult {
                allowed: true,
                reset_after: self.window.as_secs(),
            }
        } else if counter.count < self.limit {
            // Within limit
            counter.count += 1;
            let reset_after = (self.window - elapsed).as_secs();
            RateLimitResult {
                allowed: true,
                reset_after: reset_after.max(1),
            }
        } else {
            // Limit exceeded
            let reset_after = (self.window - elapsed).as_secs();
            RateLimitResult {
                allowed: false,
                reset_after: reset_after.max(1),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_rate_limit_allows_under_limit() {
        let limiter = RateLimiter::new(10, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        for i in 0..10 {
            let result = limiter.check(ip);
            assert!(result.allowed, "Request {} should be allowed", i);
        }
    }

    #[test]
    fn test_rate_limit_blocks_over_limit() {
        let limiter = RateLimiter::new(5, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // Use up the limit
        for _ in 0..5 {
            let result = limiter.check(ip);
            assert!(result.allowed);
        }

        // Next request should be blocked
        let result = limiter.check(ip);
        assert!(!result.allowed);
    }

    #[test]
    fn test_different_ips_separate_limits() {
        let limiter = RateLimiter::new(2, 60);
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        // IP1 uses its limit
        assert!(limiter.check(ip1).allowed);
        assert!(limiter.check(ip1).allowed);
        assert!(!limiter.check(ip1).allowed);

        // IP2 still has its own limit
        assert!(limiter.check(ip2).allowed);
        assert!(limiter.check(ip2).allowed);
        assert!(!limiter.check(ip2).allowed);
    }
}
