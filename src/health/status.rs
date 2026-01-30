//! Health status types for Kubernetes probes.

use serde::Serialize;

/// Health check probe types (Kubernetes-compatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeType {
    /// Liveness probe: restart container if failed.
    Liveness,
    /// Readiness probe: remove from load balancer if failed.
    Readiness,
    /// Startup probe: wait for initialization.
    Startup,
}

impl std::fmt::Display for ProbeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Liveness => write!(f, "liveness"),
            Self::Readiness => write!(f, "readiness"),
            Self::Startup => write!(f, "startup"),
        }
    }
}

/// Health status response.
#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    /// Overall status: "healthy", "unhealthy", "not_ready"
    pub status: &'static str,
    /// Optional message describing the status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Individual check results
    pub checks: Vec<CheckResult>,
    /// Detailed server information (only included when healthy)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HealthDetails>,
}

impl HealthStatus {
    /// Create a healthy status.
    pub fn healthy() -> Self {
        Self {
            status: "healthy",
            message: None,
            checks: Vec::new(),
            details: None,
        }
    }

    /// Create an unhealthy status with a message.
    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self {
            status: "unhealthy",
            message: Some(message.into()),
            checks: Vec::new(),
            details: None,
        }
    }

    /// Create a not_ready status with a message.
    pub fn not_ready(message: impl Into<String>) -> Self {
        Self {
            status: "not_ready",
            message: Some(message.into()),
            checks: Vec::new(),
            details: None,
        }
    }

    /// Add a check result.
    pub fn with_check(mut self, check: CheckResult) -> Self {
        self.checks.push(check);
        self
    }

    /// Add details.
    pub fn with_details(mut self, details: HealthDetails) -> Self {
        self.details = Some(details);
        self
    }

    /// Returns true if status is healthy.
    pub fn is_healthy(&self) -> bool {
        self.status == "healthy"
    }
}

/// Individual health check result.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    /// Check name (e.g., "php_initialized", "queue_capacity")
    pub name: &'static str,
    /// Check status: "pass", "fail", "warn", "pending"
    pub status: &'static str,
    /// Optional message with details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Check duration in milliseconds
    pub duration_ms: u64,
}

impl CheckResult {
    /// Create a passing check.
    pub fn pass(name: &'static str, duration_ms: u64) -> Self {
        Self {
            name,
            status: "pass",
            message: None,
            duration_ms,
        }
    }

    /// Create a failing check with a message.
    pub fn fail(name: &'static str, message: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            name,
            status: "fail",
            message: Some(message.into()),
            duration_ms,
        }
    }

    /// Create a warning check with a message.
    pub fn warn(name: &'static str, message: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            name,
            status: "warn",
            message: Some(message.into()),
            duration_ms,
        }
    }

    /// Create a pending check.
    pub fn pending(name: &'static str) -> Self {
        Self {
            name,
            status: "pending",
            message: None,
            duration_ms: 0,
        }
    }

    /// Returns true if check passed.
    pub fn is_pass(&self) -> bool {
        self.status == "pass"
    }

    /// Returns true if check failed.
    pub fn is_fail(&self) -> bool {
        self.status == "fail"
    }
}

/// Detailed server information included in health responses.
#[derive(Debug, Clone, Serialize)]
pub struct HealthDetails {
    /// Server uptime in seconds
    pub uptime_seconds: u64,
    /// Server version
    pub version: &'static str,
    /// Number of worker threads
    pub workers: usize,
    /// Current queue depth
    pub queue_depth: usize,
    /// Queue capacity
    pub queue_capacity: usize,
    /// Current active connections
    pub active_connections: usize,
}
