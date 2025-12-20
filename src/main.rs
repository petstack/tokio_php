mod executor;
mod server;
mod types;

use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::server::{Server, ServerConfig};

#[cfg(feature = "php")]
use crate::executor::PhpExecutor;

use crate::executor::StubExecutor;

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

    // Configure server address
    let addr: SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;

    let config = ServerConfig::new(addr);

    // Check for stub mode (via env var or feature)
    let use_stub = std::env::var("USE_STUB")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    #[cfg(all(feature = "stub", not(feature = "php")))]
    let use_stub = true;

    if use_stub {
        info!("Running in STUB mode (PHP disabled)");
        let executor = StubExecutor::new();
        let server = Server::new(config, executor);
        run_server(server).await
    } else {
        #[cfg(feature = "php")]
        {
            // Determine number of workers
            let num_workers = get_worker_count();
            info!("Initializing PHP executor with {} workers...", num_workers);

            let executor = PhpExecutor::new(num_workers).map_err(|e| {
                eprintln!("Failed to initialize PHP: {}", e);
                e
            })?;

            info!("PHP executor ready ({} workers)", executor.worker_count());

            let server = Server::new(config, executor);
            run_server(server).await
        }

        #[cfg(not(feature = "php"))]
        {
            info!("PHP feature not enabled, falling back to stub mode");
            let executor = StubExecutor::new();
            let server = Server::new(config, executor);
            run_server(server).await
        }
    }
}

async fn run_server<E: crate::executor::ScriptExecutor + 'static>(
    server: Server<E>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    // Cleanup
    server.shutdown();

    Ok(())
}

#[cfg(feature = "php")]
fn get_worker_count() -> usize {
    // Check environment variable first
    if let Ok(val) = std::env::var("PHP_WORKERS") {
        if let Ok(n) = val.parse::<usize>() {
            if n == 0 {
                return num_cpus::get();
            }
            return n;
        }
    }

    // Fall back to constant or auto-detect
    if PHP_WORKERS == 0 {
        num_cpus::get()
    } else {
        PHP_WORKERS
    }
}
