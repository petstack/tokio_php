//! System resource detection and monitoring.
//!
//! Provides cgroup-aware resource detection for Kubernetes environments.
//! Automatically detects CPU/memory limits and optimizes worker count.
//!
//! # Cgroup Support
//!
//! - **cgroup v2**: Modern unified hierarchy (default on newer kernels)
//! - **cgroup v1**: Legacy hierarchy (still common in production)
//!
//! # Example
//!
//! ```rust,ignore
//! use tokio_php::system::ResourceLimits;
//!
//! let limits = ResourceLimits::from_cgroup();
//! let workers = limits.optimal_workers();
//! println!("CPU quota: {:?}, optimal workers: {}", limits.cpu_quota, workers);
//! ```

mod cgroup;
mod memory;

pub use cgroup::{CgroupVersion, ResourceLimits};
pub use memory::{MemoryMonitor, MemoryPressure};
