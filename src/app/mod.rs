pub mod config;
pub mod state;

#[allow(unused_imports)]
pub use config::{AppConfig, TrayDisplayMode};
pub use state::{AppState, BenchmarkSession, ProcessInfo, SystemMetrics};
