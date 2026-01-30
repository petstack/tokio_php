//! OpenTelemetry integration for distributed tracing.
//!
//! Provides W3C Trace Context propagation and OTLP export to
//! collectors like Jaeger, Zipkin, or Datadog.
//!
//! # Configuration
//!
//! Set environment variables to configure:
//! - `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP gRPC endpoint (default: `http://localhost:4317`)
//! - `OTEL_SERVICE_NAME`: Service name in traces (default: `tokio_php`)
//! - `OTEL_SERVICE_VERSION`: Service version (default: from Cargo.toml)
//! - `OTEL_ENVIRONMENT`: Deployment environment (default: `development`)
//! - `OTEL_SAMPLING_RATIO`: Sampling ratio 0.0-1.0 (default: `1.0`)
//! - `OTEL_ENABLED`: Enable OpenTelemetry (`1` = enabled)
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::observability::{init_tracing, shutdown_tracing, OtelConfig};
//!
//! // Load config from environment
//! let config = OtelConfig::from_env();
//!
//! // Initialize tracing
//! init_tracing(&config)?;
//!
//! // ... your application code ...
//!
//! // Shutdown (flush pending spans)
//! shutdown_tracing();
//! ```

use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    runtime,
    trace::{Config, Sampler},
    Resource,
};
use std::time::Duration;
use tracing::info;

// Semantic convention keys (avoiding dependency on semconv_experimental feature)
const SERVICE_NAME: &str = "service.name";
const SERVICE_VERSION: &str = "service.version";
const DEPLOYMENT_ENVIRONMENT: &str = "deployment.environment";

/// OpenTelemetry configuration.
#[derive(Debug, Clone)]
pub struct OtelConfig {
    /// OTLP endpoint (e.g., "http://jaeger:4317")
    pub endpoint: String,
    /// Service name
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Deployment environment (production, staging, etc.)
    pub environment: String,
    /// Sampling ratio (0.0 - 1.0, 1.0 = sample all)
    pub sampling_ratio: f64,
    /// Export timeout in seconds
    pub export_timeout_secs: u64,
    /// Batch export size
    pub batch_size: usize,
    /// Max queue size
    pub max_queue_size: usize,
    /// Whether OpenTelemetry is enabled
    pub enabled: bool,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".into(),
            service_name: "tokio_php".into(),
            service_version: env!("CARGO_PKG_VERSION").into(),
            environment: "development".into(),
            sampling_ratio: 1.0,
            export_timeout_secs: 10,
            batch_size: 512,
            max_queue_size: 2048,
            enabled: false,
        }
    }
}

impl OtelConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let enabled = std::env::var("OTEL_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        Self {
            endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".into()),
            service_name: std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "tokio_php".into()),
            service_version: std::env::var("OTEL_SERVICE_VERSION")
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into()),
            environment: std::env::var("OTEL_ENVIRONMENT").unwrap_or_else(|_| "development".into()),
            sampling_ratio: std::env::var("OTEL_SAMPLING_RATIO")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.0),
            export_timeout_secs: std::env::var("OTEL_EXPORT_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            batch_size: std::env::var("OTEL_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(512),
            max_queue_size: std::env::var("OTEL_MAX_QUEUE_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2048),
            enabled,
        }
    }

    /// Check if OpenTelemetry is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Initialize OpenTelemetry tracing.
///
/// This sets up the OTLP exporter and configures the global tracer provider.
/// Call `shutdown_tracing()` before process exit to flush pending spans.
///
/// # Errors
///
/// Returns an error if the OTLP connection fails or configuration is invalid.
pub fn init_tracing(config: &OtelConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !config.enabled {
        info!("OpenTelemetry disabled (OTEL_ENABLED != 1)");
        return Ok(());
    }

    // Build resource with service attributes
    let resource = Resource::new([
        KeyValue::new(SERVICE_NAME, config.service_name.clone()),
        KeyValue::new(SERVICE_VERSION, config.service_version.clone()),
        KeyValue::new(DEPLOYMENT_ENVIRONMENT, config.environment.clone()),
    ]);

    // Configure OTLP exporter
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .with_timeout(Duration::from_secs(config.export_timeout_secs))
        .build()?;

    // Build tracer provider with batching
    let tracer_provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_batch_exporter(exporter, runtime::Tokio)
        .with_config(
            Config::default()
                .with_resource(resource)
                .with_sampler(Sampler::TraceIdRatioBased(config.sampling_ratio)),
        )
        .build();

    // Set global tracer provider
    global::set_tracer_provider(tracer_provider);

    info!(
        endpoint = %config.endpoint,
        service = %config.service_name,
        version = %config.service_version,
        environment = %config.environment,
        sampling = %config.sampling_ratio,
        "OpenTelemetry tracing initialized"
    );

    Ok(())
}

/// Shutdown OpenTelemetry tracing.
///
/// This flushes any pending spans to the collector.
/// Should be called before process exit.
pub fn shutdown_tracing() {
    global::shutdown_tracer_provider();
    info!("OpenTelemetry tracing shutdown complete");
}

/// Get the global tracer for creating spans.
pub fn tracer() -> impl opentelemetry::trace::Tracer {
    global::tracer("tokio_php")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = OtelConfig::default();
        assert_eq!(config.endpoint, "http://localhost:4317");
        assert_eq!(config.service_name, "tokio_php");
        assert_eq!(config.sampling_ratio, 1.0);
        assert!(!config.enabled);
    }

    #[test]
    fn test_config_from_env() {
        // Clear env vars first
        std::env::remove_var("OTEL_ENABLED");
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");

        let config = OtelConfig::from_env();
        assert!(!config.is_enabled());

        // Set enabled
        std::env::set_var("OTEL_ENABLED", "1");
        let config = OtelConfig::from_env();
        assert!(config.is_enabled());

        // Clean up
        std::env::remove_var("OTEL_ENABLED");
    }
}
