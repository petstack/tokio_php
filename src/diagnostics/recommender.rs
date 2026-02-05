use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::analyzer::{Bottleneck, Category, Severity};
use super::os::limits::OsLimits;
use super::runtime::{tokio_metrics::TokioMetrics, worker_stats::*};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub priority: Priority,
    pub category: String,
    pub issue: String,
    pub action: String,
    pub commands: Commands,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub expected_impact: String,
    pub estimated_gain_pct: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commands {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub immediate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
}

pub struct RecommendationEngine;

impl RecommendationEngine {
    pub fn generate(
        bottlenecks: &[Bottleneck],
        os_limits: &OsLimits,
        tokio_metrics: &TokioMetrics,
        worker_stats: &PhpWorkerStats,
        platform: &str,
    ) -> Vec<Recommendation> {
        let mut recommendations = Vec::new();

        for bottleneck in bottlenecks {
            if let Some(rec) = Self::bottleneck_to_recommendation(
                bottleneck,
                os_limits,
                tokio_metrics,
                worker_stats,
                platform,
            ) {
                recommendations.push(rec);
            }
        }

        // Add proactive recommendations even without bottlenecks
        recommendations.extend(Self::proactive_recommendations(
            os_limits,
            tokio_metrics,
            worker_stats,
            platform,
        ));

        // Sort by priority
        recommendations.sort_by(|a, b| {
            let a_val = match a.priority {
                Priority::Critical => 4,
                Priority::High => 3,
                Priority::Medium => 2,
                Priority::Low => 1,
            };
            let b_val = match b.priority {
                Priority::Critical => 4,
                Priority::High => 3,
                Priority::Medium => 2,
                Priority::Low => 1,
            };
            b_val.cmp(&a_val)
        });

        recommendations
    }

    fn bottleneck_to_recommendation(
        bottleneck: &Bottleneck,
        os_limits: &OsLimits,
        tokio_metrics: &TokioMetrics,
        worker_stats: &PhpWorkerStats,
        platform: &str,
    ) -> Option<Recommendation> {
        match (&bottleneck.category, bottleneck.metric.as_str()) {
            (Category::Network, "somaxconn") => {
                Some(Self::recommend_somaxconn(bottleneck, platform))
            }
            (Category::Process, "open_files") => {
                Some(Self::recommend_open_files(bottleneck, platform))
            }
            (Category::Memory, "container_memory") => {
                Some(Self::recommend_container_memory(bottleneck))
            }
            (Category::Runtime, "max_poll_time") => {
                Some(Self::recommend_poll_time(bottleneck))
            }
            (Category::Runtime, "task_queue_depth") => {
                Some(Self::recommend_task_queue(bottleneck))
            }
            (Category::Workers, "worker_saturation") => {
                Some(Self::recommend_worker_count(bottleneck, worker_stats))
            }
            (Category::Workers, "php_execution_time") => {
                Some(Self::recommend_php_optimization(bottleneck))
            }
            (Category::Memory, "php_worker_memory") => {
                Some(Self::recommend_php_memory(bottleneck))
            }
            (Category::Locks, _) => {
                Some(Self::recommend_lock_optimization(bottleneck))
            }
            _ => None,
        }
    }

    fn recommend_somaxconn(bottleneck: &Bottleneck, platform: &str) -> Recommendation {
        let threshold = bottleneck.threshold.unwrap_or(8192);

        let (immediate, persistent) = match platform {
            "linux" => (
                format!("sysctl -w net.core.somaxconn={}", threshold),
                format!("echo 'net.core.somaxconn = {}' >> /etc/sysctl.conf && sysctl -p", threshold),
            ),
            "darwin" => (
                format!("sudo sysctl -w kern.ipc.somaxconn={}", threshold),
                format!("echo 'kern.ipc.somaxconn={}' | sudo tee -a /etc/sysctl.conf", threshold),
            ),
            _ => (String::new(), String::new()),
        };

        Recommendation {
            priority: match bottleneck.severity {
                Severity::Critical => Priority::Critical,
                Severity::Warning => Priority::High,
                _ => Priority::Medium,
            },
            category: "network".to_string(),
            issue: "TCP listen backlog below recommended value for high-throughput server".to_string(),
            action: "increase_somaxconn".to_string(),
            commands: Commands {
                immediate: Some(immediate),
                persistent: Some(persistent),
                docker: None,
                env: None,
            },
            rationale: Some(format!(
                "Current value {} is below recommended {} for async servers handling high connection rates",
                bottleneck.current.unwrap_or(0),
                threshold
            )),
            expected_impact: format!(
                "Prevent connection drops during traffic spikes, support up to {} pending connections",
                threshold
            ),
            estimated_gain_pct: 15,
        }
    }

