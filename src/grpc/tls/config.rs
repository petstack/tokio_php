//! gRPC TLS configuration.

use std::path::PathBuf;

/// TLS mode for gRPC server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GrpcTlsMode {
    /// Plaintext gRPC (no encryption) - default for development
    #[default]
    Off,
    /// Auto-generated self-signed certificates - for development/testing
    Auto,
    /// External certificates - for production
    On,
}

impl GrpcTlsMode {
    /// Parse from environment variable value.
    pub fn from_env(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "auto" => Self::Auto,
            "on" | "true" | "1" => Self::On,
            _ => Self::Off,
        }
    }
}

/// gRPC TLS configuration.
#[derive(Debug, Clone)]
pub struct GrpcTlsConfig {
    /// TLS mode
    pub mode: GrpcTlsMode,

    /// Certificate file path (required for mode=On)
    pub cert_path: Option<PathBuf>,

    /// Private key file path (required for mode=On)
    pub key_path: Option<PathBuf>,

    /// CA certificate for mTLS client verification (optional)
    pub ca_path: Option<PathBuf>,

    /// Auto-generated cert validity in days (default: 365)
    pub auto_cert_days: u32,

    /// Auto-generated cert Common Name (default: "localhost")
    pub auto_cert_cn: String,

    /// Directory for auto-generated certificates
    pub auto_cert_dir: PathBuf,
}

impl Default for GrpcTlsConfig {
    fn default() -> Self {
        Self {
            mode: GrpcTlsMode::Off,
            cert_path: None,
            key_path: None,
            ca_path: None,
            auto_cert_days: 365,
            auto_cert_cn: "localhost".to_string(),
            auto_cert_dir: PathBuf::from("/tmp/tokio_php"),
        }
    }
}

impl GrpcTlsConfig {
    /// Create a new TLS configuration.
    pub fn new(
        mode: GrpcTlsMode,
        cert_path: Option<impl Into<PathBuf>>,
        key_path: Option<impl Into<PathBuf>>,
        ca_path: Option<impl Into<PathBuf>>,
    ) -> Self {
        Self {
            mode,
            cert_path: cert_path.map(Into::into),
            key_path: key_path.map(Into::into),
            ca_path: ca_path.map(Into::into),
            ..Default::default()
        }
    }

    /// Create auto-generated TLS configuration.
    pub fn auto(cn: impl Into<String>, validity_days: u32) -> Self {
        Self {
            mode: GrpcTlsMode::Auto,
            auto_cert_cn: cn.into(),
            auto_cert_days: validity_days,
            ..Default::default()
        }
    }

    /// Create configuration from environment variables.
    pub fn from_env() -> Self {
        let mode = std::env::var("GRPC_TLS")
            .map(|v| GrpcTlsMode::from_env(&v))
            .unwrap_or_default();

        let cert_path = std::env::var("GRPC_TLS_CERT").ok().map(PathBuf::from);
        let key_path = std::env::var("GRPC_TLS_KEY").ok().map(PathBuf::from);
        let ca_path = std::env::var("GRPC_TLS_CA").ok().map(PathBuf::from);

        let auto_cert_days = std::env::var("GRPC_TLS_AUTO_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(365);

        let auto_cert_cn = std::env::var("GRPC_TLS_AUTO_CN")
            .unwrap_or_else(|_| "localhost".to_string());

        let auto_cert_dir = std::env::var("GRPC_TLS_AUTO_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp/tokio_php"));

        Self {
            mode,
            cert_path,
            key_path,
            ca_path,
            auto_cert_days,
            auto_cert_cn,
            auto_cert_dir,
        }
    }

    /// Check if TLS is enabled.
    pub fn is_enabled(&self) -> bool {
        self.mode != GrpcTlsMode::Off
    }

    /// Check if mTLS is enabled (CA certificate provided).
    pub fn is_mtls(&self) -> bool {
        self.ca_path.is_some()
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<(), String> {
        match self.mode {
            GrpcTlsMode::Off => Ok(()),
            GrpcTlsMode::Auto => {
                if self.auto_cert_cn.is_empty() {
                    return Err("GRPC_TLS_AUTO_CN cannot be empty".to_string());
                }
                if self.auto_cert_days == 0 {
                    return Err("GRPC_TLS_AUTO_DAYS must be > 0".to_string());
                }
                Ok(())
            }
            GrpcTlsMode::On => {
                if self.cert_path.is_none() {
                    return Err("GRPC_TLS_CERT required when GRPC_TLS=on".to_string());
                }
                if self.key_path.is_none() {
                    return Err("GRPC_TLS_KEY required when GRPC_TLS=on".to_string());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_from_env() {
        assert_eq!(GrpcTlsMode::from_env("off"), GrpcTlsMode::Off);
        assert_eq!(GrpcTlsMode::from_env("auto"), GrpcTlsMode::Auto);
        assert_eq!(GrpcTlsMode::from_env("on"), GrpcTlsMode::On);
        assert_eq!(GrpcTlsMode::from_env("true"), GrpcTlsMode::On);
        assert_eq!(GrpcTlsMode::from_env("1"), GrpcTlsMode::On);
        assert_eq!(GrpcTlsMode::from_env("invalid"), GrpcTlsMode::Off);
    }

    #[test]
    fn test_validate_off() {
        let config = GrpcTlsConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_auto() {
        let config = GrpcTlsConfig::auto("localhost", 365);
        assert!(config.validate().is_ok());

        let mut config = GrpcTlsConfig::auto("", 365);
        config.auto_cert_cn = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_on() {
        let config = GrpcTlsConfig::new(
            GrpcTlsMode::On,
            Some("/path/cert.pem"),
            Some("/path/key.pem"),
            None::<&str>,
        );
        assert!(config.validate().is_ok());

        let config = GrpcTlsConfig::new(
            GrpcTlsMode::On,
            None::<&str>,
            None::<&str>,
            None::<&str>,
        );
        assert!(config.validate().is_err());
    }
}
