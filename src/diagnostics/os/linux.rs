use super::limits::*;
use anyhow::{Context, Result};
use std::fs;
use std::os::unix::io::AsRawFd;

/// Collect Linux-specific OS limits
pub fn collect_os_limits() -> Result<OsLimits> {
    Ok(OsLimits {
        process: collect_process_limits()?,
        network: collect_network_limits()?,
        io: collect_io_limits()?,
        container: collect_container_limits().ok(),
    })
}

fn collect_process_limits() -> Result<ProcessLimits> {
    use libc::{getrlimit, rlimit, RLIMIT_NOFILE, RLIMIT_NPROC, RLIMIT_STACK};

    unsafe {
        let mut nofile: rlimit = std::mem::zeroed();
        let mut nproc: rlimit = std::mem::zeroed();
        let mut stack: rlimit = std::mem::zeroed();

        if getrlimit(RLIMIT_NOFILE, &mut nofile) != 0 {
            anyhow::bail!("Failed to get RLIMIT_NOFILE");
        }
        if getrlimit(RLIMIT_NPROC, &mut nproc) != 0 {
            anyhow::bail!("Failed to get RLIMIT_NPROC");
        }
        if getrlimit(RLIMIT_STACK, &mut stack) != 0 {
            anyhow::bail!("Failed to get RLIMIT_STACK");
        }

        // Count current open file descriptors
        let current_fds = count_open_fds()?;

        // Count current processes (threads in same process group)
        let current_procs = count_processes()?;

        Ok(ProcessLimits {
            open_files: ResourceLimit::new(nofile.rlim_cur, nofile.rlim_max, current_fds),
            max_processes: ResourceLimit::new(nproc.rlim_cur, nproc.rlim_max, current_procs),
            stack_size_kb: stack.rlim_cur / 1024,
        })
    }
}

fn count_open_fds() -> Result<u64> {
    let fd_count = fs::read_dir("/proc/self/fd")
        .context("Failed to read /proc/self/fd")?
        .count();
    Ok(fd_count as u64)
}

fn count_processes() -> Result<u64> {
    let status = fs::read_to_string("/proc/self/status")
        .context("Failed to read /proc/self/status")?;

    for line in status.lines() {
        if line.starts_with("Threads:") {
            if let Some(count) = line.split_whitespace().nth(1) {
                return Ok(count.parse().unwrap_or(1));
            }
        }
    }
    Ok(1)
}

fn collect_network_limits() -> Result<NetworkLimits> {
    Ok(NetworkLimits {
        somaxconn: NetworkLimit::new(
            read_sysctl_u64("/proc/sys/net/core/somaxconn")?,
            Recommendations::SOMAXCONN,
        ),
        tcp_max_syn_backlog: read_sysctl_u64("/proc/sys/net/ipv4/tcp_max_syn_backlog")?,
        tcp_rmem: read_sysctl_triple("/proc/sys/net/ipv4/tcp_rmem")?,
        tcp_wmem: read_sysctl_triple("/proc/sys/net/ipv4/tcp_wmem")?,
        netdev_max_backlog: read_sysctl_u64("/proc/sys/net/core/netdev_max_backlog")?,
    })
}

fn collect_io_limits() -> Result<IoLimits> {
    Ok(IoLimits {
        epoll_max_user_watches: read_sysctl_u64("/proc/sys/fs/epoll/max_user_watches").ok(),
        kqueue_max: None, // Linux doesn't use kqueue
        aio_max_nr: read_sysctl_u64("/proc/sys/fs/aio-max-nr")?,
        file_max: read_sysctl_u64("/proc/sys/fs/file-max")?,
    })
}

fn collect_container_limits() -> Result<ContainerLimits> {
    // Detect cgroup v1 or v2
    let cgroup_version = detect_cgroup_version()?;

    match cgroup_version {
        1 => collect_cgroupv1_limits(),
        2 => collect_cgroupv2_limits(),
        _ => anyhow::bail!("Unknown cgroup version"),
    }
}

fn detect_cgroup_version() -> Result<u8> {
    // Check if cgroup v2 unified hierarchy exists
    if fs::metadata("/sys/fs/cgroup/cgroup.controllers").is_ok() {
        Ok(2)
    } else if fs::metadata("/sys/fs/cgroup/memory").is_ok() {
        Ok(1)
    } else {
        anyhow::bail!("No cgroup support detected")
    }
}

