pub mod icon_gen;

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use parking_lot::RwLock;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

use crate::app::config::{is_autostart_enabled, set_autostart, AppConfig, TrayDisplayMode};
use crate::app::{BenchmarkSession, SystemMetrics};
use crate::monitor::network::format_speed;
use crate::ui::DashboardWindow;
use icon_gen::{generate_logo_icon, generate_stat_icon};

pub struct TrayManager {
    _tray: TrayIcon,
    open_id: MenuId,
    refresh_id: MenuId,
    benchmark_item: MenuItem,
    display_cpu_usage: CheckMenuItem,
    display_cpu_temp: CheckMenuItem,
    display_gpu_usage: CheckMenuItem,
    display_gpu_temp: CheckMenuItem,
    display_ram_usage: CheckMenuItem,
    display_default_logo: CheckMenuItem,
    settings_id: MenuId,
    autostart_item: CheckMenuItem,
    about_id: MenuId,
    exit_id: MenuId,
    last_tooltip: String,
    last_icon_key: (TrayDisplayMode, u32),
}

impl TrayManager {
    pub fn new(config: Arc<RwLock<AppConfig>>) -> Result<Self, String> {
        let menu = Menu::new();

        let title_item = MenuItem::new("VeroStat", false, None);
        let open_item = MenuItem::new("Open Dashboard", true, None);
        let refresh_item = MenuItem::new("Refresh", true, None);
        let benchmark_item = MenuItem::new("▶ Start Benchmark Log", true, None);

        // Submenu: Tray Icon Display mode selection
        let current_mode = config.read().tray_display_mode;
        let display_submenu = Submenu::new("Tray Icon Display", true);

        let display_cpu_usage = CheckMenuItem::new(
            "CPU Usage (%)",
            true,
            current_mode == TrayDisplayMode::CpuUsage,
            None,
        );
        let display_cpu_temp = CheckMenuItem::new(
            "CPU Temperature (°C)",
            true,
            current_mode == TrayDisplayMode::CpuTemperature,
            None,
        );
        let display_gpu_usage = CheckMenuItem::new(
            "GPU Usage (%)",
            true,
            current_mode == TrayDisplayMode::GpuUsage,
            None,
        );
        let display_gpu_temp = CheckMenuItem::new(
            "GPU Temperature (°C)",
            true,
            current_mode == TrayDisplayMode::GpuTemperature,
            None,
        );
        let display_ram_usage = CheckMenuItem::new(
            "RAM Usage (%)",
            true,
            current_mode == TrayDisplayMode::RamUsage,
            None,
        );
        let display_default_logo = CheckMenuItem::new(
            "Default Logo",
            true,
            current_mode == TrayDisplayMode::DefaultLogo,
            None,
        );

        display_submenu.append(&display_cpu_usage).map_err(|e| e.to_string())?;
        display_submenu.append(&display_cpu_temp).map_err(|e| e.to_string())?;
        display_submenu.append(&display_gpu_usage).map_err(|e| e.to_string())?;
        display_submenu.append(&display_gpu_temp).map_err(|e| e.to_string())?;
        display_submenu.append(&display_ram_usage).map_err(|e| e.to_string())?;
        display_submenu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        display_submenu.append(&display_default_logo).map_err(|e| e.to_string())?;

        let settings_item = MenuItem::new("Settings...", true, None);

        let initial_autostart = is_autostart_enabled();
        let autostart_item = CheckMenuItem::new("Start with Windows", true, initial_autostart, None);

        let about_item = MenuItem::new("About", true, None);
        let exit_item = MenuItem::new("Exit", true, None);

        let open_id = open_item.id().clone();
        let refresh_id = refresh_item.id().clone();
        let settings_id = settings_item.id().clone();
        let about_id = about_item.id().clone();
        let exit_id = exit_item.id().clone();

        menu.append(&title_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&open_item).map_err(|e| e.to_string())?;
        menu.append(&refresh_item).map_err(|e| e.to_string())?;
        menu.append(&benchmark_item).map_err(|e| e.to_string())?;
        menu.append(&display_submenu).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&settings_item).map_err(|e| e.to_string())?;
        menu.append(&autostart_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&about_item).map_err(|e| e.to_string())?;
        menu.append(&exit_item).map_err(|e| e.to_string())?;

