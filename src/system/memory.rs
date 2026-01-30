//! Memory monitoring and pressure detection.
//!
//! Monitors memory usage and provides pressure levels for backpressure.

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{debug, info, warn};

use super::cgroup::{CgroupVersion, ResourceLimits};

/// Memory pressure levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// Normal operation (< 70% usage)
    None,
    /// Low pressure (70-80% usage)
    Low,
    /// Medium pressure (80-90% usage)
    Medium,
    /// High pressure (90-95% usage) - reduce queue capacity
    High,
    /// Critical pressure (> 95% usage) - reject new requests
    Critical,
}

impl std::fmt::Display for MemoryPressure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Memory monitor that tracks usage and pressure.
pub struct MemoryMonitor {
    /// Resource limits from cgroup
    limits: ResourceLimits,
    /// Pressure level sender
    pressure_tx: watch::Sender<MemoryPressure>,
    /// Pressure level receiver (for cloning)
    pressure_rx: watch::Receiver<MemoryPressure>,
    /// Running flag
    running: Arc<AtomicBool>,
}

impl MemoryMonitor {
    /// Create a new memory monitor.
    pub fn new() -> Self {
        let limits = ResourceLimits::from_cgroup();
        let (pressure_tx, pressure_rx) = watch::channel(MemoryPressure::None);

        Self {
            limits,
            pressure_tx,
            pressure_rx,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create with specific limits (for testing).
    pub fn with_limits(limits: ResourceLimits) -> Self {
        let (pressure_tx, pressure_rx) = watch::channel(MemoryPressure::None);

        Self {
            limits,
            pressure_tx,
            pressure_rx,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a receiver for pressure updates.
    pub fn pressure_receiver(&self) -> watch::Receiver<MemoryPressure> {
        self.pressure_rx.clone()
    }

    /// Get current pressure level.
    pub fn current_pressure(&self) -> MemoryPressure {
        *self.pressure_rx.borrow()
    }

    /// Start monitoring in a background task.
    pub fn start(&self) -> Arc<AtomicBool> {
        self.running.store(true, Ordering::SeqCst);
        Arc::clone(&self.running)
    }

    /// Stop monitoring.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Run the monitoring loop (call in a spawned task).
    pub async fn run(&self) {
        self.running.store(true, Ordering::SeqCst);

        while self.running.load(Ordering::SeqCst) {
            let usage = self.get_usage_percent();
            let pressure = Self::usage_to_pressure(usage);

            // Only log on change
            if pressure != *self.pressure_rx.borrow() {
                match pressure {
                    MemoryPressure::High => {
                        warn!("High memory pressure: {:.1}%", usage * 100.0);
                    }
                    MemoryPressure::Critical => {
                        warn!("CRITICAL memory pressure: {:.1}%", usage * 100.0);
                    }
                    MemoryPressure::None => {
                        info!("Memory pressure returned to normal: {:.1}%", usage * 100.0);
                    }
                    _ => {
                        debug!(
                            "Memory pressure changed: {} ({:.1}%)",
                            pressure,
                            usage * 100.0
                        );
                    }
                }
                let _ = self.pressure_tx.send(pressure);
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    /// Get current memory usage as a fraction (0.0 - 1.0).
    pub fn get_usage_percent(&self) -> f64 {
        let limit = match self.limits.memory_limit {
            Some(l) => l,
            None => return 0.0, // No limit set
        };

        let usage = self.read_memory_usage().unwrap_or(0);
        usage as f64 / limit as f64
    }

    /// Read current memory usage from cgroup.
    fn read_memory_usage(&self) -> Option<usize> {
        match self.limits.cgroup_version {
            Some(CgroupVersion::V2) => {
                // cgroup v2: /sys/fs/cgroup/memory.current
                fs::read_to_string("/sys/fs/cgroup/memory.current")
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
            }
            Some(CgroupVersion::V1) => {
                // cgroup v1: /sys/fs/cgroup/memory/memory.usage_in_bytes
                fs::read_to_string("/sys/fs/cgroup/memory/memory.usage_in_bytes")
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
            }
            _ => None,
        }
    }

    /// Convert usage percentage to pressure level.
    fn usage_to_pressure(usage: f64) -> MemoryPressure {
        if usage > 0.95 {
            MemoryPressure::Critical
        } else if usage > 0.90 {
            MemoryPressure::High
        } else if usage > 0.80 {
            MemoryPressure::Medium
        } else if usage > 0.70 {
            MemoryPressure::Low
        } else {
            MemoryPressure::None
        }
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_to_pressure() {
        assert_eq!(MemoryMonitor::usage_to_pressure(0.5), MemoryPressure::None);
        assert_eq!(MemoryMonitor::usage_to_pressure(0.75), MemoryPressure::Low);
        assert_eq!(
            MemoryMonitor::usage_to_pressure(0.85),
            MemoryPressure::Medium
        );
        assert_eq!(MemoryMonitor::usage_to_pressure(0.92), MemoryPressure::High);
        assert_eq!(
            MemoryMonitor::usage_to_pressure(0.97),
            MemoryPressure::Critical
        );
    }

    #[test]
    fn test_pressure_display() {
        assert_eq!(MemoryPressure::None.to_string(), "none");
        assert_eq!(MemoryPressure::Critical.to_string(), "critical");
    }
}
