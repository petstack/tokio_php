//! Configuration module for tokio_php.
//!
//! This module provides centralized configuration loading from environment variables.
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::config::Config;
//!
//! let config = Config::from_env()?;
//! println!("Listen address: {}", config.server.listen_addr);
//! println!("Workers: {}", config.executor.worker_count());
//! ```

mod error;
mod executor;
mod logging;
mod middleware;
mod parse;
mod server;

pub use error::ConfigError;
pub use executor::{ExecutorConfig, ExecutorType};
pub use logging::LoggingConfig;
pub use middleware::MiddlewareConfig;
pub use server::ServerConfig;

/// Complete application configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Server configuration.
    pub server: ServerConfig,
    /// Executor configuration.
    pub executor: ExecutorConfig,
    /// Middleware configuration.
    pub middleware: MiddlewareConfig,
    /// Logging configuration.
    pub logging: LoggingConfig,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            server: ServerConfig::from_env()?,
            executor: ExecutorConfig::from_env()?,
            middleware: MiddlewareConfig::from_env()?,
            logging: LoggingConfig::from_env()?,
        })
    }

    /// Print configuration summary to log.
    pub fn log_summary(&self) {
        use tracing::info;

        info!("Configuration loaded:");
        info!("  Listen: {}", self.server.listen_addr);
        info!("  Document root: {:?}", self.server.document_root);
        info!("  Workers: {}", self.executor.worker_count());
        info!("  Queue capacity: {}", self.executor.actual_queue_capacity());
        info!("  Executor: {:?}", self.executor.executor_type);

        if let Some(ref index) = self.server.index_file {
            info!("  Index file: {}", index);
        }

        if let Some(ref internal) = self.server.internal_addr {
            info!("  Internal server: {}", internal);
        }

        if self.server.tls.is_enabled() {
            info!("  TLS: enabled");
        }

        if self.server.static_cache_ttl.is_enabled() {
            info!(
                "  Static cache TTL: {}s",
                self.server.static_cache_ttl.as_secs()
            );
        } else {
            info!("  Static cache: disabled");
        }

        if self.server.request_timeout.is_enabled() {
            info!(
                "  Request timeout: {}s",
                self.server.request_timeout.as_secs()
            );
        } else {
            info!("  Request timeout: disabled");
        }

        if let Some(limit) = self.middleware.rate_limit {
            info!(
                "  Rate limit: {} req/{}s per IP",
                limit, self.middleware.rate_window
            );
        }

        if self.middleware.access_log {
            info!("  Access log: enabled");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        // Clear all env vars that might affect the test
        std::env::remove_var("LISTEN_ADDR");
        std::env::remove_var("DOCUMENT_ROOT");
        std::env::remove_var("PHP_WORKERS");
        std::env::remove_var("QUEUE_CAPACITY");
        std::env::remove_var("USE_STUB");
        std::env::remove_var("USE_EXT");
        std::env::remove_var("RATE_LIMIT");
        std::env::remove_var("ACCESS_LOG");

        let config = Config::from_env().expect("Should load config");

        assert_eq!(
            config.server.listen_addr,
            "0.0.0.0:8080".parse().unwrap()
        );
        assert_eq!(
            config.server.document_root.to_str().unwrap(),
            "/var/www/html"
        );
        assert_eq!(config.executor.workers, 0); // Auto-detect
        assert_eq!(config.executor.queue_capacity, 0); // Auto-calculate
        assert_eq!(config.executor.executor_type, ExecutorType::Php);
        assert!(config.middleware.rate_limit.is_none());
        assert!(!config.middleware.access_log);
    }
}
