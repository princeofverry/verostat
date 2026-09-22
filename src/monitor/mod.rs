pub mod cpu;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod sensors;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use sysinfo::{Disks, System};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_USER};

pub use cpu::CpuMonitor;
pub use gpu::GpuMonitor;
pub use memory::MemoryMonitor;
pub use network::NetworkMonitor;
pub use sensors::SensorMonitor;

use crate::app::{AppConfig, SystemMetrics};

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
        // Refresh sysinfo core
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

            updated_at: Instant::now(),
        }
    }
}

/// Spawns the background monitoring thread.
pub fn spawn_monitoring_thread(
    metrics_sink: Arc<RwLock<SystemMetrics>>,
    config: Arc<RwLock<AppConfig>>,
    is_running: Arc<AtomicBool>,
    ui_hwnd: Arc<RwLock<Option<isize>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut coordinator = MonitorCoordinator::new();

        // Take initial snapshot immediately
        let initial_metrics = coordinator.sample();
        *metrics_sink.write() = initial_metrics;

        while is_running.load(Ordering::Relaxed) {
            let interval_ms = {
                let cfg = config.read();
                cfg.refresh_interval_ms.max(250)
            };

            // Sleep in short chunks to allow rapid shutdown
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

            // Collect metrics
            let snapshot = coordinator.sample();
            *metrics_sink.write() = snapshot;

            // Notify UI if window exists
            if let Some(hwnd_raw) = *ui_hwnd.read() {
                unsafe {
                    let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);
                    let _ = PostMessageW(hwnd, WM_METRICS_UPDATED, WPARAM(0), LPARAM(0));
                }
            }
        }
    })
}
