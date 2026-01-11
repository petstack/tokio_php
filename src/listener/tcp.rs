//! TCP listener implementation.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;

use tokio::net::{TcpListener as TokioTcpListener, TcpStream};

use super::{Connection, Listener, TlsInfo};

/// A TCP connection.
pub struct TcpConnection {
    stream: TcpStream,
    remote_addr: SocketAddr,
}

impl TcpConnection {
    /// Create a new TCP connection.
    pub fn new(stream: TcpStream, remote_addr: SocketAddr) -> Self {
        Self {
            stream,
            remote_addr,
        }
    }

    /// Get the underlying TCP stream.
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

impl Connection for TcpConnection {
    fn remote_addr(&self) -> Option<SocketAddr> {
        Some(self.remote_addr)
    }

    fn tls_info(&self) -> Option<TlsInfo> {
        None
    }
}

impl tokio::io::AsyncRead for TcpConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for TcpConnection {
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

/// A TCP listener that accepts plain TCP connections.
pub struct TcpListener {
    inner: TokioTcpListener,
}

impl TcpListener {
    /// Create a new TCP listener bound to the given address.
    pub async fn bind(addr: SocketAddr) -> io::Result<Self> {
        let inner = TokioTcpListener::bind(addr).await?;
        Ok(Self { inner })
    }

    /// Create a TCP listener from an existing tokio TcpListener.
    pub fn from_std(listener: TokioTcpListener) -> Self {
        Self { inner: listener }
    }

    /// Get the underlying tokio TcpListener.
    pub fn into_inner(self) -> TokioTcpListener {
        self.inner
    }
}

impl Listener for TcpListener {
    type Conn = TcpConnection;

    fn accept(&self) -> Pin<Box<dyn Future<Output = io::Result<Self::Conn>> + Send + '_>> {
        Box::pin(async move {
            let (stream, addr) = self.inner.accept().await?;

            // Set TCP_NODELAY for lower latency
            if let Err(e) = stream.set_nodelay(true) {
                tracing::warn!(error = %e, "Failed to set TCP_NODELAY");
            }

            Ok(TcpConnection::new(stream, addr))
        })
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    fn name(&self) -> &'static str {
        "tcp"
    }

    fn is_tls(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_tcp_listener_bind() {
        // Use port 0 to get a random available port
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let listener = TcpListener::bind(addr).await.unwrap();

        let local_addr = listener.local_addr().unwrap();
        assert_eq!(local_addr.ip(), addr.ip());
        assert_ne!(local_addr.port(), 0); // Should have a real port now

        assert_eq!(listener.name(), "tcp");
        assert!(!listener.is_tls());
    }

    #[tokio::test]
    async fn test_tcp_connection() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let listener = TcpListener::bind(addr).await.unwrap();
        let server_addr = listener.local_addr().unwrap();

        // Spawn a task to accept the connection
        let accept_task = tokio::spawn(async move { listener.accept().await.unwrap() });

        // Connect to the server
        let _client = TcpStream::connect(server_addr).await.unwrap();

        // Get the server-side connection
        let conn = accept_task.await.unwrap();

        assert!(conn.remote_addr().is_some());
        assert!(conn.tls_info().is_none());
    }
}
