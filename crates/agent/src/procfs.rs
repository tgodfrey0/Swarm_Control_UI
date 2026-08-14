//! Lightweight host metrics read from /proc (static-binary friendly, no sysinfo).

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub cpu_usage: f64,
    pub memory_used_kb: u64,
    pub uptime_sec: u64,
    pub battery_percent: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Probe {
    prev_idle: u64,
    prev_total: u64,
}

impl Probe {
    pub fn sample(&mut self) -> Metrics {
        let (idle, total) = cpu_ticks();
        let mut cpu = 0.0;
        if self.prev_total > 0 && total >= self.prev_total && total > self.prev_total {
            let idle_delta = idle.saturating_sub(self.prev_idle);
            let total_delta = total - self.prev_total;
            cpu = (1.0 - idle_delta as f64 / total_delta as f64) * 100.0;
        }
        self.prev_idle = idle;
        self.prev_total = total;

        Metrics {
            cpu_usage: cpu.clamp(0.0, 100.0),
            memory_used_kb: mem_used_kb(),
            uptime_sec: uptime_sec(),
            battery_percent: battery_percent(),
        }
    }
}

fn cpu_ticks() -> (u64, u64) {
    // /proc/stat first line: cpu user nice system idle iowait irq softirq ...
    let Ok(line) = std::fs::read_to_string("/proc/stat") else {
        return (0, 0);
    };
    let Some(first) = line.lines().next() else {
        return (0, 0);
    };
    let fields: Vec<u64> = first
        .split_whitespace()
        .skip(1)
        .filter_map(|f| f.parse().ok())
        .collect();
    if fields.len() < 4 {
        return (0, 0);
    }
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    let total: u64 = fields.iter().sum();
    (idle, total)
}

fn mem_used_kb() -> u64 {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    let mut total = 0u64;
    let mut available = 0u64;
    for line in text.lines() {
        if line.starts_with("MemTotal:") {
            total = parse_kb(line);
        } else if line.starts_with("MemAvailable:") {
            available = parse_kb(line);
        }
    }
    total.saturating_sub(available)
}

fn parse_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn uptime_sec() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|f| f as u64)
        .unwrap_or(0)
}

fn battery_percent() -> f32 {
    // First power-supply capacity we can find, else 0 (unknown).
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return 0.0;
    };
    for entry in entries.flatten() {
        let cap = entry.path().join("capacity");
        if let Ok(v) = std::fs::read_to_string(cap) {
            if let Ok(pct) = v.trim().parse::<f32>() {
                return pct.clamp(0.0, 100.0);
            }
        }
    }
    0.0
}
