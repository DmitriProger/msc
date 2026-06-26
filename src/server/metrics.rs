use anyhow::{Context, Result};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct ServerMetrics {
    pub pid: Option<u32>,
    pub ram_bytes: u64,
    pub cpu_percent: f64,
    pub net_rx_bytes_per_sec: u64,
    pub net_tx_bytes_per_sec: u64,
    pub uptime_secs: u64,
}

pub struct MetricsCollector {
    prev_cpu_times: Option<(u64, u64, Instant)>,
    prev_net: Option<(u64, u64, Instant)>,
    start_time: Option<Instant>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            prev_cpu_times: None,
            prev_net: None,
            start_time: None,
        }
    }

    pub fn collect(&mut self, pid: u32) -> Result<ServerMetrics> {
        if self.start_time.is_none() {
            self.start_time = Some(Instant::now());
        }

        let ram_bytes = read_vmrss(pid)?;
        let cpu_percent = self.read_cpu_percent(pid)?;
        let (net_rx, net_tx) = self.read_net_rates()?;

        let uptime_secs = self.start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0);

        Ok(ServerMetrics {
            pid: Some(pid),
            ram_bytes,
            cpu_percent,
            net_rx_bytes_per_sec: net_rx,
            net_tx_bytes_per_sec: net_tx,
            uptime_secs,
        })
    }

    fn read_cpu_percent(&mut self, pid: u32) -> Result<f64> {
        let (utime, stime) = read_cpu_ticks(pid)?;
        let total = utime + stime;
        let now = Instant::now();

        let percent = if let Some((prev_total, _, prev_time)) = self.prev_cpu_times {
            let elapsed = now.duration_since(prev_time).as_secs_f64();
            let ticks_per_sec = ticks_per_second() as f64;
            if elapsed > 0.0 {
                let delta_ticks = (total.saturating_sub(prev_total)) as f64;
                (delta_ticks / (elapsed * ticks_per_sec)) * 100.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        self.prev_cpu_times = Some((total, 0, now));
        Ok(percent.min(100.0 * num_cpus()))
    }

    fn read_net_rates(&mut self) -> Result<(u64, u64)> {
        let (rx, tx) = read_net_totals()?;
        let now = Instant::now();
        let rates = if let Some((prev_rx, prev_tx, prev_time)) = self.prev_net {
            let elapsed = now.duration_since(prev_time).as_secs_f64();
            if elapsed > 0.0 {
                let rx_rate = (rx.saturating_sub(prev_rx) as f64 / elapsed) as u64;
                let tx_rate = (tx.saturating_sub(prev_tx) as f64 / elapsed) as u64;
                (rx_rate, tx_rate)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };
        self.prev_net = Some((rx, tx, now));
        Ok(rates)
    }
}

pub fn read_vmrss(pid: u32) -> Result<u64> {
    let path = format!("/proc/{}/status", pid);
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path))?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            return Ok(kb * 1024);
        }
    }
    Ok(0)
}

pub fn read_cpu_ticks(pid: u32) -> Result<(u64, u64)> {
    let path = format!("/proc/{}/stat", pid);
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path))?;
    let fields: Vec<&str> = content.split_whitespace().collect();
    if fields.len() < 15 {
        return Ok((0, 0));
    }
    let utime: u64 = fields[13].parse().unwrap_or(0);
    let stime: u64 = fields[14].parse().unwrap_or(0);
    Ok((utime, stime))
}

pub fn read_net_totals() -> Result<(u64, u64)> {
    let content =
        std::fs::read_to_string("/proc/net/dev").context("Failed to read /proc/net/dev")?;
    let mut total_rx = 0u64;
    let mut total_tx = 0u64;
    for line in content.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }
        let iface = parts[0].trim_end_matches(':');
        if iface == "lo" {
            continue;
        }
        total_rx += parts[1].parse::<u64>().unwrap_or(0);
        total_tx += parts[9].parse::<u64>().unwrap_or(0);
    }
    Ok((total_rx, total_tx))
}

fn ticks_per_second() -> u64 {
    unsafe { libc_sysconf() }
}

unsafe fn libc_sysconf() -> u64 {
    #[link(name = "c")]
    extern "C" {
        fn sysconf(name: i32) -> i64;
    }
    let ticks = sysconf(2); // _SC_CLK_TCK = 2
    if ticks > 0 {
        ticks as u64
    } else {
        100
    }
}

fn num_cpus() -> f64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0)
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}G", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

pub fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        format!("{}д {}ч {}м", days, hours, minutes)
    } else if hours > 0 {
        format!("{}ч {}м", hours, minutes)
    } else {
        format!("{}м", minutes)
    }
}

pub fn format_bytes_rate(bytes_per_sec: u64) -> String {
    if bytes_per_sec >= 1024 * 1024 {
        format!("{:.1} MB/s", bytes_per_sec as f64 / (1024.0 * 1024.0))
    } else if bytes_per_sec >= 1024 {
        format!("{:.1} KB/s", bytes_per_sec as f64 / 1024.0)
    } else {
        format!("{} B/s", bytes_per_sec)
    }
}

pub fn get_process_start_time(pid: u32) -> Option<u64> {
    let path = format!("/proc/{}/stat", pid);
    let content = std::fs::read_to_string(&path).ok()?;
    let fields: Vec<&str> = content.split_whitespace().collect();
    if fields.len() < 22 {
        return None;
    }
    fields[21].parse::<u64>().ok()
}

pub fn get_system_uptime_secs() -> f64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or(0.0)
}

pub fn get_process_uptime_secs(pid: u32) -> Option<u64> {
    let start_ticks = get_process_start_time(pid)?;
    let uptime = get_system_uptime_secs();
    let ticks_per_sec = ticks_per_second() as f64;
    let process_uptime = uptime - (start_ticks as f64 / ticks_per_sec);
    if process_uptime < 0.0 {
        Some(0)
    } else {
        Some(process_uptime as u64)
    }
}