        let icon = load_or_create_icon(current_mode)?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip("VeroStat - System Monitor")
            .build()
            .map_err(|e| format!("Failed to build tray icon: {:?}", e))?;

        config.write().start_with_windows = initial_autostart;

        Ok(Self {
            _tray: tray,
            open_id,
            refresh_id,
            benchmark_item,
            display_cpu_usage,
            display_cpu_temp,
            display_gpu_usage,
            display_gpu_temp,
            display_ram_usage,
            display_default_logo,
            settings_id,
            autostart_item,
            about_id,
            exit_id,
            last_tooltip: String::new(),
            last_icon_key: (current_mode, 999),
        })
    }

    pub fn handle_events(
        &mut self,
        dashboard: &DashboardWindow,
        config: Arc<RwLock<AppConfig>>,
        metrics: Arc<RwLock<SystemMetrics>>,
        is_running: Arc<AtomicBool>,
        benchmark_session: Arc<RwLock<Option<BenchmarkSession>>>,
    ) -> bool {
        // Handle Tray Left Click
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                dashboard.toggle_visibility();
            }
        }

        // Handle Menu Events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.open_id {
                dashboard.show();
            } else if event.id == self.refresh_id {
                dashboard.show();
            } else if event.id == self.benchmark_item.id() {
                toggle_benchmark_session(&self.benchmark_item, &benchmark_session);
            } else if event.id == self.display_cpu_usage.id() {
                self.set_display_mode(TrayDisplayMode::CpuUsage, &config);
            } else if event.id == self.display_cpu_temp.id() {
                self.set_display_mode(TrayDisplayMode::CpuTemperature, &config);
            } else if event.id == self.display_gpu_usage.id() {
                self.set_display_mode(TrayDisplayMode::GpuUsage, &config);
            } else if event.id == self.display_gpu_temp.id() {
                self.set_display_mode(TrayDisplayMode::GpuTemperature, &config);
            } else if event.id == self.display_ram_usage.id() {
                self.set_display_mode(TrayDisplayMode::RamUsage, &config);
            } else if event.id == self.display_default_logo.id() {
                self.set_display_mode(TrayDisplayMode::DefaultLogo, &config);
            } else if event.id == self.settings_id {
                show_settings_dialog(&config);
            } else if event.id == self.autostart_item.id() {
                let current_checked = self.autostart_item.is_checked();
                let new_checked = !current_checked;
                self.autostart_item.set_checked(new_checked);

                if let Err(err) = set_autostart(new_checked) {
                    show_message_box("VeroStat - Error", &format!("Failed to update autostart: {}", err));
                    self.autostart_item.set_checked(current_checked);
                } else {
                    let mut cfg = config.write();
                    cfg.start_with_windows = new_checked;
                    let _ = cfg.save();
                }
            } else if event.id == self.about_id {
                show_about_dialog();
            } else if event.id == self.exit_id {
                is_running.store(false, Ordering::Relaxed);
                return false; // Exit signal
            }
        }

        // 1. Update Tooltip with complete metrics including temperatures
        let m = metrics.read();
        let cpu_usage_str = m.cpu_usage.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "N/A".to_string());
        let cpu_temp_str = m.cpu_temperature.map(|t| format!(" ({:.0}°C)", t)).unwrap_or_default();

        let gpu_usage_str = m.gpu_usage.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "N/A".to_string());
        let gpu_temp_str = m.gpu_temperature.map(|t| format!(" ({:.0}°C)", t)).unwrap_or_default();

        let ram_str = format!("{:.0}%", m.ram_usage);
        let net_str = format!("↓{} ↑{}", format_speed(m.download_speed), format_speed(m.upload_speed));

        let is_rec = benchmark_session.read().is_some();
        let rec_tag = if is_rec { " [REC]" } else { "" };

        let new_tooltip = format!(
            "VeroStat{}\nCPU: {}{} | RAM: {}\nGPU: {}{}\nNet: {}",
            rec_tag,
            cpu_usage_str, cpu_temp_str, ram_str,
            gpu_usage_str, gpu_temp_str,
            net_str
        );

        if new_tooltip != self.last_tooltip {
            let _ = self._tray.set_tooltip(Some(&new_tooltip));
            self.last_tooltip = new_tooltip;
        }

        // 2. Update Dynamic Tray Icon based on user selected mode
        let current_mode = config.read().tray_display_mode;
        let (val, is_temp) = match current_mode {
            TrayDisplayMode::CpuUsage => (m.cpu_usage.map(|u| u.round() as u32).unwrap_or(0), false),
            TrayDisplayMode::CpuTemperature => (m.cpu_temperature.map(|t| t.round() as u32).unwrap_or(0), true),
            TrayDisplayMode::GpuUsage => (m.gpu_usage.map(|u| u.round() as u32).unwrap_or(0), false),
            TrayDisplayMode::GpuTemperature => (m.gpu_temperature.map(|t| t.round() as u32).unwrap_or(0), true),
            TrayDisplayMode::RamUsage => (m.ram_usage.round() as u32, false),
            TrayDisplayMode::DefaultLogo => (0, false),
        };

        let current_key = (current_mode, val);
        if self.last_icon_key != current_key {
            let icon_res = if current_mode == TrayDisplayMode::DefaultLogo {
                load_default_logo_icon()
            } else {
                generate_stat_icon(val, is_temp)
            };

            if let Ok(dyn_icon) = icon_res {
                let _ = self._tray.set_icon(Some(dyn_icon));
                self.last_icon_key = current_key;
            }
        }

        true
    }

    fn set_display_mode(&mut self, mode: TrayDisplayMode, config: &Arc<RwLock<AppConfig>>) {
        self.display_cpu_usage.set_checked(mode == TrayDisplayMode::CpuUsage);
        self.display_cpu_temp.set_checked(mode == TrayDisplayMode::CpuTemperature);
        self.display_gpu_usage.set_checked(mode == TrayDisplayMode::GpuUsage);
        self.display_gpu_temp.set_checked(mode == TrayDisplayMode::GpuTemperature);
        self.display_ram_usage.set_checked(mode == TrayDisplayMode::RamUsage);
        self.display_default_logo.set_checked(mode == TrayDisplayMode::DefaultLogo);

        {
            let mut cfg = config.write();
            cfg.tray_display_mode = mode;
            let _ = cfg.save();
        }

        // Force icon refresh on next tick
        self.last_icon_key = (mode, 9999);
    }
}