    fn recommend_open_files(bottleneck: &Bottleneck, platform: &str) -> Recommendation {
        let recommended = 65536;

        let (immediate, persistent) = match platform {
            "linux" => (
                "ulimit -n 65536".to_string(),
                "echo '* soft nofile 65536' >> /etc/security/limits.conf && echo '* hard nofile 65536' >> /etc/security/limits.conf".to_string(),
            ),
            "darwin" => (
                "ulimit -n 65536".to_string(),
                "sudo launchctl limit maxfiles 65536 200000".to_string(),
            ),
            _ => (String::new(), String::new()),
        };

        Recommendation {
            priority: Priority::High,
            category: "process".to_string(),
            issue: format!(
                "File descriptor limit at {:.0}% capacity",
                (bottleneck.current.unwrap_or(0) as f64 / bottleneck.threshold.unwrap_or(1) as f64) * 100.0
            ),
            action: "increase_nofile".to_string(),
            commands: Commands {
                immediate: Some(immediate),
                persistent: Some(persistent),
                docker: Some("Add to docker-compose.yml: ulimits: nofile: {soft: 65536, hard: 65536}".to_string()),
                env: None,
            },
            rationale: Some("Async servers need high file descriptor limits for many concurrent connections".to_string()),
            expected_impact: "Support up to 65536 concurrent connections without hitting limits".to_string(),
            estimated_gain_pct: 20,
        }
    }

    fn recommend_container_memory(bottleneck: &Bottleneck) -> Recommendation {
        let current = bottleneck.current.unwrap_or(0);
        let limit = bottleneck.threshold.unwrap_or(0);
        let recommended = limit * 2;

        Recommendation {
            priority: Priority::Critical,
            category: "memory".to_string(),
            issue: format!(
                "Container memory usage at {:.0}%",
                (current as f64 / limit as f64) * 100.0
            ),
            action: "increase_memory_limit".to_string(),
            commands: Commands {
                immediate: None,
                persistent: None,
                docker: Some(format!(
                    "Update docker-compose.yml: mem_limit: {}m",
                    recommended / 1024 / 1024
                )),
                env: None,
            },
            rationale: Some("OOM killer will terminate process if memory limit is exceeded".to_string()),
            expected_impact: "Prevent OOM kills and allow for traffic growth".to_string(),
            estimated_gain_pct: 30,
        }
    }

    fn recommend_poll_time(_bottleneck: &Bottleneck) -> Recommendation {
        Recommendation {
            priority: Priority::High,
            category: "runtime".to_string(),
            issue: "Slow async tasks blocking Tokio executor".to_string(),
            action: "optimize_blocking_code".to_string(),
            commands: Commands {
                immediate: None,
                persistent: None,
                docker: None,
                env: Some("Enable task profiling: RUST_LOG=tokio=trace".to_string()),
            },
            rationale: Some("Tasks taking >50ms block the executor and reduce throughput".to_string()),
            expected_impact: "Move blocking operations to spawn_blocking or optimize PHP scripts".to_string(),
            estimated_gain_pct: 25,
        }
    }

    fn recommend_task_queue(_bottleneck: &Bottleneck) -> Recommendation {
        Recommendation {
            priority: Priority::High,
            category: "runtime".to_string(),
            issue: "High task backlog in Tokio scheduler".to_string(),
            action: "increase_tokio_workers".to_string(),
            commands: Commands {
                immediate: None,
                persistent: None,
                docker: None,
                env: Some("Set TOKIO_WORKER_THREADS to match CPU count".to_string()),
            },
            rationale: Some("Executor saturation causes increased latency".to_string()),
            expected_impact: "Reduce task scheduling latency and improve throughput".to_string(),
            estimated_gain_pct: 20,
        }
    }

