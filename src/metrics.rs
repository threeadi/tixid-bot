//! System Resource Metrics (CPU & RAM Usage)
//! Provides real-time process memory (RAM) and CPU utilization metrics
//! with zero external dependencies.

use std::sync::Mutex;
use std::time::Instant;

/// Snapshot of process resource utilization.
#[derive(Debug, Clone, Copy)]
pub struct ProcessMetrics {
    /// Resident / Working Set RAM usage in Megabytes (MB).
    pub ram_mb: f64,
    /// Virtual / Pagefile RAM usage in Megabytes (MB).
    pub virtual_ram_mb: f64,
    /// Process CPU utilization percentage (0.0% to 100.0%).
    pub cpu_percent: f64,
}

struct CpuState {
    last_wall: Instant,
    last_cpu_time_us: u64,
}

static CPU_TRACKER: Mutex<Option<CpuState>> = Mutex::new(None);

#[cfg(windows)]
mod os {
    use super::*;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[repr(C)]
    struct FileTime {
        dw_low: u32,
        dw_high: u32,
    }

    impl FileTime {
        fn to_us(&self) -> u64 {
            // FileTime is 100-nanosecond intervals
            let intervals = ((self.dw_high as u64) << 32) | (self.dw_low as u64);
            intervals / 10
        }
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(
            h_process: *mut std::ffi::c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn K32GetProcessMemoryInfo(
            h_process: *mut std::ffi::c_void,
            ppmc: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    pub fn get_memory_bytes() -> (usize, usize) {
        unsafe {
            let handle = GetCurrentProcess();
            let mut pmc = std::mem::zeroed::<ProcessMemoryCounters>();
            pmc.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
            if K32GetProcessMemoryInfo(handle, &mut pmc, pmc.cb) != 0 {
                (pmc.working_set_size, pmc.pagefile_usage)
            } else {
                (0, 0)
            }
        }
    }

    pub fn get_cpu_time_us() -> u64 {
        unsafe {
            let handle = GetCurrentProcess();
            let mut c = std::mem::zeroed::<FileTime>();
            let mut e = std::mem::zeroed::<FileTime>();
            let mut k = std::mem::zeroed::<FileTime>();
            let mut u = std::mem::zeroed::<FileTime>();
            if GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) != 0 {
                k.to_us() + u.to_us()
            } else {
                0
            }
        }
    }
}

#[cfg(not(windows))]
mod os {
    pub fn get_memory_bytes() -> (usize, usize) {
        // Linux /proc/self/statm
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                let page_size = 4096;
                let virt = parts[0].parse::<usize>().unwrap_or(0) * page_size;
                let rss = parts[1].parse::<usize>().unwrap_or(0) * page_size;
                return (rss, virt);
            }
        }
        (0, 0)
    }

    pub fn get_cpu_time_us() -> u64 {
        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
            if let Some(rparen) = stat.rfind(')') {
                let rest = &stat[rparen + 1..];
                let fields: Vec<&str> = rest.split_whitespace().collect();
                if fields.len() > 13 {
                    let utime: u64 = fields[11].parse().unwrap_or(0);
                    let stime: u64 = fields[12].parse().unwrap_or(0);
                    // Standard Linux USER_HZ is usually 100 ticks per second (10,000 us per tick)
                    return (utime + stime) * 10_000;
                }
            }
        }
        0
    }
}

/// Retrieves the current process resource usage (RAM in MB, CPU in %).
pub fn sample_metrics() -> ProcessMetrics {
    let (rss_bytes, virt_bytes) = os::get_memory_bytes();
    let ram_mb = rss_bytes as f64 / (1024.0 * 1024.0);
    let virtual_ram_mb = virt_bytes as f64 / (1024.0 * 1024.0);

    let current_cpu_us = os::get_cpu_time_us();
    let now = Instant::now();

    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as f64;

    let mut tracker = CPU_TRACKER.lock().unwrap();
    let cpu_percent = if let Some(ref prev) = *tracker {
        let elapsed_us = now.duration_since(prev.last_wall).as_micros() as f64;
        let delta_cpu_us = current_cpu_us.saturating_sub(prev.last_cpu_time_us) as f64;
        if elapsed_us > 1000.0 {
            // Normalized across all CPU cores
            let pct = (delta_cpu_us / (elapsed_us * num_cpus)) * 100.0;
            pct.clamp(0.0, 100.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    *tracker = Some(CpuState {
        last_wall: now,
        last_cpu_time_us: current_cpu_us,
    });

    ProcessMetrics {
        ram_mb,
        virtual_ram_mb,
        cpu_percent,
    }
}

/// Returns a human-friendly single-line string of system resources.
/// Example: "RAM: 24.8 MB (RSS) • CPU: 0.4%"
pub fn format_metrics_chip() -> String {
    let m = sample_metrics();
    format!("RAM: {:.1} MB • CPU: {:.1}%", m.ram_mb, m.cpu_percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_metrics() {
        let m = sample_metrics();
        assert!(m.ram_mb > 0.0, "RAM usage should be greater than 0");
        assert!(m.cpu_percent >= 0.0);
        let chip = format_metrics_chip();
        assert!(chip.contains("RAM:"));
        assert!(chip.contains("CPU:"));
    }
}
