use super::limits::*;
use anyhow::{Context, Result};
use std::process::Command;

/// Collect macOS-specific OS limits
pub fn collect_os_limits() -> Result<OsLimits> {
    Ok(OsLimits {
        process: collect_process_limits()?,
        network: collect_network_limits()?,
        io: collect_io_limits()?,
        container: None, // macOS doesn't use cgroups
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

        // On macOS, we can use lsof to count open files for current process
        let current_fds = count_open_fds()?;
        let current_procs = count_processes()?;

        Ok(ProcessLimits {
            open_files: ResourceLimit::new(nofile.rlim_cur, nofile.rlim_max, current_fds),
            max_processes: ResourceLimit::new(nproc.rlim_cur, nproc.rlim_max, current_procs),
            stack_size_kb: stack.rlim_cur / 1024,
        })
    }
}

fn count_open_fds() -> Result<u64> {
    let pid = std::process::id();
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .output()
        .context("Failed to run lsof")?;

    if !output.status.success() {
        return Ok(0);
    }

    let count = String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1) // Skip header
        .count();

    Ok(count as u64)
}

fn count_processes() -> Result<u64> {
    // On macOS, count threads using sysctl
    let pid = std::process::id();
    let output = Command::new("ps")
        .args(["-M", "-p", &pid.to_string()])
        .output()
        .context("Failed to run ps")?;

    if !output.status.success() {
        return Ok(1);
    }

    let count = String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1) // Skip header
        .count();

    Ok(count.max(1) as u64)
}

fn collect_network_limits() -> Result<NetworkLimits> {
    Ok(NetworkLimits {
        somaxconn: NetworkLimit::new(
            sysctl_read_u64("kern.ipc.somaxconn")?,
            Recommendations::SOMAXCONN,
        ),
        tcp_max_syn_backlog: sysctl_read_u64("net.inet.tcp.syncache.bucketlimit")
            .unwrap_or(512),
        tcp_rmem: [
            16384,
            sysctl_read_u64("net.inet.tcp.recvspace").unwrap_or(131072),
            sysctl_read_u64("net.inet.tcp.autorcvbufmax").unwrap_or(2097152),
        ],
        tcp_wmem: [
            16384,
            sysctl_read_u64("net.inet.tcp.sendspace").unwrap_or(131072),
            sysctl_read_u64("net.inet.tcp.autosndbufmax").unwrap_or(2097152),
        ],
        netdev_max_backlog: sysctl_read_u64("net.inet.ip.intr_queue_maxlen")
            .unwrap_or(256),
    })
}

fn collect_io_limits() -> Result<IoLimits> {
    Ok(IoLimits {
        epoll_max_user_watches: None, // macOS doesn't use epoll
        kqueue_max: sysctl_read_u64("kern.maxfiles").ok(),
        aio_max_nr: sysctl_read_u64("kern.aiomax").unwrap_or(90),
        file_max: sysctl_read_u64("kern.maxfiles").unwrap_or(12288),
    })
}

fn sysctl_read_u64(name: &str) -> Result<u64> {
    let output = Command::new("sysctl")
        .args(["-n", name])
        .output()
        .with_context(|| format!("Failed to run sysctl {}", name))?;

    if !output.status.success() {
        anyhow::bail!("sysctl {} failed", name);
    }

    let value = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .with_context(|| format!("Failed to parse sysctl {} value", name))?;

    Ok(value)
}
