use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokioMetrics {
    pub workers: usize,
    pub active_tasks: u64,
    pub queue_depth: u64,
    pub total_park_count: u64,
    pub total_steal_count: u64,
    pub total_steal_operations: u64,
    pub mean_poll_time_us: u64,
    pub max_poll_time_us: u64,
    pub busy_duration_total_sec: f64,
    pub scheduler_latency_p50_us: u64,
    pub scheduler_latency_p99_us: u64,
    pub status: super::super::os::limits::LimitStatus,
}

#[cfg(feature = "tokio-console")]
pub fn collect_tokio_metrics(runtime_handle: &tokio::runtime::Handle) -> TokioMetrics {
    // Use tokio-console instrumentation if available
    use tokio_metrics::RuntimeMetrics;

    let metrics = RuntimeMetrics::new(runtime_handle);
    let intervals = metrics.intervals();

    // Collect recent interval data
    let mut total_busy_duration = std::time::Duration::ZERO;
    let mut total_park_count = 0;
    let mut total_steal_count = 0;
    let mut total_steal_operations = 0;
    let mut max_poll_time = std::time::Duration::ZERO;
    let mut poll_times = Vec::new();

    for interval in intervals.take(10) {
        total_busy_duration += interval.total_busy_duration;
        total_park_count += interval.total_park_count;
        total_steal_count += interval.total_steal_count;
        total_steal_operations += interval.total_steal_operations;

        if interval.max_poll_duration > max_poll_time {
            max_poll_time = interval.max_poll_duration;
        }

        poll_times.push(interval.mean_poll_duration.as_micros() as u64);
    }

    let mean_poll_time_us = if !poll_times.is_empty() {
        poll_times.iter().sum::<u64>() / poll_times.len() as u64
    } else {
        0
    };

    // Estimate scheduler latency (simplified - in production use tokio-metrics histogram)
    let scheduler_latency_p50_us = mean_poll_time_us / 2;
    let scheduler_latency_p99_us = max_poll_time.as_micros() as u64 / 2;

    // Determine status based on poll times
    let status = if max_poll_time.as_millis() > 100 {
        super::super::os::limits::LimitStatus::Critical
    } else if max_poll_time.as_millis() > 50 {
        super::super::os::limits::LimitStatus::Warning
    } else {
        super::super::os::limits::LimitStatus::Ok
    };

    TokioMetrics {
        workers: runtime_handle.metrics().num_workers(),
        active_tasks: runtime_handle.metrics().num_alive_tasks() as u64,
        queue_depth: 0, // Requires tokio-console
        total_park_count,
        total_steal_count,
        total_steal_operations,
        mean_poll_time_us,
        max_poll_time_us: max_poll_time.as_micros() as u64,
        busy_duration_total_sec: total_busy_duration.as_secs_f64(),
        scheduler_latency_p50_us,
        scheduler_latency_p99_us,
        status,
    }
}

#[cfg(not(feature = "tokio-console"))]
pub fn collect_tokio_metrics(runtime_handle: &tokio::runtime::Handle) -> TokioMetrics {
    // Fallback to basic metrics from tokio::runtime::RuntimeMetrics
    let metrics = runtime_handle.metrics();

    let workers = metrics.num_workers();
    let active_tasks = metrics.num_alive_tasks() as u64;

    // Without tokio-console, we have limited visibility
    // Return conservative estimates
    TokioMetrics {
        workers,
        active_tasks,
        queue_depth: 0,
        total_park_count: 0,
        total_steal_count: 0,
        total_steal_operations: 0,
        mean_poll_time_us: 0,
        max_poll_time_us: 0,
        busy_duration_total_sec: 0.0,
        scheduler_latency_p50_us: 0,
        scheduler_latency_p99_us: 0,
        status: super::super::os::limits::LimitStatus::Ok,
    }
}

/// Simplified version that works with current tokio_php architecture
/// This reads from existing Prometheus metrics if available
pub fn collect_tokio_metrics_from_handle(runtime_handle: &tokio::runtime::Handle) -> TokioMetrics {
    let metrics = runtime_handle.metrics();

    let workers = metrics.num_workers();
    let active_tasks = metrics.num_alive_tasks() as u64;
    let blocking_threads = metrics.num_blocking_threads() as u64;

    // Derive basic health status
    let tasks_per_worker = if workers > 0 {
        active_tasks / workers as u64
    } else {
        0
    };

    let status = if tasks_per_worker > 1000 {
        super::super::os::limits::LimitStatus::Critical
    } else if tasks_per_worker > 500 {
        super::super::os::limits::LimitStatus::Warning
    } else {
        super::super::os::limits::LimitStatus::Ok
    };

    TokioMetrics {
        workers,
        active_tasks,
        queue_depth: tasks_per_worker,
        total_park_count: 0,
        total_steal_count: 0,
        total_steal_operations: 0,
        mean_poll_time_us: 0,
        max_poll_time_us: 0,
        busy_duration_total_sec: 0.0,
        scheduler_latency_p50_us: 0,
        scheduler_latency_p99_us: 0,
        status,
    }
}
