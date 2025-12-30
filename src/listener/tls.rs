//! TLS listener implementation using rustls.

use std::fs::File;
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::pin::Pin;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::{TcpListener as TokioTcpListener, TcpStream};
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::ServerConfig as RustlsConfig;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use super::{Connection, Listener, TlsConfig, TlsInfo};

/// A TLS connection.
pub struct TlsConnection {
    stream: TlsStream<TcpStream>,
    remote_addr: SocketAddr,
    tls_info: TlsInfo,
}

impl TlsConnection {
    /// Create a new TLS connection.
    pub fn new(stream: TlsStream<TcpStream>, remote_addr: SocketAddr, tls_info: TlsInfo) -> Self {
        Self {
            stream,
            remote_addr,
            tls_info,
        }
    }

    /// Get the underlying TLS stream.
    pub fn into_inner(self) -> TlsStream<TcpStream> {
        self.stream
    }
}

impl Connection for TlsConnection {
    fn remote_addr(&self) -> Option<SocketAddr> {
        Some(self.remote_addr)
    }

    fn tls_info(&self) -> Option<TlsInfo> {
        Some(self.tls_info.clone())
    }
}

impl tokio::io::AsyncRead for TlsConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for TlsConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// A TLS listener that accepts encrypted connections.
pub struct TlsListener {
    tcp_listener: TokioTcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    /// Create a new TLS listener bound to the given address.
    pub async fn bind(addr: SocketAddr, config: &TlsConfig) -> io::Result<Self> {
        let tcp_listener = TokioTcpListener::bind(addr).await?;
        let tls_config = Self::load_tls_config(config).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidInput, e.to_string())
        })?;
        let acceptor = TlsAcceptor::from(Arc::new(tls_config));

        Ok(Self {
            tcp_listener,
            acceptor,
        })
    }

    /// Create a TLS listener from an existing TCP listener and acceptor.
    pub fn from_parts(tcp_listener: TokioTcpListener, acceptor: TlsAcceptor) -> Self {
        Self {
            tcp_listener,
            acceptor,
        }
    }

    /// Load TLS configuration from cert and key files.
    fn load_tls_config(config: &TlsConfig) -> Result<RustlsConfig, Box<dyn std::error::Error + Send + Sync>> {
        // Load certificate chain
        let cert_file = File::open(&config.cert_path)?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
            .filter_map(|r| r.ok())
            .collect();

        if certs.is_empty() {
            return Err("No certificates found in cert file".into());
        }

        // Load private key
        let key_file = File::open(&config.key_path)?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)?
            .ok_or("No private key found in key file")?;

        // Build TLS config with ALPN for HTTP/2
        let mut tls_config = RustlsConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        // Enable ALPN for HTTP/2 and HTTP/1.1
        tls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(tls_config)
    }

    /// Get TLS protocol version string.
    fn protocol_version(conn: &tokio_rustls::server::TlsStream<TcpStream>) -> String {
        let (_, server_conn) = conn.get_ref();
        match server_conn.protocol_version() {
            Some(tokio_rustls::rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2".to_string(),
            Some(tokio_rustls::rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3".to_string(),
            _ => "unknown".to_string(),
        }
    }

    /// Get ALPN negotiated protocol.
    fn alpn_protocol(conn: &tokio_rustls::server::TlsStream<TcpStream>) -> Option<String> {
        let (_, server_conn) = conn.get_ref();
        server_conn
            .alpn_protocol()
            .map(|p| String::from_utf8_lossy(p).to_string())
    }
}

impl Listener for TlsListener {
    type Conn = TlsConnection;

    fn accept(&self) -> Pin<Box<dyn Future<Output = io::Result<Self::Conn>> + Send + '_>> {
        Box::pin(async move {
            let (stream, addr) = self.tcp_listener.accept().await?;

            // Set TCP_NODELAY for lower latency
            if let Err(e) = stream.set_nodelay(true) {
                tracing::warn!(error = %e, "Failed to set TCP_NODELAY");
            }

            // Perform TLS handshake with timing
            let handshake_start = Instant::now();
            let tls_stream = self.acceptor.accept(stream).await.map_err(|e| {
                io::Error::new(io::ErrorKind::ConnectionAborted, e)
            })?;
            let handshake_duration = handshake_start.elapsed();

            // Extract TLS info
            let tls_info = TlsInfo {
                protocol: Self::protocol_version(&tls_stream),
                alpn: Self::alpn_protocol(&tls_stream),
                handshake_duration,
            };

            Ok(TlsConnection::new(tls_stream, addr, tls_info))
        })
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.tcp_listener.local_addr()
    }

    fn name(&self) -> &'static str {
        "tls"
    }

    fn is_tls(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_tls_info() {
        let info = TlsInfo {
            protocol: "TLSv1.3".to_string(),
            alpn: Some("h2".to_string()),
            handshake_duration: Duration::from_millis(50),
        };

        assert_eq!(info.protocol, "TLSv1.3");
        assert_eq!(info.alpn.as_deref(), Some("h2"));
        assert_eq!(info.handshake_duration.as_millis(), 50);
    }

    #[test]
    fn test_tls_config() {
        let config = TlsConfig {
            cert_path: "/path/to/cert.pem".to_string(),
            key_path: "/path/to/key.pem".to_string(),
        };

        assert_eq!(config.cert_path, "/path/to/cert.pem");
        assert_eq!(config.key_path, "/path/to/key.pem");
    }

    // Note: Full TLS listener tests require valid certificates
    // and are covered by integration tests.
}
