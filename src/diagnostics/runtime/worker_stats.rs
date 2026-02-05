use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpWorkerStats {
    pub count: usize,
    pub busy: usize,
    pub idle: usize,
    pub queue_depth: usize,
    pub avg_execution_time_ms: f64,
    pub p99_execution_time_ms: f64,
    pub avg_wait_time_ms: f64,
    pub max_wait_time_ms: f64,
    pub total_requests: u64,
    pub status: super::super::os::limits::LimitStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub rust_allocated_bytes: u64,
    pub rust_resident_bytes: u64,
    pub php_per_worker_avg_bytes: u64,
    pub php_per_worker_max_bytes: u64,
    pub total_php_memory_bytes: u64,
    pub file_cache_bytes: u64,
    pub status: super::super::os::limits::LimitStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockStats {
    pub worker_pool_contention_pct: f64,
    pub file_cache_contention_pct: f64,
    pub config_lock_contention_pct: f64,
    pub status: super::super::os::limits::LimitStatus,
}

/// Collect PHP worker pool statistics
/// This would integrate with your existing worker pool implementation
pub fn collect_worker_stats(
    worker_count: usize,
    busy_workers: usize,
    queue_depth: usize,
    total_requests: u64,
    execution_times_ms: &[f64],
    wait_times_ms: &[f64],
) -> PhpWorkerStats {
    let idle = worker_count.saturating_sub(busy_workers);

    let avg_execution_time_ms = if !execution_times_ms.is_empty() {
        execution_times_ms.iter().sum::<f64>() / execution_times_ms.len() as f64
    } else {
        0.0
    };

    let p99_execution_time_ms = calculate_percentile(execution_times_ms, 0.99);

    let avg_wait_time_ms = if !wait_times_ms.is_empty() {
        wait_times_ms.iter().sum::<f64>() / wait_times_ms.len() as f64
    } else {
        0.0
    };

    let max_wait_time_ms = wait_times_ms.iter().cloned().fold(0.0f64, f64::max);

    // Determine health status
    let utilization_pct = if worker_count > 0 {
        (busy_workers as f64 / worker_count as f64) * 100.0
    } else {
        0.0
    };

    let status = if utilization_pct > 90.0 && queue_depth > 10 {
        super::super::os::limits::LimitStatus::Critical
    } else if utilization_pct > 70.0 || queue_depth > 5 {
        super::super::os::limits::LimitStatus::Warning
    } else {
        super::super::os::limits::LimitStatus::Ok
    };

    PhpWorkerStats {
        count: worker_count,
        busy: busy_workers,
        idle,
        queue_depth,
        avg_execution_time_ms,
        p99_execution_time_ms,
        avg_wait_time_ms,
        max_wait_time_ms,
        total_requests,
        status,
    }
}

/// Collect memory statistics
pub fn collect_memory_stats(
    php_worker_count: usize,
    php_memory_per_worker: Vec<u64>,
    file_cache_size: u64,
) -> MemoryStats {
    use sysinfo::{ProcessExt, System, SystemExt};

    let mut sys = System::new_all();
    sys.refresh_all();

    let pid = sysinfo::get_current_pid().unwrap();
    let process = sys.process(pid).unwrap();

    let rust_allocated_bytes = process.memory() * 1024; // Convert from KB
    let rust_resident_bytes = process.memory() * 1024;

    let total_php_memory_bytes: u64 = php_memory_per_worker.iter().sum();
    let php_per_worker_avg_bytes = if !php_memory_per_worker.is_empty() {
        total_php_memory_bytes / php_memory_per_worker.len() as u64
    } else {
        0
    };

    let php_per_worker_max_bytes = php_memory_per_worker.iter().cloned().max().unwrap_or(0);

    // Calculate total memory usage percentage (if we can detect system memory)
    let total_memory = sys.total_memory() * 1024; // Convert from KB
    let total_used = rust_allocated_bytes + total_php_memory_bytes + file_cache_size;
    let usage_pct = if total_memory > 0 {
        (total_used as f64 / total_memory as f64) * 100.0
    } else {
        0.0
    };

    let status = if usage_pct > 90.0 {
        super::super::os::limits::LimitStatus::Critical
    } else if usage_pct > 75.0 {
        super::super::os::limits::LimitStatus::Warning
    } else {
        super::super::os::limits::LimitStatus::Ok
    };

    MemoryStats {
        rust_allocated_bytes,
        rust_resident_bytes,
        php_per_worker_avg_bytes,
        php_per_worker_max_bytes,
        total_php_memory_bytes,
        file_cache_bytes: file_cache_size,
        status,
    }
}

/// Collect lock contention statistics
/// This would integrate with instrumentation around your mutexes/RwLocks
pub fn collect_lock_stats(
    worker_pool_wait_ns: u64,
    worker_pool_hold_ns: u64,
    file_cache_wait_ns: u64,
    file_cache_hold_ns: u64,
    config_wait_ns: u64,
    config_hold_ns: u64,
) -> LockStats {
    let worker_pool_contention_pct = calculate_contention_pct(worker_pool_wait_ns, worker_pool_hold_ns);
    let file_cache_contention_pct = calculate_contention_pct(file_cache_wait_ns, file_cache_hold_ns);
    let config_lock_contention_pct = calculate_contention_pct(config_wait_ns, config_hold_ns);

    let max_contention = worker_pool_contention_pct
        .max(file_cache_contention_pct)
        .max(config_lock_contention_pct);

    let status = if max_contention > 20.0 {
        super::super::os::limits::LimitStatus::Critical
    } else if max_contention > 10.0 {
        super::super::os::limits::LimitStatus::Warning
    } else {
        super::super::os::limits::LimitStatus::Ok
    };

    LockStats {
        worker_pool_contention_pct,
        file_cache_contention_pct,
        config_lock_contention_pct,
        status,
    }
}

fn calculate_contention_pct(wait_time_ns: u64, hold_time_ns: u64) -> f64 {
    let total_time = wait_time_ns + hold_time_ns;
    if total_time == 0 {
        return 0.0;
    }
    (wait_time_ns as f64 / total_time as f64) * 100.0
}

fn calculate_percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let index = ((sorted.len() as f64 - 1.0) * percentile) as usize;
    sorted[index.min(sorted.len() - 1)]
}