fn collect_cgroupv2_limits() -> Result<ContainerLimits> {
    let memory_max = read_cgroup_file("/sys/fs/cgroup/memory.max")?
        .trim()
        .parse::<u64>()
        .unwrap_or(u64::MAX);

    let memory_current = read_cgroup_file("/sys/fs/cgroup/memory.current")?
        .trim()
        .parse::<u64>()
        .unwrap_or(0);

    // CPU quota: read cpu.max which is "max period"
    let cpu_max = read_cgroup_file("/sys/fs/cgroup/cpu.max")?;
    let (cpu_quota, cpu_period_us) = parse_cpu_max(&cpu_max)?;

    let pids_max = read_cgroup_file("/sys/fs/cgroup/pids.max")?
        .trim()
        .parse::<u64>()
        .unwrap_or(u64::MAX);

    let pids_current = read_cgroup_file("/sys/fs/cgroup/pids.current")?
        .trim()
        .parse::<u64>()
        .unwrap_or(0);

    let memory_utilization_pct = if memory_max < u64::MAX {
        (memory_current as f64 / memory_max as f64) * 100.0
    } else {
        0.0
    };

    let status = if memory_utilization_pct >= 90.0 {
        LimitStatus::Critical
    } else if memory_utilization_pct >= 70.0 {
        LimitStatus::Warning
    } else {
        LimitStatus::Ok
    };

    Ok(ContainerLimits {
        memory_limit_bytes: memory_max,
        memory_usage_bytes: memory_current,
        memory_utilization_pct,
        cpu_quota,
        cpu_period_us,
        pids_limit: pids_max,
        pids_current,
        status,
    })
}

fn collect_cgroupv1_limits() -> Result<ContainerLimits> {
    let memory_limit = read_cgroup_file("/sys/fs/cgroup/memory/memory.limit_in_bytes")?
        .trim()
        .parse::<u64>()
        .unwrap_or(u64::MAX);

    let memory_usage = read_cgroup_file("/sys/fs/cgroup/memory/memory.usage_in_bytes")?
        .trim()
        .parse::<u64>()
        .unwrap_or(0);

    let cpu_quota = read_cgroup_file("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")?
        .trim()
        .parse::<i64>()
        .unwrap_or(-1);

    let cpu_period = read_cgroup_file("/sys/fs/cgroup/cpu/cpu.cfs_period_us")?
        .trim()
        .parse::<u64>()
        .unwrap_or(100000);

    let cpu_quota_f = if cpu_quota > 0 {
        cpu_quota as f64 / cpu_period as f64
    } else {
        0.0
    };

    let pids_max = read_cgroup_file("/sys/fs/cgroup/pids/pids.max")?
        .trim()
        .parse::<u64>()
        .unwrap_or(u64::MAX);

    let pids_current = read_cgroup_file("/sys/fs/cgroup/pids/pids.current")?
        .trim()
        .parse::<u64>()
        .unwrap_or(0);

    let memory_utilization_pct = if memory_limit < u64::MAX {
        (memory_usage as f64 / memory_limit as f64) * 100.0
    } else {
        0.0
    };

    let status = if memory_utilization_pct >= 90.0 {
        LimitStatus::Critical
    } else if memory_utilization_pct >= 70.0 {
        LimitStatus::Warning
    } else {
        LimitStatus::Ok
    };

    Ok(ContainerLimits {
        memory_limit_bytes: memory_limit,
        memory_usage_bytes: memory_usage,
        memory_utilization_pct,
        cpu_quota: cpu_quota_f,
        cpu_period_us: cpu_period,
        pids_limit: pids_max,
        pids_current,
        status,
    })
}

fn parse_cpu_max(content: &str) -> Result<(f64, u64)> {
    let parts: Vec<&str> = content.trim().split_whitespace().collect();
    if parts.len() != 2 {
        return Ok((0.0, 100000));
    }

    let quota = if parts[0] == "max" {
        -1i64
    } else {
        parts[0].parse::<i64>().unwrap_or(-1)
    };

    let period = parts[1].parse::<u64>().unwrap_or(100000);

    let quota_f = if quota > 0 {
        quota as f64 / period as f64
    } else {
        0.0
    };

    Ok((quota_f, period))
}

fn read_sysctl_u64(path: &str) -> Result<u64> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path))?;
    content
        .trim()
        .parse()
        .with_context(|| format!("Failed to parse {} as u64", path))
}

fn read_sysctl_triple(path: &str) -> Result<[u64; 3]> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path))?;

    let parts: Vec<u64> = content
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();

    if parts.len() != 3 {
        anyhow::bail!("Expected 3 values in {}", path);
    }

    Ok([parts[0], parts[1], parts[2]])
}

fn read_cgroup_file(path: &str) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))
}
