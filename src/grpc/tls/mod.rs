//! TLS support for gRPC server.
//!
//! Provides three modes of operation:
//! - `off`: Plaintext gRPC (default, for development)
//! - `auto`: Auto-generated self-signed certificates (for development/testing)
//! - `on`: External certificates (for production)
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::grpc::tls::{GrpcTlsConfig, GrpcTlsMode};
//!
//! // Development: auto-generated certs
//! let config = GrpcTlsConfig::auto("localhost", 365);
//!
//! // Production: external certs
//! let config = GrpcTlsConfig::new(
//!     GrpcTlsMode::On,
//!     Some("/path/to/cert.pem"),
//!     Some("/path/to/key.pem"),
//!     Some("/path/to/ca.pem"),  // for mTLS
//! );
//! ```

mod auto_cert;
mod config;
mod loader;

pub use auto_cert::AutoCertGenerator;
pub use config::{GrpcTlsConfig, GrpcTlsMode};
pub use loader::TlsConfigBuilder;
