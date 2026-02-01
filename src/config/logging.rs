//! Logging configuration.

use super::parse::env_or;
use super::ConfigError;

/// Logging configuration loaded from environment.
#[derive(Clone, Debug)]
pub struct LoggingConfig {
    /// Log level filter (from LOG_LEVEL or RUST_LOG).
    pub filter: String,
    /// Service name for structured logging.
    pub service_name: String,
}

impl LoggingConfig {
    /// Load configuration from environment variables.
    ///
    /// Priority: LOG_LEVEL > RUST_LOG > default
    ///
    /// LOG_LEVEL accepts simple values: trace, debug, info, warn, error
    /// RUST_LOG accepts full tracing filter syntax: tokio_php=debug,hyper=warn
    pub fn from_env() -> Result<Self, ConfigError> {
        let filter = Self::resolve_log_filter();
        Ok(Self {
            filter,
            service_name: env_or("SERVICE_NAME", "tokio_php"),
        })
    }

    /// Resolve log filter from environment.
    ///
    /// Priority: LOG_LEVEL > RUST_LOG > default (info)
    fn resolve_log_filter() -> String {
        // 1. Check LOG_LEVEL first (simple: debug, info, warn, error)
        if let Ok(level) = std::env::var("LOG_LEVEL") {
            let level = level.to_lowercase();
            match level.as_str() {
                "trace" | "debug" | "info" | "warn" | "error" => {
                    return format!("tokio_php={}", level);
                }
                _ => {
                    // Invalid level, fall through to RUST_LOG
                    eprintln!(
                        "Warning: Invalid LOG_LEVEL '{}', expected: trace, debug, info, warn, error",
                        level
                    );
                }
            }
        }

        // 2. Check RUST_LOG (full tracing filter syntax)
        if let Ok(filter) = std::env::var("RUST_LOG") {
            return filter;
        }

        // 3. Default
        "tokio_php=info".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_log_level_priority() {
        // Clean env
        env::remove_var("LOG_LEVEL");
        env::remove_var("RUST_LOG");

        // Default
        assert_eq!(LoggingConfig::resolve_log_filter(), "tokio_php=info");

        // RUST_LOG
        env::set_var("RUST_LOG", "tokio_php=warn,hyper=debug");
        assert_eq!(
            LoggingConfig::resolve_log_filter(),
            "tokio_php=warn,hyper=debug"
        );

        // LOG_LEVEL takes priority over RUST_LOG
        env::set_var("LOG_LEVEL", "debug");
        assert_eq!(LoggingConfig::resolve_log_filter(), "tokio_php=debug");

        // Cleanup
        env::remove_var("LOG_LEVEL");
        env::remove_var("RUST_LOG");
    }
}
