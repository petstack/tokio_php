//! gRPC server setup and configuration.

use std::net::SocketAddr;
use std::sync::Arc;

use tonic::transport::Server;
use tracing::info;

use crate::executor::ScriptExecutor;
use crate::health::HealthChecker;

use super::proto::php_service_server::PhpServiceServer;
use super::service::PhpServiceImpl;
use super::tls::{GrpcTlsConfig, GrpcTlsMode, TlsConfigBuilder};

/// gRPC server for PHP script execution.
pub struct GrpcServer<E: ScriptExecutor> {
    addr: SocketAddr,
    service: PhpServiceImpl<E>,
    tls_config: GrpcTlsConfig,
}

impl<E: ScriptExecutor + 'static> GrpcServer<E> {
    /// Create a new gRPC server.
    pub fn new(
        addr: SocketAddr,
        executor: Arc<E>,
        health_checker: Arc<HealthChecker>,
        document_root: String,
    ) -> Self {
        Self::with_tls(
            addr,
            executor,
            health_checker,
            document_root,
            GrpcTlsConfig::default(),
        )
    }

    /// Create a new gRPC server with TLS configuration.
    pub fn with_tls(
        addr: SocketAddr,
        executor: Arc<E>,
        health_checker: Arc<HealthChecker>,
        document_root: String,
        tls_config: GrpcTlsConfig,
    ) -> Self {
        let service = PhpServiceImpl::new(executor, health_checker, document_root);
        Self {
            addr,
            service,
            tls_config,
        }
    }

    /// Run the gRPC server.
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tls = TlsConfigBuilder::build(&self.tls_config)?;
        let service = PhpServiceServer::new(self.service);

        match (&self.tls_config.mode, tls) {
            (GrpcTlsMode::Off, _) => {
                info!(addr = %self.addr, "gRPC server listening (plaintext)");
                Server::builder()
                    .add_service(service)
                    .serve(self.addr)
                    .await?;
            }
            (GrpcTlsMode::Auto, Some(tls_config)) => {
                info!(addr = %self.addr, mode = "auto", "gRPC server listening (TLS auto-generated)");
                Server::builder()
                    .tls_config(tls_config)?
                    .add_service(service)
                    .serve(self.addr)
                    .await?;
            }
            (GrpcTlsMode::On, Some(tls_config)) => {
                let mtls = self.tls_config.is_mtls();
                if mtls {
                    info!(addr = %self.addr, mode = "on", mtls = true, "gRPC server listening (mTLS)");
                } else {
                    info!(addr = %self.addr, mode = "on", "gRPC server listening (TLS)");
                }
                Server::builder()
                    .tls_config(tls_config)?
                    .add_service(service)
                    .serve(self.addr)
                    .await?;
            }
            _ => {
                return Err("Invalid TLS configuration".into());
            }
        }

        Ok(())
    }

    /// Run the gRPC server with graceful shutdown.
    pub async fn run_with_shutdown(
        self,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tls = TlsConfigBuilder::build(&self.tls_config)?;
        let service = PhpServiceServer::new(self.service);

        match (&self.tls_config.mode, tls) {
            (GrpcTlsMode::Off, _) => {
                info!(addr = %self.addr, "gRPC server listening (plaintext)");
                Server::builder()
                    .add_service(service)
                    .serve_with_shutdown(self.addr, shutdown)
                    .await?;
            }
            (GrpcTlsMode::Auto, Some(tls_config)) => {
                info!(addr = %self.addr, mode = "auto", "gRPC server listening (TLS auto-generated)");
                Server::builder()
                    .tls_config(tls_config)?
                    .add_service(service)
                    .serve_with_shutdown(self.addr, shutdown)
                    .await?;
            }
            (GrpcTlsMode::On, Some(tls_config)) => {
                let mtls = self.tls_config.is_mtls();
                if mtls {
                    info!(addr = %self.addr, mode = "on", mtls = true, "gRPC server listening (mTLS)");
                } else {
                    info!(addr = %self.addr, mode = "on", "gRPC server listening (TLS)");
                }
                Server::builder()
                    .tls_config(tls_config)?
                    .add_service(service)
                    .serve_with_shutdown(self.addr, shutdown)
                    .await?;
            }
            _ => {
                return Err("Invalid TLS configuration".into());
            }
        }

        Ok(())
    }
}
