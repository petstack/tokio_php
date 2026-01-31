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
#[cfg(feature = "grpc")]
mod grpc;
mod logging;
mod middleware;
mod parse;
mod server;

pub use error::ConfigError;
pub use executor::{ExecutorConfig, ExecutorType};
#[cfg(feature = "grpc")]
pub use grpc::GrpcConfig;
pub use logging::LoggingConfig;
pub use middleware::{MiddlewareConfig, RateLimitConfig};
pub use server::{OptionalDuration, RequestTimeout, ServerConfig, SseTimeout, StaticCacheTtl};

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
    /// gRPC server configuration (optional feature).
    #[cfg(feature = "grpc")]
    pub grpc: GrpcConfig,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            server: ServerConfig::from_env()?,
            executor: ExecutorConfig::from_env()?,
            middleware: MiddlewareConfig::from_env()?,
            logging: LoggingConfig::from_env()?,
            #[cfg(feature = "grpc")]
            grpc: GrpcConfig::from_env()?,
        })
    }

    /// Print configuration summary to log.
    pub fn log_summary(&self) {
        use tracing::info;

        info!("Configuration loaded:");
        info!("Listen: {}", self.server.listen_addr);
        info!("Document root: {:?}", self.server.document_root);
        info!("Workers: {}", self.executor.worker_count());
        info!("Queue capacity: {}", self.executor.queue_capacity());
        info!("Executor: {:?}", self.executor.executor_type);

        if let Some(ref index) = self.server.index_file {
            info!("Index file: {}", index);
        }

        if let Some(ref internal) = self.server.internal_addr {
            info!("Internal server: {}", internal);
        }

        if self.server.tls.is_enabled() {
            info!("TLS: enabled");
        }

        if self.server.static_cache_ttl.is_enabled() {
            info!(
                "Static cache TTL: {}s",
                self.server.static_cache_ttl.as_secs()
            );
        } else {
            info!("Static cache: disabled");
        }

        if self.server.request_timeout.is_enabled() {
            info!(
                "Request timeout: {}s",
                self.server.request_timeout.as_secs()
            );
        } else {
            info!("Request timeout: disabled");
        }

        if self.server.sse_timeout.is_enabled() {
            info!("SSE timeout: {}s", self.server.sse_timeout.as_secs());
        } else {
            info!("SSE timeout: disabled");
        }

        if let Some(rl) = self.middleware.rate_limit() {
            info!(
                "Rate limit: {} req/{}s per IP",
                rl.limit(),
                rl.window_secs()
            );
        }

        if self.middleware.is_access_log_enabled() {
            info!("Access log: enabled");
        }

        #[cfg(feature = "grpc")]
        if let Some(addr) = self.grpc.addr {
            info!("gRPC server: {}", addr);
            info!("gRPC TLS: {:?}", self.grpc.tls.mode);
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
        std::env::remove_var("EXECUTOR");
        std::env::remove_var("RATE_LIMIT");
        std::env::remove_var("ACCESS_LOG");

        let config = Config::from_env().expect("Should load config");

        assert_eq!(config.server.listen_addr, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(
            config.server.document_root.to_str().unwrap(),
            "/var/www/html"
        );
        // Workers and queue_capacity are pre-computed (never zero)
        assert!(config.executor.worker_count() >= 1);
        assert!(config.executor.queue_capacity() >= 100);
        // Default executor depends on feature: sapi for tokio-sapi, ext otherwise
        #[cfg(feature = "tokio-sapi")]
        assert_eq!(config.executor.executor_type, ExecutorType::Sapi);
        #[cfg(not(feature = "tokio-sapi"))]
        assert_eq!(config.executor.executor_type, ExecutorType::Ext);
        assert!(config.middleware.rate_limit().is_none());
        assert!(!config.middleware.is_access_log_enabled());
    }
}
