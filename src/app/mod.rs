pub mod config;
pub mod specs;
pub mod state;

#[allow(unused_imports)]
pub use config::{AppConfig, TrayDisplayMode};
pub use specs::{copy_to_clipboard, format_system_specs, format_uptime};
pub use state::{AppState, BenchmarkSession, CoreInfo, ProcessInfo, SystemMetrics};
