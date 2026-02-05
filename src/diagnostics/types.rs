use serde::{Deserialize, Serialize};
use super::analyzer::Bottleneck;
use super::os::limits::OsLimits;
use super::recommender::Recommendation;
use super::runtime::{tokio_metrics::TokioMetrics, worker_stats::*};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResponse {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub platform: PlatformInfo,
    pub os_limits: OsLimits,
    pub runtime_metrics: RuntimeMetrics,
    pub bottlenecks: Vec<Bottleneck>,
    pub recommendations: Vec<Recommendation>,
    pub health_score: u8,
    pub collection_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub container: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgroup_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    pub tokio: TokioMetrics,
    pub php_workers: PhpWorkerStats,
    pub memory: MemoryStats,
    pub locks: LockStats,
}

impl DiagnosticResponse {
    /// Calculate overall health score (0-100)
    pub fn calculate_health_score(
        os_limits: &OsLimits,
        runtime_metrics: &RuntimeMetrics,
        bottlenecks: &[Bottleneck],
    ) -> u8 {
        let mut score = 100u8;

        // Deduct points for critical issues
        for bottleneck in bottlenecks {
            use super::analyzer::Severity;
            match bottleneck.severity {
                Severity::Critical => score = score.saturating_sub(20),
                Severity::Warning => score = score.saturating_sub(10),
                Severity::Info => score = score.saturating_sub(5),
            }
        }

        // Deduct points based on resource utilization
        if let Some(container) = &os_limits.container {
            if container.memory_utilization_pct > 90.0 {
                score = score.saturating_sub(15);
            } else if container.memory_utilization_pct > 75.0 {
                score = score.saturating_sub(8);
            }
        }

        // Deduct points for file descriptor usage
        if os_limits.process.open_files.utilization_pct > 80.0 {
            score = score.saturating_sub(10);
        }

        // Deduct points for worker saturation
        let worker_utilization = if runtime_metrics.php_workers.count > 0 {
            (runtime_metrics.php_workers.busy as f64 / runtime_metrics.php_workers.count as f64) * 100.0
        } else {
            0.0
        };

        if worker_utilization > 90.0 && runtime_metrics.php_workers.queue_depth > 5 {
            score = score.saturating_sub(12);
        }

        // Ensure score is in valid range
        score.max(0).min(100)
    }
}
