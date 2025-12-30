//! Logging configuration.

use super::parse::{env_bool, env_or};
use super::ConfigError;

/// Log level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse log level from string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "trace" => Self::Trace,
            "debug" => Self::Debug,
            "info" => Self::Info,
            "warn" | "warning" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

impl Default for LogLevel {
    fn default() -> Self {
        Self::Info
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trace => write!(f, "trace"),
            Self::Debug => write!(f, "debug"),
            Self::Info => write!(f, "info"),
            Self::Warn => write!(f, "warn"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Logging configuration loaded from environment.
#[derive(Clone, Debug)]
pub struct LoggingConfig {
    /// Log level filter (from RUST_LOG).
    pub filter: String,
    /// Service name for structured logging.
    pub service_name: String,
    /// Profiling enabled.
    pub profiling: bool,
}

impl LoggingConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            filter: env_or("RUST_LOG", "tokio_php=info"),
            service_name: env_or("SERVICE_NAME", "tokio_php"),
            profiling: env_bool("PROFILE", false),
        })
    }
}
