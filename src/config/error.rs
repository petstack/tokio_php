//! Configuration error types.

use std::fmt;

/// Error type for configuration loading.
#[derive(Debug)]
pub enum ConfigError {
    /// Failed to parse environment variable.
    Parse {
        key: String,
        value: String,
        error: String,
    },
    /// Missing required environment variable.
    Missing { key: String },
    /// Invalid value for environment variable.
    Invalid { key: String, message: String },
    /// IO error (e.g., reading TLS certificates).
    Io { path: String, error: std::io::Error },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Parse { key, value, error } => {
                write!(f, "failed to parse {}='{}': {}", key, value, error)
            }
            ConfigError::Missing { key } => {
                write!(f, "missing required environment variable: {}", key)
            }
            ConfigError::Invalid { key, message } => {
                write!(f, "invalid value for {}: {}", key, message)
            }
            ConfigError::Io { path, error } => {
                write!(f, "IO error for '{}': {}", path, error)
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io { error, .. } => Some(error),
            _ => None,
        }
    }
}
