use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use parking_lot::RwLock;

use super::config::AppConfig;

/// System metrics snapshot containing hardware and network data.
#[derive(Debug, Clone)]
pub struct SystemMetrics {
    // CPU
    pub cpu_usage: Option<f32>,
    pub cpu_temperature: Option<f32>,
    pub cpu_frequency: Option<f32>, // in GHz
    pub cpu_name: String,

    // GPU
    pub gpu_usage: Option<f32>,
    pub gpu_temperature: Option<f32>,
    pub gpu_memory_used: Option<u64>,  // in bytes
    pub gpu_memory_total: Option<u64>, // in bytes
    pub gpu_name: Option<String>,

    // RAM
    pub ram_used: u64,  // in bytes
    pub ram_total: u64, // in bytes
    pub ram_usage: f32, // percentage 0.0 - 100.0

    // Network
    pub download_speed: u64, // bytes per second
    pub upload_speed: u64,   // bytes per second

    // Storage
    pub disk_used: u64,  // in bytes
    pub disk_total: u64, // in bytes
    pub disk_usage: f32, // percentage 0.0 - 100.0

    #[allow(dead_code)]
    pub updated_at: Instant,
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self {
            cpu_usage: None,
            cpu_temperature: None,
            cpu_frequency: None,
            cpu_name: String::from("Detecting CPU..."),

            gpu_usage: None,
            gpu_temperature: None,
            gpu_memory_used: None,
            gpu_memory_total: None,
            gpu_name: None,

            ram_used: 0,
            ram_total: 0,
            ram_usage: 0.0,

            download_speed: 0,
            upload_speed: 0,

            disk_used: 0,
            disk_total: 0,
            disk_usage: 0.0,

            updated_at: Instant::now(),
        }
    }
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub metrics: Arc<RwLock<SystemMetrics>>,
    pub config: Arc<RwLock<AppConfig>>,
    pub is_running: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            metrics: Arc::new(RwLock::new(SystemMetrics::default())),
            config: Arc::new(RwLock::new(config)),
            is_running: Arc::new(AtomicBool::new(true)),
        }
    }
}
