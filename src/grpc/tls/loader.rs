//! Certificate loading and TLS configuration building.

use std::fs;
use std::io;
use std::path::Path;

use tonic::transport::{Certificate, Identity, ServerTlsConfig};
use tracing::info;

use super::auto_cert::AutoCertGenerator;
use super::config::{GrpcTlsConfig, GrpcTlsMode};

/// Builder for gRPC TLS configuration.
pub struct TlsConfigBuilder;

impl TlsConfigBuilder {
    /// Build TLS configuration from GrpcTlsConfig.
    ///
    /// Returns `None` if TLS is disabled (mode=off).
    /// Returns `Some(ServerTlsConfig)` if TLS is enabled.
    pub fn build(config: &GrpcTlsConfig) -> io::Result<Option<ServerTlsConfig>> {
        // Validate configuration
        config
            .validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        match config.mode {
            GrpcTlsMode::Off => {
                info!("gRPC TLS disabled (plaintext mode)");
                Ok(None)
            }
            GrpcTlsMode::Auto => Self::build_auto(config),
            GrpcTlsMode::On => Self::build_external(config),
        }
    }

    /// Build TLS config with auto-generated certificates.
    fn build_auto(config: &GrpcTlsConfig) -> io::Result<Option<ServerTlsConfig>> {
        let generator = AutoCertGenerator::new(
            &config.auto_cert_cn,
            config.auto_cert_days,
            &config.auto_cert_dir,
        );

        let (cert_path, key_path) = generator.ensure_certs()?;

        let tls_config = Self::load_identity(&cert_path, &key_path, None)?;

        info!(
            mode = "auto",
            cert = %cert_path.display(),
            "gRPC TLS enabled with auto-generated certificates"
        );

        Ok(Some(tls_config))
    }

    /// Build TLS config with external certificates.
    fn build_external(config: &GrpcTlsConfig) -> io::Result<Option<ServerTlsConfig>> {
        let cert_path = config.cert_path.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "GRPC_TLS_CERT is required")
        })?;

        let key_path = config.key_path.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "GRPC_TLS_KEY is required")
        })?;

        let tls_config = Self::load_identity(cert_path, key_path, config.ca_path.as_deref())?;

        if let Some(ca_path) = &config.ca_path {
            info!(
                mode = "on",
                cert = %cert_path.display(),
                ca = %ca_path.display(),
                "gRPC mTLS enabled (client certificates required)"
            );
        } else {
            info!(
                mode = "on",
                cert = %cert_path.display(),
                "gRPC TLS enabled with external certificates"
            );
        }

        Ok(Some(tls_config))
    }

    /// Load identity (cert + key) and optionally CA for mTLS.
    fn load_identity(
        cert_path: &Path,
        key_path: &Path,
        ca_path: Option<&Path>,
    ) -> io::Result<ServerTlsConfig> {
        // Load certificate and key
        let cert_pem = fs::read_to_string(cert_path).map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Failed to read certificate {}: {}", cert_path.display(), e),
            )
        })?;

        let key_pem = fs::read_to_string(key_path).map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Failed to read private key {}: {}", key_path.display(), e),
            )
        })?;

        let identity = Identity::from_pem(&cert_pem, &key_pem);

        // Build TLS config
        let mut tls_config = ServerTlsConfig::new().identity(identity);

        // Add CA for mTLS (client certificate verification)
        if let Some(ca_path) = ca_path {
            let ca_pem = fs::read_to_string(ca_path).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Failed to read CA certificate {}: {}", ca_path.display(), e),
                )
            })?;

            let ca_cert = Certificate::from_pem(&ca_pem);
            tls_config = tls_config.client_ca_root(ca_cert);
        }

        Ok(tls_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_build_off() {
        let config = GrpcTlsConfig::default();
        let result = TlsConfigBuilder::build(&config).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_build_auto() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = GrpcTlsConfig::auto("localhost", 365);
        config.auto_cert_dir = temp_dir.path().to_path_buf();

        let result = TlsConfigBuilder::build(&config).unwrap();
        assert!(result.is_some());

        // Verify files were created
        assert!(temp_dir.path().join("grpc-cert.pem").exists());
        assert!(temp_dir.path().join("grpc-key.pem").exists());
    }
}
