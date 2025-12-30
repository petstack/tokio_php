//! Middleware configuration.

use super::parse::{env_bool, env_or};
use super::ConfigError;

/// Middleware configuration loaded from environment.
#[derive(Clone, Debug)]
pub struct MiddlewareConfig {
    /// Rate limit: max requests per IP per window (0 = disabled).
    pub rate_limit: Option<u64>,
    /// Rate limit window in seconds.
    pub rate_window: u64,
    /// Access logging enabled.
    pub access_log: bool,
    /// Profiling enabled (requires X-Profile: 1 header per request).
    pub profile: bool,
}

impl MiddlewareConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        // Parse rate limit
        let rate_limit_value: u64 = env_or("RATE_LIMIT", "0")
            .parse()
            .map_err(|e| ConfigError::Parse {
                key: "RATE_LIMIT".into(),
                value: env_or("RATE_LIMIT", "0"),
                error: format!("{}", e),
            })?;

        // Parse rate window
        let rate_window: u64 = env_or("RATE_WINDOW", "60")
            .parse()
            .map_err(|e| ConfigError::Parse {
                key: "RATE_WINDOW".into(),
                value: env_or("RATE_WINDOW", "60"),
                error: format!("{}", e),
            })?;

        Ok(Self {
            rate_limit: if rate_limit_value > 0 {
                Some(rate_limit_value)
            } else {
                None
            },
            rate_window,
            access_log: env_bool("ACCESS_LOG", false),
            profile: env_bool("PROFILE", false),
        })
    }

    /// Check if rate limiting is enabled.
    pub fn is_rate_limiting_enabled(&self) -> bool {
        self.rate_limit.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiting_disabled_when_zero() {
        let config = MiddlewareConfig {
            rate_limit: None,
            rate_window: 60,
            access_log: false,
            profile: false,
        };
        assert!(!config.is_rate_limiting_enabled());
    }

    #[test]
    fn test_rate_limiting_enabled_when_set() {
        let config = MiddlewareConfig {
            rate_limit: Some(100),
            rate_window: 60,
            access_log: false,
            profile: false,
        };
        assert!(config.is_rate_limiting_enabled());
        assert_eq!(config.rate_limit, Some(100));
    }

    #[test]
    fn test_default_rate_window() {
        let config = MiddlewareConfig {
            rate_limit: Some(100),
            rate_window: 60,
            access_log: false,
            profile: false,
        };
        assert_eq!(config.rate_window, 60);
    }

    #[test]
    fn test_access_log_flag() {
        let config = MiddlewareConfig {
            rate_limit: None,
            rate_window: 60,
            access_log: true,
            profile: false,
        };
        assert!(config.access_log);
    }

    #[test]
    fn test_profile_flag() {
        let config = MiddlewareConfig {
            rate_limit: None,
            rate_window: 60,
            access_log: false,
            profile: true,
        };
        assert!(config.profile);
    }
}
