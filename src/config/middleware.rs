//! Middleware configuration.

use super::parse::{env_bool, env_or};
use super::ConfigError;
use std::num::NonZeroU64;

/// Rate limiting configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Max requests per IP per window (guaranteed non-zero).
    limit: NonZeroU64,
    /// Window size in seconds.
    window_secs: u64,
}

impl RateLimitConfig {
    /// Get max requests per window.
    #[inline]
    pub const fn limit(&self) -> u64 {
        self.limit.get()
    }

    /// Get window size in seconds.
    #[inline]
    pub const fn window_secs(&self) -> u64 {
        self.window_secs
    }
}

/// Middleware configuration loaded from environment.
///
/// All fields are pre-computed for zero-cost access.
#[derive(Clone, Copy, Debug)]
pub struct MiddlewareConfig {
    /// Rate limiting configuration (None if disabled).
    rate_limit: Option<RateLimitConfig>,
    /// Access logging enabled.
    access_log: bool,
}

impl MiddlewareConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            rate_limit: Self::parse_rate_limit()?,
            access_log: env_bool("ACCESS_LOG", false),
        })
    }

    /// Get rate limit config if enabled.
    #[inline]
    pub const fn rate_limit(&self) -> Option<RateLimitConfig> {
        self.rate_limit
    }

    /// Check if rate limiting is enabled.
    #[inline]
    pub const fn is_rate_limiting_enabled(&self) -> bool {
        self.rate_limit.is_some()
    }

    /// Check if access logging is enabled.
    #[inline]
    pub const fn is_access_log_enabled(&self) -> bool {
        self.access_log
    }

    /// Check if profiling is enabled.
    ///
    /// With `debug-profile` feature: always true.
    /// Without: always false (profiling code is compiled out).
    #[inline]
    pub const fn is_profile_enabled(&self) -> bool {
        cfg!(feature = "debug-profile")
    }

    fn parse_rate_limit() -> Result<Option<RateLimitConfig>, ConfigError> {
        let raw_limit = env_or("RATE_LIMIT", "0");
        let limit: u64 = raw_limit.parse().map_err(|e| ConfigError::Parse {
            key: "RATE_LIMIT".into(),
            value: raw_limit,
            error: format!("{e}"),
        })?;

        // If limit is 0, rate limiting is disabled
        let Some(limit) = NonZeroU64::new(limit) else {
            return Ok(None);
        };

        let raw_window = env_or("RATE_WINDOW", "60");
        let window_secs: u64 = raw_window.parse().map_err(|e| ConfigError::Parse {
            key: "RATE_WINDOW".into(),
            value: raw_window,
            error: format!("{e}"),
        })?;

        Ok(Some(RateLimitConfig { limit, window_secs }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiting_disabled_when_zero() {
        let config = MiddlewareConfig {
            rate_limit: None,
            access_log: false,
        };
        assert!(!config.is_rate_limiting_enabled());
        assert!(config.rate_limit().is_none());
    }

    #[test]
    fn test_rate_limiting_enabled_when_set() {
        let config = MiddlewareConfig {
            rate_limit: Some(RateLimitConfig {
                limit: NonZeroU64::new(100).unwrap(),
                window_secs: 60,
            }),
            access_log: false,
        };
        assert!(config.is_rate_limiting_enabled());
        let rl = config.rate_limit().unwrap();
        assert_eq!(rl.limit(), 100);
        assert_eq!(rl.window_secs(), 60);
    }

    #[test]
    fn test_rate_limit_config_values() {
        let rl = RateLimitConfig {
            limit: NonZeroU64::new(500).unwrap(),
            window_secs: 120,
        };
        assert_eq!(rl.limit(), 500);
        assert_eq!(rl.window_secs(), 120);
    }

    #[test]
    fn test_access_log_flag() {
        let config = MiddlewareConfig {
            rate_limit: None,
            access_log: true,
        };
        assert!(config.is_access_log_enabled());
    }

    #[test]
    fn test_profile_enabled_depends_on_feature() {
        let config = MiddlewareConfig {
            rate_limit: None,
            access_log: false,
        };
        // With debug-profile feature: true, without: false
        assert_eq!(config.is_profile_enabled(), cfg!(feature = "debug-profile"));
    }

    #[test]
    fn test_middleware_config_is_copy() {
        let config = MiddlewareConfig {
            rate_limit: None,
            access_log: true,
        };
        let copy = config; // Copy
        assert!(copy.is_access_log_enabled());
        assert!(config.is_access_log_enabled()); // Original still valid
    }
}
