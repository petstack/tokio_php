//! gRPC server setup and configuration.

use std::net::SocketAddr;
use std::sync::Arc;

use tonic::transport::Server;
use tracing::info;

use crate::executor::ScriptExecutor;
use crate::health::HealthChecker;

use super::proto::php_service_server::PhpServiceServer;
use super::service::PhpServiceImpl;

/// gRPC server for PHP script execution.
pub struct GrpcServer<E: ScriptExecutor> {
    addr: SocketAddr,
    service: PhpServiceImpl<E>,
}

impl<E: ScriptExecutor + 'static> GrpcServer<E> {
    /// Create a new gRPC server.
    pub fn new(
        addr: SocketAddr,
        executor: Arc<E>,
        health_checker: Arc<HealthChecker>,
        document_root: String,
    ) -> Self {
        let service = PhpServiceImpl::new(executor, health_checker, document_root);
        Self { addr, service }
    }

    /// Run the gRPC server.
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("gRPC server listening on {}", self.addr);

        Server::builder()
            .add_service(PhpServiceServer::new(self.service))
            .serve(self.addr)
            .await?;

        Ok(())
    }

    /// Run the gRPC server with graceful shutdown.
    pub async fn run_with_shutdown(
        self,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("gRPC server listening on {}", self.addr);

        Server::builder()
            .add_service(PhpServiceServer::new(self.service))
            .serve_with_shutdown(self.addr, shutdown)
            .await?;

        Ok(())
    }
}
