use serde::{Deserialize, Serialize};
use super::os::limits::{LimitStatus, OsLimits};
use super::runtime::{tokio_metrics::TokioMetrics, worker_stats::*};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bottleneck {
    pub severity: Severity,
    pub category: Category,
    pub metric: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<u64>,
    pub impact: String,
    pub detected_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Network,
    Process,
    Memory,
    Io,
    Runtime,
    Workers,
    Locks,
}

pub struct PerformanceAnalyzer;

impl PerformanceAnalyzer {
    pub fn analyze(
        os_limits: &OsLimits,
        tokio_metrics: &TokioMetrics,
        worker_stats: &PhpWorkerStats,
        memory_stats: &MemoryStats,
        lock_stats: &LockStats,
    ) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();
        let now = chrono::Utc::now();

        // Analyze OS limits
        bottlenecks.extend(Self::analyze_os_limits(os_limits, now));

        // Analyze runtime
        bottlenecks.extend(Self::analyze_tokio_runtime(tokio_metrics, now));

        // Analyze workers
        bottlenecks.extend(Self::analyze_workers(worker_stats, now));

        // Analyze memory
        bottlenecks.extend(Self::analyze_memory(memory_stats, now));

        // Analyze lock contention
        bottlenecks.extend(Self::analyze_locks(lock_stats, now));

