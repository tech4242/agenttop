//! System-wide CPU%, MEM%, and 1-minute load average.
//!
//! Cross-platform via `sysinfo`. CPU% is a stateful delta between ticks, so we
//! need a long-lived `HostSampler` rather than ad-hoc snapshots.

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

use super::HostMetrics;

pub struct HostSampler {
    sys: System,
    initialized: bool,
}

impl Default for HostSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl HostSampler {
    pub fn new() -> Self {
        let refresh = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(MemoryRefreshKind::nothing().with_ram());
        let sys = System::new_with_specifics(refresh);
        Self {
            sys,
            initialized: false,
        }
    }

    pub fn sample(&mut self) -> HostMetrics {
        // CPU usage requires two refreshes spaced by at least
        // `MINIMUM_CPU_UPDATE_INTERVAL` to compute a delta. The first call
        // returns 0.0; subsequent calls give real numbers.
        self.sys
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage());
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());

        let cpu_pct = if self.initialized {
            let cpus = self.sys.cpus();
            if cpus.is_empty() {
                0.0
            } else {
                let sum: f32 = cpus.iter().map(|c| c.cpu_usage()).sum();
                (sum / cpus.len() as f32) as f64
            }
        } else {
            self.initialized = true;
            0.0
        };

        let total = self.sys.total_memory();
        let used = self.sys.used_memory();
        let mem_pct = if total == 0 {
            0.0
        } else {
            (used as f64 / total as f64) * 100.0
        };

        let load1 = System::load_average().one;

        HostMetrics {
            cpu_pct,
            mem_pct,
            load1,
        }
    }
}
