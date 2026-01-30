//! Cgroup detection and resource limit parsing.
//!
//! Supports both cgroup v1 and v2 for detecting CPU and memory limits
//! in containerized environments (Docker, Kubernetes).

use std::fs;
use std::path::Path;

use tracing::{debug, trace};

/// Cgroup version detected on the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupVersion {
    /// cgroup v2 (unified hierarchy)
    V2,
    /// cgroup v1 (legacy hierarchy)
    V1,
    /// No cgroup detected (bare metal or unsupported)
    None,
}

impl CgroupVersion {
    /// Detect the cgroup version on the current system.
    pub fn detect() -> Self {
        // Check cgroup v2 first (unified hierarchy)
        if Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
            debug!("Detected cgroup v2 (unified hierarchy)");
            return Self::V2;
        }

        // Check cgroup v1 (legacy hierarchy)
        if Path::new("/sys/fs/cgroup/memory/memory.limit_in_bytes").exists()
            || Path::new("/sys/fs/cgroup/cpu/cpu.cfs_quota_us").exists()
        {
            debug!("Detected cgroup v1 (legacy hierarchy)");
            return Self::V1;
        }

        debug!("No cgroup detected");
        Self::None
    }
}

impl std::fmt::Display for CgroupVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V2 => write!(f, "v2"),
            Self::V1 => write!(f, "v1"),
            Self::None => write!(f, "none"),
        }
    }
}

/// Resource limits detected from cgroup.
#[derive(Debug, Clone, Default)]
pub struct ResourceLimits {
    /// Cgroup version detected
    pub cgroup_version: Option<CgroupVersion>,
    /// Memory limit in bytes (None = unlimited)
    pub memory_limit: Option<usize>,
    /// CPU quota as fraction (e.g., 2.0 = 2 CPUs)
    pub cpu_quota: Option<f64>,
    /// PIDs limit
    pub pids_max: Option<u32>,
}

impl ResourceLimits {
    /// Read resource limits from cgroups.
    ///
    /// Automatically detects cgroup version and reads appropriate files.
    pub fn from_cgroup() -> Self {
        let version = CgroupVersion::detect();
        match version {
            CgroupVersion::V2 => Self::from_cgroup_v2(),
            CgroupVersion::V1 => Self::from_cgroup_v1(),
            CgroupVersion::None => Self::default(),
        }
    }

    /// Read limits from cgroup v2.
    fn from_cgroup_v2() -> Self {
        let mut limits = Self {
            cgroup_version: Some(CgroupVersion::V2),
            ..Default::default()
        };

        // Memory limit: /sys/fs/cgroup/memory.max
        // Format: bytes or "max" (unlimited)
        if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/memory.max") {
            let trimmed = content.trim();
            if trimmed != "max" {
                if let Ok(value) = trimmed.parse::<usize>() {
                    limits.memory_limit = Some(value);
                    trace!("cgroup v2 memory.max: {} bytes", value);
                }
            }
        }