    fn recommend_worker_count(
        bottleneck: &Bottleneck,
        worker_stats: &PhpWorkerStats,
    ) -> Recommendation {
        let utilization_pct = if worker_stats.count > 0 {
            (worker_stats.busy as f64 / worker_stats.count as f64) * 100.0
        } else {
            0.0
        };

        // Recommend 20-30% more workers
        let recommended = (worker_stats.count as f64 * 1.25).ceil() as usize;

        Recommendation {
            priority: if utilization_pct > 90.0 {
                Priority::Critical
            } else {
                Priority::High
            },
            category: "configuration".to_string(),
            issue: format!(
                "PHP workers at {:.0}% utilization with {} requests queued",
                utilization_pct, worker_stats.queue_depth
            ),
            action: "adjust_worker_count".to_string(),
            commands: Commands {
                immediate: None,
                persistent: None,
                docker: Some(format!("docker run -e PHP_WORKERS={} ...", recommended)),
                env: Some(format!("export PHP_WORKERS={}", recommended)),
            },
            rationale: Some(format!(
                "Average queue wait time {:.1}ms suggests more workers could improve throughput",
                worker_stats.avg_wait_time_ms
            )),
            expected_impact: format!(
                "Reduce request latency by ~30%, increase throughput by ~{}%",
                ((recommended - worker_stats.count) * 100 / worker_stats.count)
            ),
            estimated_gain_pct: 20,
        }
    }

    fn recommend_php_optimization(_bottleneck: &Bottleneck) -> Recommendation {
        Recommendation {
            priority: Priority::Medium,
            category: "application".to_string(),
            issue: "Slow PHP script execution detected".to_string(),
            action: "optimize_php_code".to_string(),
            commands: Commands {
                immediate: None,
                persistent: None,
                docker: None,
                env: Some("Enable OPcache: opcache.enable=1, opcache.jit=tracing".to_string()),
            },
            rationale: Some("P99 execution time >1s indicates expensive PHP operations".to_string()),
            expected_impact: "Profile and optimize slow endpoints, enable JIT compilation".to_string(),
            estimated_gain_pct: 40,
        }
    }

    fn recommend_php_memory(_bottleneck: &Bottleneck) -> Recommendation {
        Recommendation {
            priority: Priority::Medium,
            category: "memory".to_string(),
            issue: "High per-worker PHP memory usage".to_string(),
            action: "investigate_memory_leak".to_string(),
            commands: Commands {
                immediate: None,
                persistent: None,
                docker: None,
                env: Some("Reduce memory_limit in php.ini or enable garbage collection".to_string()),
            },
            rationale: Some("Workers using >100MB may indicate memory leaks or inefficient code".to_string()),
            expected_impact: "Profile memory usage, fix leaks, or reduce worker lifetime".to_string(),
            estimated_gain_pct: 15,
        }
    }

    fn recommend_lock_optimization(bottleneck: &Bottleneck) -> Recommendation {
        Recommendation {
            priority: Priority::Medium,
            category: "concurrency".to_string(),
            issue: format!("High lock contention on {}", bottleneck.metric),
            action: "reduce_lock_contention".to_string(),
            commands: Commands {
                immediate: None,
                persistent: None,
                docker: None,
                env: None,
            },
            rationale: Some("Lock contention >10% reduces concurrency benefits".to_string()),
            expected_impact: "Use lock-free data structures or reduce critical section size".to_string(),
            estimated_gain_pct: 10,
        }
    }

    fn proactive_recommendations(
        os_limits: &OsLimits,
        tokio_metrics: &TokioMetrics,
        worker_stats: &PhpWorkerStats,
        platform: &str,
    ) -> Vec<Recommendation> {
        let mut recommendations = Vec::new();

        // Recommend TCP tuning for production
        if os_limits.network.tcp_max_syn_backlog < 8192 {
            recommendations.push(Recommendation {
                priority: Priority::Low,
                category: "network".to_string(),
                issue: "TCP SYN backlog below optimal for production".to_string(),
                action: "tune_tcp_backlog".to_string(),
                commands: Commands {
                    immediate: if platform == "linux" {
                        Some("sysctl -w net.ipv4.tcp_max_syn_backlog=8192".to_string())
                    } else {
                        None
                    },
                    persistent: if platform == "linux" {
                        Some("echo 'net.ipv4.tcp_max_syn_backlog = 8192' >> /etc/sysctl.conf".to_string())
                    } else {
                        None
                    },
                    docker: None,
                    env: None,
                },
                rationale: None,
                expected_impact: "Better handling of SYN flood and connection bursts".to_string(),
                estimated_gain_pct: 5,
            });
        }

        recommendations
    }
}
