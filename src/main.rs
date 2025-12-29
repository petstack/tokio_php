mod executor;
pub mod logging;
pub mod profiler;
mod server;
mod types;

use std::net::SocketAddr;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::server::{access_log, rate_limit, Server, ServerConfig};

#[cfg(feature = "php")]
use crate::executor::PhpExecutor;

#[cfg(feature = "php")]
use crate::executor::ExtExecutor;

use crate::executor::StubExecutor;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging with custom JSON formatter
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tokio_php=info".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .event_format(logging::JsonFormatter::new("tokio_php"))
                .with_ansi(false),
        )
        .init();

    // Initialize profiler
    profiler::init();

    // Initialize access logging
    let access_log_enabled = std::env::var("ACCESS_LOG")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    access_log::init(access_log_enabled);

    // Initialize rate limiting (RATE_LIMIT=requests per window, RATE_WINDOW=seconds)
    let rate_limit_value = std::env::var("RATE_LIMIT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let rate_window = std::env::var("RATE_WINDOW")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60);
    if rate_limit_value > 0 {
        rate_limit::init(rate_limit_value, rate_window);
    }

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

    // Queue capacity (0 = default: workers * 100)
    let queue_capacity = std::env::var("QUEUE_CAPACITY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

    // Use single-threaded Tokio runtime - PHP workers handle blocking work
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async_main(num_workers, worker_threads, queue_capacity))
}

async fn async_main(
    num_workers: usize,
    worker_threads: usize,
    queue_capacity: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Configure server address
    let addr: SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;

    // TLS configuration
    let tls_cert = std::env::var("TLS_CERT").ok();
    let tls_key = std::env::var("TLS_KEY").ok();

    // Index file for single entry point mode (Laravel/Symfony style routing)
    // Filter out empty strings
    let index_file = std::env::var("INDEX_FILE")
        .ok()
        .filter(|s| !s.is_empty());

    // Document root (default: /var/www/html)
    let document_root = std::env::var("DOCUMENT_ROOT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/var/www/html".to_string());

    info!("Document root: {}", document_root);

    let mut config = ServerConfig::new(addr)
        .with_workers(num_workers)
        .with_document_root(&document_root);

    if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
        info!("TLS enabled: cert={}, key={}", cert, key);
        config = config.with_tls(cert, key);
    }

    if let Some(ref idx) = index_file {
        config = config.with_index_file(idx.clone());
    }

    // Internal server for /health and /metrics
    if let Some(internal_addr) = std::env::var("INTERNAL_ADDR")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
    {
        config = config.with_internal_addr(internal_addr);
    }

    // Custom error pages directory
    if let Some(error_pages_dir) = std::env::var("ERROR_PAGES_DIR")
        .ok()
        .filter(|s| !s.is_empty())
    {
        info!("Error pages directory: {}", error_pages_dir);
        config = config.with_error_pages_dir(error_pages_dir);
    }

    // Graceful shutdown drain timeout (default: 30 seconds)
    let drain_timeout_secs = std::env::var("DRAIN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30);
    config = config.with_drain_timeout(Duration::from_secs(drain_timeout_secs));

    // Static file cache TTL (default: 1d, "off" to disable)
    let static_cache_ttl = std::env::var("STATIC_CACHE_TTL")
        .ok()
        .map(|v| server::config::StaticCacheTtl::parse(&v))
        .unwrap_or_default();
    if static_cache_ttl.is_enabled() {
        info!("Static file caching: {} seconds", static_cache_ttl.as_secs());
    } else {
        info!("Static file caching: disabled");
    }
    config = config.with_static_cache_ttl(static_cache_ttl);

    // Check for stub mode (via env var or feature)
    let use_stub = std::env::var("USE_STUB")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    #[cfg(all(feature = "stub", not(feature = "php")))]
    let use_stub = true;

    // Check for extension mode (USE_EXT=1 uses FFI superglobals)
    let use_ext = std::env::var("USE_EXT")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    if use_stub {
        info!("Running in STUB mode (PHP disabled)");
        let executor = StubExecutor::new();
        let server = Server::new(config, executor)?;
        run_server(server).await
    } else if use_ext {
        #[cfg(feature = "php")]
        {
            info!(
                "Initializing EXT executor with {} workers (FFI superglobals)...",
                worker_threads
            );

            let executor = ExtExecutor::with_queue_capacity(worker_threads, queue_capacity)
                .map_err(|e| {
                    eprintln!("Failed to initialize ExtExecutor: {}", e);
                    e
                })?;

            info!("ExtExecutor ready ({} workers, FFI mode)", executor.worker_count());

            let server = Server::new(config, executor)?;
            run_server(server).await
        }

        #[cfg(not(feature = "php"))]
        {
            info!("PHP feature not enabled, falling back to stub mode");
            let executor = StubExecutor::new();
            let server = Server::new(config, executor)?;
            run_server(server).await
        }
    } else {
        #[cfg(feature = "php")]
        {
            info!(
                "Initializing PHP executor with {} workers...",
                worker_threads
            );

            let executor = PhpExecutor::with_queue_capacity(worker_threads, queue_capacity)
                .map_err(|e| {
                    eprintln!("Failed to initialize PHP: {}", e);
                    e
                })?;

            info!("PHP executor ready ({} workers)", executor.worker_count());

            let server = Server::new(config, executor)?;
            run_server(server).await
        }

        #[cfg(not(feature = "php"))]
        {
            info!("PHP feature not enabled, falling back to stub mode");
            let executor = StubExecutor::new();
            let server = Server::new(config, executor)?;
            run_server(server).await
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
