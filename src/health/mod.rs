//! Health check module for Kubernetes probes.
//!
//! Provides three types of health checks:
//! - **Liveness**: Is the process alive? (restart container if failed)
//! - **Readiness**: Can the service handle traffic? (remove from LB if failed)
//! - **Startup**: Has initialization completed? (wait before other probes)
//!
//! # Kubernetes Integration
//!
//! ```yaml
//! livenessProbe:
//!   httpGet:
//!     path: /health/live
//!     port: 9090
//!   initialDelaySeconds: 5
//!   periodSeconds: 10
//!
//! readinessProbe:
//!   httpGet:
//!     path: /health/ready
//!     port: 9090
//!   initialDelaySeconds: 5
//!   periodSeconds: 5
//!
//! startupProbe:
//!   httpGet:
//!     path: /health/startup
//!     port: 9090
//!   failureThreshold: 30
//!   periodSeconds: 2
//! ```

mod checker;
mod status;

pub use checker::{HealthChecker, HealthConfig};
pub use status::{CheckResult, HealthDetails, HealthStatus, ProbeType};
