//! gRPC server implementation for tokio_php.
//!
//! Provides gRPC interface for PHP script execution,
//! enabling microservices architecture with service-to-service communication.
//!
//! # Features
//!
//! - **Unary RPC**: Execute PHP scripts via gRPC
//! - **Server Streaming**: Stream responses for SSE/long-polling
//! - **Health Checking**: gRPC health checking protocol
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::grpc::GrpcServer;
//!
//! let server = GrpcServer::new(
//!     "0.0.0.0:50051".parse()?,
//!     executor,
//!     health_checker,
//!     "/var/www/html".to_string(),
//! );
//!
//! server.run().await?;
//! ```
//!
//! # gRPC Client Example (grpcurl)
//!
//! ```bash
//! # Execute a PHP script
//! grpcurl -plaintext -d '{"script_path": "index.php", "method": "GET"}' \
//!     localhost:50051 tokio_php.v1.PhpService/Execute
//!
//! # Health check
//! grpcurl -plaintext localhost:50051 tokio_php.v1.PhpService/Check
//! ```

mod conversion;
mod server;
mod service;
pub mod tls;

pub use server::GrpcServer;
pub use service::PhpServiceImpl;
pub use tls::{GrpcTlsConfig, GrpcTlsMode};

// Generated code from proto
pub mod proto {
    include!("generated/tokio_php.v1.rs");
}
