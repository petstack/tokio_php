//! Logging configuration.

use super::parse::env_or;
use super::ConfigError;

/// Logging configuration loaded from environment.
#[derive(Clone, Debug)]
pub struct LoggingConfig {
    /// Log level filter (from RUST_LOG).
    pub filter: String,
    /// Service name for structured logging.
    pub service_name: String,
}

impl LoggingConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            filter: env_or("RUST_LOG", "tokio_php=info"),
            service_name: env_or("SERVICE_NAME", "tokio_php"),
        })
    }
}