        bottlenecks
    }

    fn analyze_os_limits(
        limits: &OsLimits,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        // Check network limits
        if matches!(limits.network.somaxconn.status, LimitStatus::Warning | LimitStatus::Critical) {
            bottlenecks.push(Bottleneck {
                severity: match limits.network.somaxconn.status {
                    LimitStatus::Critical => Severity::Critical,
                    _ => Severity::Warning,
                },
                category: Category::Network,
                metric: "somaxconn".to_string(),
                current: Some(limits.network.somaxconn.value),
                threshold: Some(limits.network.somaxconn.recommended),
                impact: "Connection requests may be dropped during traffic spikes".to_string(),
                detected_at: now,
            });
        }

        // Check file descriptor limits
        if matches!(limits.process.open_files.status, LimitStatus::Warning | LimitStatus::Critical) {
            bottlenecks.push(Bottleneck {
                severity: match limits.process.open_files.status {
                    LimitStatus::Critical => Severity::Critical,
                    _ => Severity::Warning,
                },
                category: Category::Process,
                metric: "open_files".to_string(),
                current: Some(limits.process.open_files.current),
                threshold: Some(limits.process.open_files.soft),
                impact: format!(
                    "File descriptor usage at {:.1}%, may hit limit under load",
                    limits.process.open_files.utilization_pct
                ),
                detected_at: now,
            });
        }

        // Check container limits
        if let Some(container) = &limits.container {
            if matches!(container.status, LimitStatus::Warning | LimitStatus::Critical) {
                bottlenecks.push(Bottleneck {
                    severity: match container.status {
                        LimitStatus::Critical => Severity::Critical,
                        _ => Severity::Warning,
                    },
                    category: Category::Memory,
                    metric: "container_memory".to_string(),
                    current: Some(container.memory_usage_bytes),
                    threshold: Some(container.memory_limit_bytes),
                    impact: format!(
                        "Container memory at {:.1}%, OOM killer may terminate process",
                        container.memory_utilization_pct
                    ),
                    detected_at: now,
                });
            }
        }

        bottlenecks
    }

    fn analyze_tokio_runtime(
        metrics: &TokioMetrics,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        // Check for slow poll times
        if metrics.max_poll_time_us > 50_000 {
            bottlenecks.push(Bottleneck {
                severity: if metrics.max_poll_time_us > 100_000 {
                    Severity::Critical
                } else {
                    Severity::Warning
                },
                category: Category::Runtime,
                metric: "max_poll_time".to_string(),
                current: Some(metrics.max_poll_time_us),
                threshold: Some(50_000),
                impact: format!(
                    "Slow async tasks blocking executor ({}ms max poll time)",
                    metrics.max_poll_time_us / 1000
                ),
                detected_at: now,
            });
        }

        // Check task queue depth
        if metrics.queue_depth > 100 {
            bottlenecks.push(Bottleneck {
                severity: if metrics.queue_depth > 500 {
                    Severity::Critical
                } else {
                    Severity::Warning
                },
                category: Category::Runtime,
                metric: "task_queue_depth".to_string(),
                current: Some(metrics.queue_depth),
                threshold: Some(100),
                impact: "High task backlog indicates executor saturation".to_string(),
                detected_at: now,
            });
        }

        bottlenecks
    }

    fn analyze_workers(
        stats: &PhpWorkerStats,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        // Check worker utilization and queue depth
        if matches!(stats.status, LimitStatus::Warning | LimitStatus::Critical) {
            let utilization_pct = if stats.count > 0 {
                (stats.busy as f64 / stats.count as f64) * 100.0
            } else {
                0.0
            };

            if utilization_pct > 70.0 && stats.queue_depth > 0 {
                bottlenecks.push(Bottleneck {
                    severity: if utilization_pct > 90.0 {
                        Severity::Critical
                    } else {
                        Severity::Warning
                    },
                    category: Category::Workers,
                    metric: "worker_saturation".to_string(),
                    current: Some(stats.busy as u64),
                    threshold: Some(stats.count as u64),
                    impact: format!(
                        "Workers at {:.0}% utilization with {} requests queued",
                        utilization_pct, stats.queue_depth
                    ),
                    detected_at: now,
                });
            }
        }

        // Check for slow PHP execution
        if stats.p99_execution_time_ms > 1000.0 {
            bottlenecks.push(Bottleneck {
                severity: Severity::Warning,
                category: Category::Workers,
                metric: "php_execution_time".to_string(),
                current: Some(stats.p99_execution_time_ms as u64),
                threshold: Some(1000),
                impact: format!(
                    "Slow PHP scripts detected (p99: {:.0}ms)",
                    stats.p99_execution_time_ms
                ),
                detected_at: now,
            });
        }

        bottlenecks
    }

    fn analyze_memory(
        stats: &MemoryStats,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        if matches!(stats.status, LimitStatus::Warning | LimitStatus::Critical) {
            bottlenecks.push(Bottleneck {
                severity: match stats.status {
                    LimitStatus::Critical => Severity::Critical,
                    _ => Severity::Warning,
                },
                category: Category::Memory,
                metric: "memory_usage".to_string(),
                current: Some(stats.rust_allocated_bytes + stats.total_php_memory_bytes),
                threshold: None,
                impact: "High memory usage may lead to swapping or OOM".to_string(),
                detected_at: now,
            });
        }

        // Check for PHP memory leaks (high per-worker memory)
        if stats.php_per_worker_max_bytes > 100 * 1024 * 1024 {
            bottlenecks.push(Bottleneck {
                severity: Severity::Warning,
                category: Category::Memory,
                metric: "php_worker_memory".to_string(),
                current: Some(stats.php_per_worker_max_bytes),
                threshold: Some(100 * 1024 * 1024),
                impact: "High per-worker PHP memory usage, possible memory leak".to_string(),
                detected_at: now,
            });
        }

        bottlenecks
    }

    fn analyze_locks(
        stats: &LockStats,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        if stats.worker_pool_contention_pct > 10.0 {
            bottlenecks.push(Bottleneck {
                severity: if stats.worker_pool_contention_pct > 20.0 {
                    Severity::Critical
                } else {
                    Severity::Warning
                },
                category: Category::Locks,
                metric: "worker_pool_contention".to_string(),
                current: Some((stats.worker_pool_contention_pct * 100.0) as u64),
                threshold: Some(1000), // 10%
                impact: format!(
                    "High worker pool lock contention ({:.1}%)",
                    stats.worker_pool_contention_pct
                ),
                detected_at: now,
            });
        }

        if stats.file_cache_contention_pct > 10.0 {
            bottlenecks.push(Bottleneck {
                severity: Severity::Warning,
                category: Category::Locks,
                metric: "file_cache_contention".to_string(),
                current: Some((stats.file_cache_contention_pct * 100.0) as u64),
                threshold: Some(1000),
                impact: format!(
                    "File cache lock contention ({:.1}%)",
                    stats.file_cache_contention_pct
                ),
                detected_at: now,
            });
        }

        bottlenecks
    }
}
