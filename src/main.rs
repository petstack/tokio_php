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

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tokio_php=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting tokio_php server...");

    // Get worker count from env (0 = auto-detect)
    let num_workers = std::env::var("PHP_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

    // Resolve 0 to actual CPU count
    let worker_threads = if num_workers == 0 {
        num_cpus::get()
    } else {
        num_workers
    };

    // Build Tokio runtime with limited worker threads
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()?;

    runtime.block_on(async_main(num_workers, worker_threads))
}

async fn async_main(
    num_workers: usize,
    worker_threads: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Configure server address
    let addr: SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;

    let config = ServerConfig::new(addr).with_workers(num_workers);

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
            info!(
                "Initializing PHP executor with {} workers...",
                worker_threads
            );

            let executor = PhpExecutor::new(worker_threads).map_err(|e| {
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

