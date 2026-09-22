pub mod cpu;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod sensors;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use sysinfo::{Disks, ProcessRefreshKind, ProcessesToUpdate, System};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_USER};

pub use cpu::CpuMonitor;
pub use gpu::GpuMonitor;
pub use memory::MemoryMonitor;
pub use network::NetworkMonitor;
pub use sensors::SensorMonitor;

use crate::app::{AppConfig, BenchmarkSession, ProcessInfo, SystemMetrics};

pub const WM_METRICS_UPDATED: u32 = WM_USER + 101;

pub struct MonitorCoordinator {
    sys: System,
    cpu_monitor: CpuMonitor,
    mem_monitor: MemoryMonitor,
    net_monitor: NetworkMonitor,
    gpu_monitor: GpuMonitor,
    sensor_monitor: SensorMonitor,
    disks: Disks,
}

impl MonitorCoordinator {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();

        Self {
            sys,
            cpu_monitor: CpuMonitor::new(),
            mem_monitor: MemoryMonitor::new(),
            net_monitor: NetworkMonitor::new(),
            gpu_monitor: GpuMonitor::new(),
            sensor_monitor: SensorMonitor::new(),
            disks: Disks::new_with_refreshed_list(),
        }
    }

    pub fn sample(&mut self) -> SystemMetrics {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();

        // 1. CPU
        let cpu_data = self.cpu_monitor.sample(&self.sys);
        let cpu_temp = self.sensor_monitor.sample_cpu_temp();

        // 2. RAM
        let mem_data = self.mem_monitor.sample(&self.sys);

        // 3. Network
        let net_data = self.net_monitor.sample();

        // 4. GPU
        let gpu_data = self.gpu_monitor.sample();

        // 5. Disks
        self.disks.refresh(true);
        let mut disk_total = 0u64;
        let mut disk_available = 0u64;
        for disk in &self.disks {
            disk_total += disk.total_space();
            disk_available += disk.available_space();
        }
        let disk_used = disk_total.saturating_sub(disk_available);
        let disk_usage = if disk_total > 0 {
            (disk_used as f32 / disk_total as f32) * 100.0
        } else {
            0.0
        };

        // 6. Top 3 Resource Hogs
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );

        let mut top_processes: Vec<ProcessInfo> = self
            .sys
            .processes()
            .values()
            .filter(|p| {
                let name = p.name().to_string_lossy();
                name != "System Idle Process" && name != "System" && !name.is_empty()
            })
            .map(|p| ProcessInfo {
                name: p.name().to_string_lossy().to_string(),
                cpu_usage: p.cpu_usage(),
                memory_bytes: p.memory(),
            })
            .collect();

        top_processes.sort_by(|a, b| {
            b.cpu_usage
                .partial_cmp(&a.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_processes.truncate(3);

        SystemMetrics {
            cpu_usage: Some(cpu_data.usage),
            cpu_temperature: cpu_temp,
            cpu_frequency: cpu_data.frequency_ghz,
            cpu_name: cpu_data.model,

            gpu_usage: gpu_data.as_ref().and_then(|g| g.usage),
            gpu_temperature: gpu_data.as_ref().and_then(|g| g.temperature),
            gpu_memory_used: gpu_data.as_ref().and_then(|g| g.memory_used),
            gpu_memory_total: gpu_data.as_ref().and_then(|g| g.memory_total),
            gpu_name: gpu_data.map(|g| g.name),

            ram_used: mem_data.used,
            ram_total: mem_data.total,
            ram_usage: mem_data.percentage,

            download_speed: net_data.download_speed,
            upload_speed: net_data.upload_speed,

            disk_used,
            disk_total,
            disk_usage,

            top_processes,

            updated_at: Instant::now(),
        }
    }
}

/// Spawns the background monitoring thread.
pub fn spawn_monitoring_thread(
    metrics_sink: Arc<RwLock<SystemMetrics>>,
    config: Arc<RwLock<AppConfig>>,
    is_running: Arc<AtomicBool>,
    ui_hwnds: Arc<RwLock<Vec<isize>>>,
    benchmark_session: Arc<RwLock<Option<BenchmarkSession>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut coordinator = MonitorCoordinator::new();

        let initial_metrics = coordinator.sample();
        *metrics_sink.write() = initial_metrics;

        while is_running.load(Ordering::Relaxed) {
            let interval_ms = {
                let cfg = config.read();
                cfg.refresh_interval_ms.max(250)
            };

            let chunk = Duration::from_millis(100);
            let elapsed_target = Duration::from_millis(interval_ms);
            let start = Instant::now();

            while start.elapsed() < elapsed_target {
                if !is_running.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(chunk);
            }

            if !is_running.load(Ordering::Relaxed) {
                return;
            }

            let snapshot = coordinator.sample();

            if let Some(session) = benchmark_session.write().as_mut() {
                session.sample_count += 1;
                let cpu_u = snapshot.cpu_usage.unwrap_or(0.0);
                let cpu_t = snapshot.cpu_temperature.unwrap_or(0.0);
                let gpu_u = snapshot.gpu_usage.unwrap_or(0.0);
                let gpu_t = snapshot.gpu_temperature.unwrap_or(0.0);
                let ram_u = snapshot.ram_usage;
                let ram_mb = snapshot.ram_used / 1024 / 1024;
                let dl_kb = snapshot.download_speed / 1024;
                let ul_kb = snapshot.upload_speed / 1024;

                session.peak_cpu_temp = session.peak_cpu_temp.max(cpu_t);
                session.peak_gpu_temp = session.peak_gpu_temp.max(gpu_t);
                session.peak_cpu_usage = session.peak_cpu_usage.max(cpu_u);
                session.peak_gpu_usage = session.peak_gpu_usage.max(gpu_u);
                session.sum_cpu_usage += cpu_u as f64;
                session.sum_gpu_usage += gpu_u as f64;

                let elapsed_secs = session.start_time.elapsed().as_secs();
                let _ = writeln!(
                    session.file,
                    "{},{:.1},{:.1},{:.1},{:.1},{:.1},{},{},{}",
                    elapsed_secs, cpu_u, cpu_t, gpu_u, gpu_t, ram_u, ram_mb, dl_kb, ul_kb
                );
            }

            *metrics_sink.write() = snapshot;

            // Notify all active UI windows (Dashboard and HUD)
            for &hwnd_raw in ui_hwnds.read().iter() {
                unsafe {
                    let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);
                    let _ = PostMessageW(hwnd, WM_METRICS_UPDATED, WPARAM(0), LPARAM(0));
                }
            }
        }
    })
}
