//! gRPC server configuration.

use std::net::SocketAddr;

use crate::grpc::tls::GrpcTlsConfig;

/// gRPC server configuration.
#[derive(Debug, Clone, Default)]
pub struct GrpcConfig {
    /// gRPC server address (None = disabled)
    pub addr: Option<SocketAddr>,
    /// TLS configuration
    pub tls: GrpcTlsConfig,
}

impl GrpcConfig {
    /// Load gRPC configuration from environment variables.
    pub fn from_env() -> Result<Self, super::ConfigError> {
        let addr = std::env::var("GRPC_ADDR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<SocketAddr>()
                    .map_err(|e| super::ConfigError::Parse {
                        key: "GRPC_ADDR".to_string(),
                        value: s.clone(),
                        error: e.to_string(),
                    })
            })
            .transpose()?;

        let tls = GrpcTlsConfig::from_env();

        // Validate TLS config if gRPC is enabled
        if addr.is_some() {
            tls.validate().map_err(|e| super::ConfigError::Invalid {
                key: "GRPC_TLS".to_string(),
                message: e,
            })?;
        }

        Ok(Self { addr, tls })
    }

    /// Check if gRPC server is enabled.
    pub fn is_enabled(&self) -> bool {
        self.addr.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_disabled_by_default() {
        std::env::remove_var("GRPC_ADDR");
        std::env::remove_var("GRPC_TLS");

        let config = GrpcConfig::from_env().unwrap();
        assert!(!config.is_enabled());
        assert!(config.addr.is_none());
    }

    #[test]
    fn test_grpc_enabled_with_addr() {
        std::env::set_var("GRPC_ADDR", "0.0.0.0:50051");
        std::env::remove_var("GRPC_TLS");

        let config = GrpcConfig::from_env().unwrap();
        assert!(config.is_enabled());
        assert_eq!(
            config.addr.unwrap(),
            "0.0.0.0:50051".parse::<SocketAddr>().unwrap()
        );

        std::env::remove_var("GRPC_ADDR");
    }
}
