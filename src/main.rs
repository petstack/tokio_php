use std::sync::Arc;

use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tokio_php::config::{Config, ExecutorType};
use tokio_php::executor::ScriptExecutor;
use tokio_php::logging;
use tokio_php::server::{Server, ServerConfig};

#[cfg(feature = "grpc")]
use tokio_php::grpc::GrpcServer;
#[cfg(feature = "grpc")]
use tokio_php::health::HealthChecker;

#[cfg(feature = "php")]
use tokio_php::executor::PhpExecutor;

#[cfg(feature = "php")]
use tokio_php::executor::ExtExecutor;

use tokio_php::executor::StubExecutor;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Install the crypto provider for TLS (required when both HTTP and gRPC TLS are used)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

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
                .event_format(logging::JsonFormatter::new(
                    config.logging.service_name.clone(),
                ))
                .with_ansi(false),
        )
        .init();

    // Log configuration summary
    config.log_summary();

    // Debug profile warning
    #[cfg(feature = "debug-profile")]
    {
        eprintln!();
        eprintln!("⚠️  DEBUG PROFILE BUILD - Single worker mode, not for production");
        eprintln!("    Profile reports: /tmp/tokio_profile_request_{{request_id}}.md");
        eprintln!();
    }

    info!("Starting tokio_php v{}", tokio_php::VERSION);

    // Use single-threaded Tokio runtime - PHP workers handle blocking work
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async_main(config))
}

async fn async_main(config: Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Build ServerConfig from new Config
    let mut server_config = ServerConfig::new(config.server.listen_addr)
        .with_workers(config.executor.worker_count())
        .with_document_root(
            config
                .server
                .document_root
                .to_str()
                .unwrap_or("/var/www/html"),
        );

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

    // Static cache TTL (unified type, no conversion needed)
    server_config = server_config.with_static_cache_ttl(config.server.static_cache_ttl);

    // Request timeout (unified type, no conversion needed)
    server_config = server_config.with_request_timeout(config.server.request_timeout);

    // Get worker parameters
    #[allow(unused_variables)]
    let worker_threads = config.executor.worker_count();
    #[allow(unused_variables)]
    let queue_capacity = config.executor.queue_capacity();
    let profile_enabled = config.middleware.is_profile_enabled();
    let access_log_enabled = config.middleware.is_access_log_enabled();
    let rate_limit_config = config.middleware.rate_limit();

    // gRPC configuration
    #[cfg(feature = "grpc")]
    let grpc_ctx = config.grpc.addr.map(|addr| GrpcContext {
        addr,
        tls_config: config.grpc.tls.clone(),
    });

    // Create executor based on type
    match config.executor.executor_type {
        ExecutorType::Stub => {
            info!("Running in STUB mode (PHP disabled)");
            let executor = StubExecutor::new();
            let server = Server::new(server_config, executor)?
                .with_profile_enabled(profile_enabled)
                .with_access_log_enabled(access_log_enabled)
                .with_rate_limiter(rate_limit_config);
            run_server(
                    server,
                    #[cfg(feature = "grpc")]
                    grpc_ctx.clone(),
                )
                .await
        }
        ExecutorType::Ext => {
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

                info!(
                    "ExtExecutor ready ({} workers, FFI mode)",
                    executor.worker_count()
                );

                let server = Server::new(server_config, executor)?
                    .with_profile_enabled(profile_enabled)
                    .with_access_log_enabled(access_log_enabled)
                    .with_rate_limiter(rate_limit_config);
                run_server(
                    server,
                    #[cfg(feature = "grpc")]
                    grpc_ctx.clone(),
                )
                .await
            }

            #[cfg(not(feature = "php"))]
            {
                info!("PHP feature not enabled, falling back to stub mode");
                let executor = StubExecutor::new();
                let server = Server::new(server_config, executor)?
                    .with_profile_enabled(profile_enabled)
                    .with_access_log_enabled(access_log_enabled)
                    .with_rate_limiter(rate_limit_config);
                run_server(
                    server,
                    #[cfg(feature = "grpc")]
                    grpc_ctx.clone(),
                )
                .await
            }
        }
        ExecutorType::Php => {
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

                let server = Server::new(server_config, executor)?
                    .with_profile_enabled(profile_enabled)
                    .with_access_log_enabled(access_log_enabled)
                    .with_rate_limiter(rate_limit_config);
                run_server(
                    server,
                    #[cfg(feature = "grpc")]
                    grpc_ctx.clone(),
                )
                .await
            }

            #[cfg(not(feature = "php"))]
            {
                info!("PHP feature not enabled, falling back to stub mode");
                let executor = StubExecutor::new();
                let server = Server::new(server_config, executor)?
                    .with_profile_enabled(profile_enabled)
                    .with_access_log_enabled(access_log_enabled)
                    .with_rate_limiter(rate_limit_config);
                run_server(
                    server,
                    #[cfg(feature = "grpc")]
                    grpc_ctx.clone(),
                )
                .await
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

/// gRPC configuration passed to run_server.
#[cfg(feature = "grpc")]
#[derive(Clone)]
struct GrpcContext {
    addr: std::net::SocketAddr,
    tls_config: tokio_php::grpc::tls::GrpcTlsConfig,
}

async fn run_server<E: ScriptExecutor + 'static>(
    server: Server<E>,
    #[cfg(feature = "grpc")] grpc_ctx: Option<GrpcContext>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let drain_timeout = server.drain_timeout();

    // Start gRPC server if configured
    #[cfg(feature = "grpc")]
    let grpc_handle = if let Some(ctx) = grpc_ctx {
        let executor = server.executor();
        let health_checker = server.health_checker();
        let document_root = server.document_root().to_string();

        let grpc_server =
            GrpcServer::with_tls(ctx.addr, executor, health_checker, document_root, ctx.tls_config);

        Some(tokio::spawn(async move {
            if let Err(e) = grpc_server.run().await {
                tracing::error!("gRPC server error: {}", e);
            }
        }))
    } else {
        None
    };

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

    // Abort gRPC server if running
    #[cfg(feature = "grpc")]
    if let Some(handle) = grpc_handle {
        handle.abort();
    }

    // Cleanup PHP workers
    server.shutdown();
    info!("Shutdown complete");

    Ok(())
}