fn toggle_benchmark_session(
    item: &MenuItem,
    session_lock: &Arc<RwLock<Option<BenchmarkSession>>>,
) {
    let mut guard = session_lock.write();

    if let Some(mut session) = guard.take() {
        let _ = session.file.flush();
        drop(session.file);

        let duration = session.start_time.elapsed();
        let dur_secs = duration.as_secs();
        let mins = dur_secs / 60;
        let secs = dur_secs % 60;

        let avg_cpu = if session.sample_count > 0 {
            session.sum_cpu_usage / session.sample_count as f64
        } else {
            0.0
        };

        let avg_gpu = if session.sample_count > 0 {
            session.sum_gpu_usage / session.sample_count as f64
        } else {
            0.0
        };

        item.set_text("▶ Start Benchmark Log");

        let summary = format!(
            "VeroStat Benchmark Summary\n\n\
            • Duration: {}m {}s ({} samples)\n\
            • CPU Peak Temp: {:.0}°C\n\
            • CPU Peak Load: {:.0}% (Avg: {:.1}%)\n\
            • GPU Peak Temp: {:.0}°C\n\
            • GPU Peak Load: {:.0}% (Avg: {:.1}%)\n\n\
            Session log saved to:\n\
            {}",
            mins, secs, session.sample_count,
            session.peak_cpu_temp, session.peak_cpu_usage, avg_cpu,
            session.peak_gpu_temp, session.peak_gpu_usage, avg_gpu,
            session.file_path.to_string_lossy()
        );

        show_message_box("VeroStat - Benchmark Finished", &summary);
    } else {
        let path = get_log_file_path();
        if let Ok(mut file) = File::create(&path) {
            let _ = writeln!(
                file,
                "Time_Sec,CPU_Load_Pct,CPU_Temp_C,GPU_Load_Pct,GPU_Temp_C,RAM_Pct,RAM_Used_MB,Net_Down_KBps,Net_Up_KBps"
            );

            *guard = Some(BenchmarkSession {
                file_path: path.clone(),
                file,
                start_time: Instant::now(),
                sample_count: 0,
                peak_cpu_temp: 0.0,
                peak_gpu_temp: 0.0,
                peak_cpu_usage: 0.0,
                peak_gpu_usage: 0.0,
                sum_cpu_usage: 0.0,
                sum_gpu_usage: 0.0,
            });

            item.set_text("⏹ Stop Benchmark Log (REC)");
            show_message_box(
                "VeroStat - Logging Started",
                &format!("Recording hardware metrics every second to:\n\n{}", path.to_string_lossy()),
            );
        } else {
            show_message_box("VeroStat - Error", "Failed to create benchmark CSV log file.");
        }
    }
}

