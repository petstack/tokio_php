//! Listener abstractions for accepting connections.
//!
//! This module provides traits and implementations for accepting
//! TCP and TLS connections in a unified way.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Listener trait                         │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
//! │  │ TcpListener │    │ TlsListener │    │ UnixListener    │  │
//! │  │   (tcp.rs)  │    │   (tls.rs)  │    │   (future)      │  │
//! │  └──────┬──────┘    └──────┬──────┘    └────────┬────────┘  │
//! │         │                  │                    │           │
//! │         └──────────────────┴────────────────────┘           │
//! │                           │                                 │
//! │                   ┌───────▼───────┐                         │
//! │                   │   Connection  │                         │
//! │                   └───────────────┘                         │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod tcp;
mod tls;

pub use tcp::TcpListener;
pub use tls::TlsListener;

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};

/// A connection accepted by a listener.
pub trait Connection: AsyncRead + AsyncWrite + Send + Unpin + 'static {
    /// Get the remote address of the connection.
    fn remote_addr(&self) -> Option<SocketAddr>;

    /// Get TLS information if this is a TLS connection.
    fn tls_info(&self) -> Option<TlsInfo> {
        None
    }
}

/// TLS connection information.
#[derive(Debug, Clone)]
pub struct TlsInfo {
    /// TLS protocol version (e.g., "TLSv1.3").
    pub protocol: String,
    /// ALPN negotiated protocol (e.g., "h2", "http/1.1").
    pub alpn: Option<String>,
    /// TLS handshake duration.
    pub handshake_duration: Duration,
}

/// Trait for listening and accepting connections.
pub trait Listener: Send + Sync {
    /// The connection type produced by this listener.
    type Conn: Connection;

    /// Accept a new connection.
    ///
    /// Returns the connection and its remote address.
    fn accept(&self) -> Pin<Box<dyn Future<Output = io::Result<Self::Conn>> + Send + '_>>;

    /// Get the local address this listener is bound to.
    fn local_addr(&self) -> io::Result<SocketAddr>;

    /// Get the listener name for logging.
    fn name(&self) -> &'static str;

    /// Check if this listener uses TLS.
    fn is_tls(&self) -> bool {
        false
    }
}

/// Configuration for creating listeners.
#[derive(Debug, Clone)]
pub struct ListenerConfig {
    /// Address to bind to.
    pub addr: SocketAddr,
    /// TLS configuration (if any).
    pub tls: Option<TlsConfig>,
}

/// TLS configuration.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to certificate file (PEM format).
    pub cert_path: String,
    /// Path to private key file (PEM format).
    pub key_path: String,
}

impl ListenerConfig {
    /// Create a new TCP listener configuration.
    pub fn tcp(addr: SocketAddr) -> Self {
        Self { addr, tls: None }
    }

    /// Create a new TLS listener configuration.
    pub fn tls(addr: SocketAddr, cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        Self {
            addr,
            tls: Some(TlsConfig {
                cert_path: cert_path.into(),
                key_path: key_path.into(),
            }),
        }
    }

    /// Check if TLS is configured.
    pub fn is_tls(&self) -> bool {
        self.tls.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_listener_config_tcp() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let config = ListenerConfig::tcp(addr);

        assert_eq!(config.addr, addr);
        assert!(!config.is_tls());
    }

    #[test]
    fn test_listener_config_tls() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8443);
        let config = ListenerConfig::tls(addr, "/path/to/cert.pem", "/path/to/key.pem");

        assert_eq!(config.addr, addr);
        assert!(config.is_tls());
        assert_eq!(config.tls.as_ref().unwrap().cert_path, "/path/to/cert.pem");
        assert_eq!(config.tls.as_ref().unwrap().key_path, "/path/to/key.pem");
    }

    #[test]
    fn test_tls_info() {
        let info = TlsInfo {
            protocol: "TLSv1.3".to_string(),
            alpn: Some("h2".to_string()),
            handshake_duration: Duration::from_millis(50),
        };

        assert_eq!(info.protocol, "TLSv1.3");
        assert_eq!(info.alpn, Some("h2".to_string()));
    }
}