        // CPU quota: /sys/fs/cgroup/cpu.max
        // Format: "$MAX $PERIOD" or "max $PERIOD"
        if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/cpu.max") {
            let parts: Vec<&str> = content.split_whitespace().collect();
            if parts.len() == 2 && parts[0] != "max" {
                if let (Ok(max), Ok(period)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    if period > 0.0 {
                        limits.cpu_quota = Some(max / period);
                        trace!(
                            "cgroup v2 cpu.max: {}/{} = {:.2} CPUs",
                            max,
                            period,
                            max / period
                        );
                    }
                }
            }
        }

        // PIDs limit: /sys/fs/cgroup/pids.max
        // Format: number or "max"
        if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/pids.max") {
            let trimmed = content.trim();
            if trimmed != "max" {
                if let Ok(value) = trimmed.parse::<u32>() {
                    limits.pids_max = Some(value);
                    trace!("cgroup v2 pids.max: {}", value);
                }
            }
        }

        limits
    }

    /// Read limits from cgroup v1.
    fn from_cgroup_v1() -> Self {
        let mut limits = Self {
            cgroup_version: Some(CgroupVersion::V1),
            ..Default::default()
        };

        // Memory limit: /sys/fs/cgroup/memory/memory.limit_in_bytes
        // Very large value (>9 quintillion) means unlimited
        if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes") {
            if let Ok(value) = content.trim().parse::<u64>() {
                // Check for "unlimited" (very large value, typically ~9 exabytes)
                if value < 9_000_000_000_000_000_000 {
                    limits.memory_limit = Some(value as usize);
                    trace!("cgroup v1 memory.limit_in_bytes: {} bytes", value);
                }
            }
        }

        // CPU quota: cpu.cfs_quota_us / cpu.cfs_period_us
        // Negative quota means unlimited
        let quota = fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok());

        let period = fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us")
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok());

        if let (Some(q), Some(p)) = (quota, period) {
            if q > 0 && p > 0.0 {
                limits.cpu_quota = Some(q as f64 / p);
                trace!(
                    "cgroup v1 cpu quota: {}/{} = {:.2} CPUs",
                    q,
                    p,
                    q as f64 / p
                );
            }
        }

        // PIDs limit: /sys/fs/cgroup/pids/pids.max
        if let Ok(content) = fs::read_to_string("/sys/fs/cgroup/pids/pids.max") {
            let trimmed = content.trim();
            if trimmed != "max" {
                if let Ok(value) = trimmed.parse::<u32>() {
                    limits.pids_max = Some(value);
                    trace!("cgroup v1 pids.max: {}", value);
                }
            }
        }

        limits
    }

    /// Calculate optimal worker count based on CPU quota.
    ///
    /// Returns the ceiling of the CPU quota if set, otherwise falls back
    /// to the number of CPUs detected by num_cpus.
    ///
    /// # Example
    ///
    /// - CPU quota 2.5 → 3 workers
    /// - CPU quota 1.0 → 1 worker
    /// - No quota → num_cpus::get()
    pub fn optimal_workers(&self) -> usize {
        if let Some(quota) = self.cpu_quota {
            let workers = (quota.ceil() as usize).max(1);
            debug!(
                "Auto-tuned workers: {} (cgroup CPU quota: {:.2})",
                workers, quota
            );
            workers
        } else {
            let cpus = num_cpus::get();
            debug!(
                "Auto-tuned workers: {} (no cgroup limit, using num_cpus)",
                cpus
            );
            cpus
        }
    }

    /// Calculate optimal queue capacity.
    ///
    /// Returns workers * multiplier, respecting any PIDs limit.
    pub fn optimal_queue_capacity(&self, workers: usize, multiplier: usize) -> usize {
        let base = workers * multiplier;

        // If PIDs limit is set, ensure we don't exceed it
        if let Some(pids_max) = self.pids_max {
            let max_safe = (pids_max as usize).saturating_sub(workers + 10); // Leave room for main + overhead
            base.min(max_safe)
        } else {
            base
        }
    }

    /// Get memory limit in human-readable format.
    pub fn memory_limit_display(&self) -> String {
        match self.memory_limit {
            Some(bytes) => {
                if bytes >= 1_073_741_824 {
                    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
                } else if bytes >= 1_048_576 {
                    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
                } else {
                    format!("{} bytes", bytes)
                }
            }
            None => "unlimited".to_string(),
        }
    }

    /// Get CPU quota in human-readable format.
    pub fn cpu_quota_display(&self) -> String {
        match self.cpu_quota {
            Some(quota) => format!("{:.2} CPUs", quota),
            None => "unlimited".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimal_workers_with_quota() {
        let limits = ResourceLimits {
            cpu_quota: Some(2.5),
            ..Default::default()
        };
        assert_eq!(limits.optimal_workers(), 3);

        let limits = ResourceLimits {
            cpu_quota: Some(1.0),
            ..Default::default()
        };
        assert_eq!(limits.optimal_workers(), 1);

        let limits = ResourceLimits {
            cpu_quota: Some(0.5),
            ..Default::default()
        };
        assert_eq!(limits.optimal_workers(), 1); // Minimum 1 worker
    }

    #[test]
    fn test_memory_limit_display() {
        let limits = ResourceLimits {
            memory_limit: Some(1_073_741_824),
            ..Default::default()
        };
        assert_eq!(limits.memory_limit_display(), "1.0 GB");

        let limits = ResourceLimits {
            memory_limit: Some(536_870_912),
            ..Default::default()
        };
        assert_eq!(limits.memory_limit_display(), "512.0 MB");

        let limits = ResourceLimits::default();
        assert_eq!(limits.memory_limit_display(), "unlimited");
    }

    #[test]
    fn test_cpu_quota_display() {
        let limits = ResourceLimits {
            cpu_quota: Some(2.5),
            ..Default::default()
        };
        assert_eq!(limits.cpu_quota_display(), "2.50 CPUs");

        let limits = ResourceLimits::default();
        assert_eq!(limits.cpu_quota_display(), "unlimited");
    }
}
