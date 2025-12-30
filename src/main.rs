mod config;
mod executor;
pub mod logging;
pub mod profiler;
mod server;
pub mod trace_context;
mod types;

use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{Config, ExecutorType};
use crate::server::{access_log, rate_limit, Server, ServerConfig};

#[cfg(feature = "php")]
use crate::executor::PhpExecutor;

#[cfg(feature = "php")]
use crate::executor::ExtExecutor;

use crate::executor::StubExecutor;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load configuration from environment
    let config = Config::from_env().map_err(|e| {
        eprintln!("Configuration error: {}", e);
        e
    })?;

    // Initialize logging with custom JSON formatter
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.logging.filter.clone().into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .event_format(logging::JsonFormatter::new(config.logging.service_name.clone()))
                .with_ansi(false),
        )
        .init();

    // Initialize profiler
    profiler::init();

    // Initialize access logging
    access_log::init(config.middleware.access_log);

    // Initialize rate limiting
    if let Some(limit) = config.middleware.rate_limit {
        rate_limit::init(limit, config.middleware.rate_window);
    }

    // Log configuration summary
    config.log_summary();

    info!("Starting tokio_php server...");

    // Use single-threaded Tokio runtime - PHP workers handle blocking work
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async_main(config))
}

async fn async_main(config: Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Build ServerConfig from new Config
    let mut server_config = ServerConfig::new(config.server.listen_addr)
        .with_workers(config.executor.workers)
        .with_document_root(config.server.document_root.to_str().unwrap_or("/var/www/html"));

    // TLS configuration
    if let (Some(cert), Some(key)) = (
        config.server.tls.cert_path.as_ref(),
        config.server.tls.key_path.as_ref(),
    ) {
        info!("TLS enabled: cert={:?}, key={:?}", cert, key);
        server_config = server_config.with_tls(
            cert.to_string_lossy().into_owned(),
            key.to_string_lossy().into_owned(),
        );
    }

    // Index file
    if let Some(ref idx) = config.server.index_file {
        server_config = server_config.with_index_file(idx.clone());
    }

    // Internal server
    if let Some(internal_addr) = config.server.internal_addr {
        server_config = server_config.with_internal_addr(internal_addr);
    }

    // Error pages
    if let Some(ref dir) = config.server.error_pages_dir {
        info!("Error pages directory: {:?}", dir);
        server_config = server_config.with_error_pages_dir(dir.to_string_lossy().into_owned());
    }

    // Drain timeout
    server_config = server_config.with_drain_timeout(config.server.drain_timeout);

    // Static cache TTL
    let static_cache_ttl = crate::server::config::StaticCacheTtl(config.server.static_cache_ttl.0);
    server_config = server_config.with_static_cache_ttl(static_cache_ttl);

    // Request timeout
    let request_timeout = crate::server::config::RequestTimeout(config.server.request_timeout.0);
    server_config = server_config.with_request_timeout(request_timeout);

    // Get worker parameters
    let worker_threads = config.executor.worker_count();
    let queue_capacity = config.executor.queue_capacity;

    // Create executor based on type
    match config.executor.executor_type {
        ExecutorType::Stub => {
            info!("Running in STUB mode (PHP disabled)");
            let executor = StubExecutor::new();
            let server = Server::new(server_config, executor)?;
            run_server(server).await
        }
        ExecutorType::Ext => {
            #[cfg(feature = "php")]
            {
                info!(
                    "Initializing EXT executor with {} workers (FFI superglobals)...",
                    worker_threads
                );

                let executor =
                    ExtExecutor::with_queue_capacity(worker_threads, queue_capacity).map_err(
                        |e| {
                            eprintln!("Failed to initialize ExtExecutor: {}", e);
                            e
                        },
                    )?;

                info!(
                    "ExtExecutor ready ({} workers, FFI mode)",
                    executor.worker_count()
                );

                let server = Server::new(server_config, executor)?;
                run_server(server).await
            }

            #[cfg(not(feature = "php"))]
            {
                info!("PHP feature not enabled, falling back to stub mode");
                let executor = StubExecutor::new();
                let server = Server::new(server_config, executor)?;
                run_server(server).await
            }
        }
        ExecutorType::Php => {
            #[cfg(feature = "php")]
            {
                info!(
                    "Initializing PHP executor with {} workers...",
                    worker_threads
                );

                let executor =
                    PhpExecutor::with_queue_capacity(worker_threads, queue_capacity).map_err(
                        |e| {
                            eprintln!("Failed to initialize PHP: {}", e);
                            e
                        },
                    )?;

                info!("PHP executor ready ({} workers)", executor.worker_count());

                let server = Server::new(server_config, executor)?;
                run_server(server).await
            }

            #[cfg(not(feature = "php"))]
            {
                info!("PHP feature not enabled, falling back to stub mode");
                let executor = StubExecutor::new();
                let server = Server::new(server_config, executor)?;
                run_server(server).await
            }
        }
    }
}

/// Wait for shutdown signal (SIGINT or SIGTERM).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl_c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn run_server<E: crate::executor::ScriptExecutor + 'static>(
    server: Server<E>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let drain_timeout = server.drain_timeout();

    // Handle shutdown gracefully with tokio::select
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                eprintln!("Server error: {}", e);
            }
        }
        _ = shutdown_signal() => {
            info!("Received shutdown signal, initiating graceful shutdown...");

            // Trigger shutdown - stops accept loops and signals all connections
            // Each connection will receive the shutdown signal and send HTTP/2 GOAWAY
            server.trigger_shutdown();

            let active = server.active_connections();
            if active > 0 {
                info!(
                    "Waiting up to {}s for {} active connections to complete (HTTP/2 GOAWAY sent)",
                    drain_timeout.as_secs(),
                    active
                );

                // Wait for connections to drain with timeout
                if server.wait_for_drain(drain_timeout).await {
                    info!("All connections drained successfully");
                } else {
                    info!("Drain timeout reached, forcing shutdown");
                }
            } else {
                info!("No active connections, shutting down immediately");
            }
        }
    }

    // Cleanup PHP workers
    server.shutdown();
    info!("Shutdown complete");

    Ok(())
}
