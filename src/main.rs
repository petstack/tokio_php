mod php;
mod server;

use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::php::PhpRuntime;
use crate::server::Server;

/// Number of PHP workers. Set to 0 for auto-detection (CPU cores count).
const PHP_WORKERS: usize = 0;

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

    // Determine number of workers (0 = auto-detect from CPU cores)
    let num_workers = if PHP_WORKERS == 0 {
        num_cpus::get()
    } else {
        PHP_WORKERS
    };

    // Also check environment variable (overrides constant)
    let num_workers = std::env::var("PHP_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|n| if n == 0 { num_cpus::get() } else { n })
        .unwrap_or(num_workers);

    info!("Initializing PHP worker pool with {} workers...", num_workers);

    PhpRuntime::init_with_workers(num_workers).map_err(|e| {
        eprintln!("Failed to initialize PHP: {}", e);
        e
    })?;

    info!("PHP worker pool ready ({} workers)", PhpRuntime::worker_count());

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
