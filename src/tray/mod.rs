use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use parking_lot::RwLock;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

use crate::app::config::{is_autostart_enabled, set_autostart, AppConfig};
use crate::app::SystemMetrics;
use crate::monitor::network::format_speed;
use crate::ui::DashboardWindow;

pub struct TrayManager {
    _tray: TrayIcon,
    open_id: MenuId,
    refresh_id: MenuId,
    settings_id: MenuId,
    autostart_item: CheckMenuItem,
    about_id: MenuId,
    exit_id: MenuId,
    last_tooltip: String,
}

impl TrayManager {
    pub fn new(config: Arc<RwLock<AppConfig>>) -> Result<Self, String> {
        let menu = Menu::new();

        let title_item = MenuItem::new("VeroStat", false, None);
        let open_item = MenuItem::new("Open Dashboard", true, None);
        let refresh_item = MenuItem::new("Refresh", true, None);
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
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&settings_item).map_err(|e| e.to_string())?;
        menu.append(&autostart_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&about_item).map_err(|e| e.to_string())?;
        menu.append(&exit_item).map_err(|e| e.to_string())?;

        let icon = load_or_create_icon()?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip("VeroStat - System Monitor")
            .build()
            .map_err(|e| format!("Failed to build tray icon: {:?}", e))?;

        // Synchronize config
        config.write().start_with_windows = initial_autostart;

        Ok(Self {
            _tray: tray,
            open_id,
            refresh_id,
            settings_id,
            autostart_item,
            about_id,
            exit_id,
            last_tooltip: String::new(),
        })
    }

    pub fn handle_events(
        &mut self,
        dashboard: &DashboardWindow,
        config: Arc<RwLock<AppConfig>>,
        metrics: Arc<RwLock<SystemMetrics>>,
        is_running: Arc<AtomicBool>,
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

        // Update Tooltip with complete metrics including temperatures
        let m = metrics.read();
        let cpu_usage_str = m.cpu_usage.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "N/A".to_string());
        let cpu_temp_str = m.cpu_temperature.map(|t| format!(" ({:.0}°C)", t)).unwrap_or_default();

        let gpu_usage_str = m.gpu_usage.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "N/A".to_string());
        let gpu_temp_str = m.gpu_temperature.map(|t| format!(" ({:.0}°C)", t)).unwrap_or_default();

        let ram_str = format!("{:.0}%", m.ram_usage);
        let net_str = format!("↓{} ↑{}", format_speed(m.download_speed), format_speed(m.upload_speed));

        let new_tooltip = format!(
            "VeroStat\nCPU: {}{} | RAM: {}\nGPU: {}{}\nNet: {}",
            cpu_usage_str, cpu_temp_str, ram_str,
            gpu_usage_str, gpu_temp_str,
            net_str
        );

        if new_tooltip != self.last_tooltip {
            let _ = self._tray.set_tooltip(Some(&new_tooltip));
            self.last_tooltip = new_tooltip;
        }

        true
    }
}

fn load_or_create_icon() -> Result<Icon, String> {
    let icon_path = Path::new("assets/icon.ico");
    if icon_path.exists() {
        if let Ok(icon) = Icon::from_path(icon_path, Some((32, 32))) {
            return Ok(icon);
        }
    }

    // Fallback: Generate 32x32 RGBA icon programmatically
    let width = 32u32;
    let height = 32u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            let cx = x as f32 - 15.5;
            let cy = y as f32 - 15.5;
            let dist = (cx * cx + cy * cy).sqrt();

            if dist <= 14.0 {
                // Vibrant Sky Blue (#38BDF8)
                rgba.extend_from_slice(&[56, 189, 248, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    Icon::from_rgba(rgba, width, height).map_err(|e| format!("Failed to create icon from rgba: {:?}", e))
}

fn show_about_dialog() {
    let title: Vec<u16> = "About VeroStat\0".encode_utf16().collect();
    let text: Vec<u16> = "VeroStat v0.1.0\n\n\
        A lightweight native Windows system-tray utility.\n\n\
        Features:\n\
        • Real-time CPU, GPU, RAM, & Network stats\n\
        • Hardware temperature & frequency monitoring\n\
        • Native Win32 dark dashboard\n\
        • Ultra-low resource usage\n\n\
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
    let msg = format!(
        "VeroStat Settings\n\n\
        • Refresh Interval: {} ms\n\
        • Start with Windows: {}\n\
        • High Temp Warning: {:.0}°C\n\
        • Show Storage Stats: {}\n\n\
        Configuration file is stored at:\n\
        {}\n\n\
        (Edit config.toml directly to change refresh rates)\0",
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
