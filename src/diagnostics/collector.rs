use anyhow::Result;
use std::time::Instant;
use super::analyzer::PerformanceAnalyzer;
use super::os;
use super::recommender::RecommendationEngine;
use super::runtime::{tokio_metrics, worker_stats::*};
use super::types::{DiagnosticResponse, PlatformInfo, RuntimeMetrics};

pub struct DiagnosticCollector {
    platform: String,
}

impl DiagnosticCollector {
    pub fn new() -> Self {
        let platform = if cfg!(target_os = "linux") {
            "linux".to_string()
        } else if cfg!(target_os = "macos") {
            "darwin".to_string()
        } else {
            "unknown".to_string()
        };

        Self { platform }
    }

    /// Collect full diagnostics
    /// This is the main entry point called by the /diagnostics endpoint
    pub async fn collect(
        &self,
        runtime_handle: &tokio::runtime::Handle,
        // These would come from your existing app state
        worker_count: usize,
        busy_workers: usize,
        queue_depth: usize,
        total_requests: u64,
        execution_times_ms: Vec<f64>,
        wait_times_ms: Vec<f64>,
        php_memory_per_worker: Vec<u64>,
        file_cache_size: u64,
        // Lock contention metrics (you'd need to instrument these)
        worker_pool_wait_ns: u64,
        worker_pool_hold_ns: u64,
        file_cache_wait_ns: u64,
        file_cache_hold_ns: u64,
        config_wait_ns: u64,
        config_hold_ns: u64,
    ) -> Result<DiagnosticResponse> {
        let start = Instant::now();

        // Collect platform info
        let platform_info = self.collect_platform_info().await?;

        // Collect OS limits
        let os_limits = self.collect_os_limits()?;

        // Collect runtime metrics
        let tokio_metrics = tokio_metrics::collect_tokio_metrics_from_handle(runtime_handle);

        let worker_stats = worker_stats::collect_worker_stats(
            worker_count,
            busy_workers,
            queue_depth,
            total_requests,
            &execution_times_ms,
            &wait_times_ms,
        );

        let memory_stats = worker_stats::collect_memory_stats(
            worker_count,
            php_memory_per_worker,
            file_cache_size,
        );

        let lock_stats = worker_stats::collect_lock_stats(
            worker_pool_wait_ns,
            worker_pool_hold_ns,
            file_cache_wait_ns,
            file_cache_hold_ns,
            config_wait_ns,
            config_hold_ns,
        );

        let runtime_metrics = RuntimeMetrics {
            tokio: tokio_metrics,
            php_workers: worker_stats,
            memory: memory_stats,
            locks: lock_stats,
        };

        // Analyze bottlenecks
        let bottlenecks = PerformanceAnalyzer::analyze(
            &os_limits,
            &runtime_metrics.tokio,
            &runtime_metrics.php_workers,
            &runtime_metrics.memory,
            &runtime_metrics.locks,
        );

        // Generate recommendations
        let recommendations = RecommendationEngine::generate(
            &bottlenecks,
            &os_limits,
            &runtime_metrics.tokio,
            &runtime_metrics.php_workers,
            &self.platform,
        );

        // Calculate health score
        let health_score = DiagnosticResponse::calculate_health_score(
            &os_limits,
            &runtime_metrics,
            &bottlenecks,
        );

        let collection_time_ms = start.elapsed().as_millis() as u64;

        Ok(DiagnosticResponse {
            timestamp: chrono::Utc::now(),
            platform: platform_info,
            os_limits,
            runtime_metrics,
            bottlenecks,
            recommendations,
            health_score,
            collection_time_ms,
        })
    }

    fn collect_os_limits(&self) -> Result<os::limits::OsLimits> {
        #[cfg(target_os = "linux")]
        {
            os::linux::collect_os_limits()
        }

        #[cfg(target_os = "macos")]
        {
            os::macos::collect_os_limits()
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            anyhow::bail!("Unsupported platform for OS limits collection")
        }
    }

    async fn collect_platform_info(&self) -> Result<PlatformInfo> {
        use sysinfo::{System, SystemExt};

        let mut sys = System::new_all();
        sys.refresh_all();

        let os = sys.name().unwrap_or_else(|| "unknown".to_string());
        let kernel = sys.kernel_version().unwrap_or_else(|| "unknown".to_string());
        let arch = std::env::consts::ARCH.to_string();

        // Detect if running in container
        let container = self.detect_container();

        // Detect cgroup version on Linux
        let cgroup_version = if cfg!(target_os = "linux") {
            self.detect_cgroup_version()
        } else {
            None
        };

        Ok(PlatformInfo {
            os,
            kernel,
            arch,
            container,
            cgroup_version,
        })
    }

    fn detect_container(&self) -> String {
        #[cfg(target_os = "linux")]
        {
            // Check for Docker
            if std::path::Path::new("/.dockerenv").exists() {
                return "docker".to_string();
            }

            // Check for Kubernetes
            if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
                return "kubernetes".to_string();
            }

            // Check cgroup for container runtime
            if let Ok(cgroup) = std::fs::read_to_string("/proc/self/cgroup") {
                if cgroup.contains("docker") {
                    return "docker".to_string();
                }
                if cgroup.contains("kubepods") {
                    return "kubernetes".to_string();
                }
                if cgroup.contains("containerd") {
                    return "containerd".to_string();
                }
            }
        }

        "none".to_string()
    }

    fn detect_cgroup_version(&self) -> Option<String> {
        #[cfg(target_os = "linux")]
        {
            if std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
                return Some("v2".to_string());
            } else if std::path::Path::new("/sys/fs/cgroup/memory").exists() {
                return Some("v1".to_string());
            }
        }

        None
    }
}

impl Default for DiagnosticCollector {
    fn default() -> Self {
        Self::new()
    }
}