fn get_log_file_path() -> PathBuf {
    let now = std::time::SystemTime::now();
    let epoch = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    let mut path = if let Some(userprofile) = std::env::var_os("USERPROFILE") {
        let mut p = PathBuf::from(userprofile);
        p.push("Desktop");
        if p.exists() {
            p
        } else {
            std::env::temp_dir()
        }
    } else {
        std::env::temp_dir()
    };

    path.push(format!("verostat_session_{}.csv", epoch));
    path
}

fn load_or_create_icon(mode: TrayDisplayMode) -> Result<Icon, String> {
    if mode == TrayDisplayMode::DefaultLogo {
        load_default_logo_icon()
    } else {
        generate_stat_icon(0, mode == TrayDisplayMode::CpuTemperature || mode == TrayDisplayMode::GpuTemperature)
    }
}

fn load_default_logo_icon() -> Result<Icon, String> {
    let icon_path = Path::new("assets/icon.ico");
    if icon_path.exists() {
        if let Ok(icon) = Icon::from_path(icon_path, Some((32, 32))) {
            return Ok(icon);
        }
    }
    generate_logo_icon()
}

fn show_about_dialog() {
    let title: Vec<u16> = "About VeroStat\0".encode_utf16().collect();
    let text: Vec<u16> = "VeroStat v0.1.0\n\n\
        A lightweight native Windows system-tray utility.\n\n\
        Features:\n\
        • Real-time CPU, GPU, RAM, & Network stats\n\
        • Customizable taskbar icon (CPU %, Temp, GPU %, RAM, or Logo)\n\
        • Top 3 Resource Hogs process viewer\n\
        • Benchmark session CSV logging & peak reporting\n\
        • Native Win32 dark dashboard\n\
        • Ultra-low resource usage (< 45 MB RAM, 0% CPU)\n\n\
        Press Esc or Close button to minimize to tray.\0"
        .encode_utf16()
        .collect();

    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn show_settings_dialog(config: &Arc<RwLock<AppConfig>>) {
    let cfg = config.read();
    let title: Vec<u16> = "VeroStat - Settings\0".encode_utf16().collect();
    let mode_str = match cfg.tray_display_mode {
        TrayDisplayMode::CpuUsage => "CPU Usage (%)",
        TrayDisplayMode::CpuTemperature => "CPU Temperature (°C)",
        TrayDisplayMode::GpuUsage => "GPU Usage (%)",
        TrayDisplayMode::GpuTemperature => "GPU Temperature (°C)",
        TrayDisplayMode::RamUsage => "RAM Usage (%)",
        TrayDisplayMode::DefaultLogo => "Default Logo",
    };

    let msg = format!(
        "VeroStat Settings\n\n\
        • Tray Icon Display: {}\n\
        • Refresh Interval: {} ms\n\
        • Start with Windows: {}\n\
        • High Temp Warning: {:.0}°C\n\
        • Show Storage Stats: {}\n\n\
        Configuration file is stored at:\n\
        {}\n\n\
        (You can switch Tray Icon Display directly from the tray menu!)\0",
        mode_str,
        cfg.refresh_interval_ms,
        if cfg.start_with_windows { "Enabled" } else { "Disabled" },
        cfg.high_temp_threshold,
        if cfg.show_disk_stats { "Yes" } else { "No" },
        AppConfig::config_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "%APPDATA%\\VeroStat\\config.toml".to_string())
    );

    let text: Vec<u16> = msg.encode_utf16().collect();

    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn show_message_box(title: &str, message: &str) {
    let title_w: Vec<u16> = format!("{}\0", title).encode_utf16().collect();
    let text_w: Vec<u16> = format!("{}\0", message).encode_utf16().collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
