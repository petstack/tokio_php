use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimit {
    pub soft: u64,
    pub hard: u64,
    pub current: u64,
    pub utilization_pct: f64,
    pub status: LimitStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LimitStatus {
    Ok,
    Warning,
    Critical,
}

impl ResourceLimit {
    pub fn new(soft: u64, hard: u64, current: u64) -> Self {
        let utilization_pct = if soft > 0 {
            (current as f64 / soft as f64) * 100.0
        } else {
            0.0
        };

        let status = match utilization_pct {
            x if x >= 90.0 => LimitStatus::Critical,
            x if x >= 70.0 => LimitStatus::Warning,
            _ => LimitStatus::Ok,
        };

        Self {
            soft,
            hard,
            current,
            utilization_pct,
            status,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessLimits {
    pub open_files: ResourceLimit,
    pub max_processes: ResourceLimit,
    pub stack_size_kb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkLimits {
    pub somaxconn: NetworkLimit,
    pub tcp_max_syn_backlog: u64,
    pub tcp_rmem: [u64; 3],
    pub tcp_wmem: [u64; 3],
    pub netdev_max_backlog: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkLimit {
    pub value: u64,
    pub recommended: u64,
    pub status: LimitStatus,
}

impl NetworkLimit {
    pub fn new(value: u64, recommended: u64) -> Self {
        let status = if value < recommended * 50 / 100 {
            LimitStatus::Critical
        } else if value < recommended {
            LimitStatus::Warning
        } else {
            LimitStatus::Ok
        };

        Self {
            value,
            recommended,
            status,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoll_max_user_watches: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kqueue_max: Option<u64>,
    pub aio_max_nr: u64,
    pub file_max: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerLimits {
    pub memory_limit_bytes: u64,
    pub memory_usage_bytes: u64,
    pub memory_utilization_pct: f64,
    pub cpu_quota: f64,
    pub cpu_period_us: u64,
    pub pids_limit: u64,
    pub pids_current: u64,
    pub status: LimitStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsLimits {
    pub process: ProcessLimits,
    pub network: NetworkLimits,
    pub io: IoLimits,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerLimits>,
}

// Recommended thresholds for high-performance async server
pub struct Recommendations;

impl Recommendations {
    pub const SOMAXCONN: u64 = 8192;
    pub const TCP_MAX_SYN_BACKLOG: u64 = 8192;
    pub const NETDEV_MAX_BACKLOG: u64 = 5000;
    pub const OPEN_FILES_SOFT: u64 = 65536;
    pub const EPOLL_MAX_USER_WATCHES: u64 = 524288;
}
