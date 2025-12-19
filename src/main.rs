mod php;
mod server;

use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::php::PhpRuntime;
use crate::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tokio_php=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting tokio_php server...");

    // Initialize PHP runtime
    PhpRuntime::init().map_err(|e| {
        eprintln!("Failed to initialize PHP: {}", e);
        e
    })?;

    // Configure server address
    let addr: SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;

    // Create and run server
    let server = Server::new(addr);

    // Handle shutdown gracefully
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                eprintln!("Server error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down...");
        }
    }

    // Cleanup PHP runtime
    PhpRuntime::shutdown();

    Ok(())
}
