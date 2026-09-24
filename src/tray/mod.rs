pub mod icon_gen;

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use parking_lot::RwLock;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

use crate::app::config::{is_autostart_enabled, set_autostart, AppConfig, TrayDisplayMode};
use crate::app::{BenchmarkSession, SystemMetrics};
use crate::monitor::network::format_speed;
use crate::ui::{CoreInspectorWindow, DashboardWindow, FloatingHud};
use icon_gen::{generate_logo_icon, generate_stat_icon};

pub struct TrayManager {
    _tray: TrayIcon,
    open_id: MenuId,
    inspector_id: MenuId,
    copy_specs_id: MenuId,
    refresh_id: MenuId,
    hud_item: CheckMenuItem,
    hud_elem_cpu: CheckMenuItem,
    hud_elem_gpu: CheckMenuItem,
    hud_elem_ram: CheckMenuItem,
    hud_elem_net: CheckMenuItem,
    hud_opacity_100: CheckMenuItem,
    hud_opacity_90: CheckMenuItem,
    hud_opacity_75: CheckMenuItem,
    hud_opacity_50: CheckMenuItem,
    hud_opacity_25: CheckMenuItem,
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
    last_toggle_time: Instant,
}

impl TrayManager {
    pub fn new(config: Arc<RwLock<AppConfig>>) -> Result<Self, String> {
        let menu = Menu::new();

        let title_item = MenuItem::new("VeroStat", false, None);
        let open_item = MenuItem::new("Open Dashboard", true, None);
        let inspector_item = MenuItem::new("CPU Core Inspector (Ctrl+Shift+C)", true, None);
        let copy_specs_item = MenuItem::new("📋 Copy System Specs", true, None);
        let refresh_item = MenuItem::new("Refresh", true, None);
        let hud_item = CheckMenuItem::new("In-Game HUD Overlay (Ctrl+Shift+O)", true, false, None);

        // Submenu: HUD Elements selection
        let cfg_read = config.read();
        let hud_elements_submenu = Submenu::new("HUD Elements", true);
        let hud_elem_cpu = CheckMenuItem::new("Show CPU", true, cfg_read.hud_show_cpu, None);
        let hud_elem_gpu = CheckMenuItem::new("Show GPU", true, cfg_read.hud_show_gpu, None);
        let hud_elem_ram = CheckMenuItem::new("Show RAM", true, cfg_read.hud_show_ram, None);
        let hud_elem_net = CheckMenuItem::new("Show Network", true, cfg_read.hud_show_network, None);

        hud_elements_submenu.append(&hud_elem_cpu).map_err(|e| e.to_string())?;
        hud_elements_submenu.append(&hud_elem_gpu).map_err(|e| e.to_string())?;
        hud_elements_submenu.append(&hud_elem_ram).map_err(|e| e.to_string())?;
        hud_elements_submenu.append(&hud_elem_net).map_err(|e| e.to_string())?;

        // Submenu: HUD Opacity / Transparency selection
        let cur_opacity = cfg_read.hud_opacity;
        let hud_opacity_submenu = Submenu::new("HUD Opacity", true);
        let hud_opacity_100 = CheckMenuItem::new("100% (Solid)", true, cur_opacity == 100, None);
        let hud_opacity_90 = CheckMenuItem::new("90% (Default)", true, cur_opacity == 90, None);
        let hud_opacity_75 = CheckMenuItem::new("75%", true, cur_opacity == 75, None);
        let hud_opacity_50 = CheckMenuItem::new("50%", true, cur_opacity == 50, None);
        let hud_opacity_25 = CheckMenuItem::new("25% (Ghost)", true, cur_opacity == 25, None);

        hud_opacity_submenu.append(&hud_opacity_100).map_err(|e| e.to_string())?;
        hud_opacity_submenu.append(&hud_opacity_90).map_err(|e| e.to_string())?;
        hud_opacity_submenu.append(&hud_opacity_75).map_err(|e| e.to_string())?;
        hud_opacity_submenu.append(&hud_opacity_50).map_err(|e| e.to_string())?;
        hud_opacity_submenu.append(&hud_opacity_25).map_err(|e| e.to_string())?;

        let benchmark_item = MenuItem::new("▶ Start Benchmark Log", true, None);

        // Submenu: Tray Icon Display mode selection
        let current_mode = cfg_read.tray_display_mode;
        drop(cfg_read);
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
        let inspector_id = inspector_item.id().clone();
        let copy_specs_id = copy_specs_item.id().clone();
        let refresh_id = refresh_item.id().clone();
        let settings_id = settings_item.id().clone();
        let about_id = about_item.id().clone();
        let exit_id = exit_item.id().clone();

        menu.append(&title_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&open_item).map_err(|e| e.to_string())?;
        menu.append(&inspector_item).map_err(|e| e.to_string())?;
        menu.append(&copy_specs_item).map_err(|e| e.to_string())?;
        menu.append(&refresh_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&hud_item).map_err(|e| e.to_string())?;
        menu.append(&hud_elements_submenu).map_err(|e| e.to_string())?;
        menu.append(&hud_opacity_submenu).map_err(|e| e.to_string())?;
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
            inspector_id,
            copy_specs_id,
            refresh_id,
            hud_item,
            hud_elem_cpu,
            hud_elem_gpu,
            hud_elem_ram,
            hud_elem_net,
            hud_opacity_100,
            hud_opacity_90,
            hud_opacity_75,
            hud_opacity_50,
            hud_opacity_25,
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
            last_toggle_time: Instant::now(),
        })
    }

    pub fn set_hud_checked(&self, checked: bool) {
        self.hud_item.set_checked(checked);
    }

    pub fn handle_events(
        &mut self,
        dashboard: &DashboardWindow,
        hud: &FloatingHud,
        inspector: &CoreInspectorWindow,
        config: Arc<RwLock<AppConfig>>,
        metrics: Arc<RwLock<SystemMetrics>>,
        is_running: Arc<AtomicBool>,
        benchmark_session: Arc<RwLock<Option<BenchmarkSession>>>,
    ) -> bool {
        // Handle Tray Left Click with debouncing and queue drain
        let mut left_clicked = false;
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                left_clicked = true;
            }
        }

        if left_clicked {
            let now = Instant::now();
            if now.duration_since(self.last_toggle_time) > Duration::from_millis(250) {
                dashboard.toggle_visibility();
                self.last_toggle_time = now;
            }
        }

        // Handle Menu Events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.open_id || event.id == self.refresh_id {
                while TrayIconEvent::receiver().try_recv().is_ok() {}
                self.last_toggle_time = Instant::now();
                dashboard.show();
            } else if event.id == self.inspector_id {
                inspector.toggle_visibility();
            } else if event.id == self.copy_specs_id {
                let specs = crate::app::format_system_specs(&metrics.read());
                if let Err(e) = crate::app::copy_to_clipboard(&specs) {
                    show_message_box("VeroStat - Error", &format!("Failed to copy specs to clipboard: {}", e));
                } else {
                    show_message_box("VeroStat", "System specifications copied to clipboard!\n\nYou can now paste (Ctrl+V) it anywhere.");
                }
            } else if event.id == self.hud_item.id() {
                hud.toggle_visibility();
                self.hud_item.set_checked(hud.is_visible());
            } else if event.id == self.hud_elem_cpu.id() {
                let checked = !config.read().hud_show_cpu;
                self.hud_elem_cpu.set_checked(checked);
                config.write().hud_show_cpu = checked;
                let _ = config.read().save();
                unsafe { let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hud.hwnd, None, false); }
            } else if event.id == self.hud_elem_gpu.id() {
                let checked = !config.read().hud_show_gpu;
                self.hud_elem_gpu.set_checked(checked);
                config.write().hud_show_gpu = checked;
                let _ = config.read().save();
                unsafe { let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hud.hwnd, None, false); }
            } else if event.id == self.hud_elem_ram.id() {
                let checked = !config.read().hud_show_ram;
                self.hud_elem_ram.set_checked(checked);
                config.write().hud_show_ram = checked;
                let _ = config.read().save();
                unsafe { let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hud.hwnd, None, false); }
            } else if event.id == self.hud_elem_net.id() {
                let checked = !config.read().hud_show_network;
                self.hud_elem_net.set_checked(checked);
                config.write().hud_show_network = checked;
                let _ = config.read().save();
                unsafe { let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hud.hwnd, None, false); }
            } else if event.id == self.hud_opacity_100.id() {
                self.set_hud_opacity(100, &config, hud);
            } else if event.id == self.hud_opacity_90.id() {
                self.set_hud_opacity(90, &config, hud);
            } else if event.id == self.hud_opacity_75.id() {
                self.set_hud_opacity(75, &config, hud);
            } else if event.id == self.hud_opacity_50.id() {
                self.set_hud_opacity(50, &config, hud);
            } else if event.id == self.hud_opacity_25.id() {
                self.set_hud_opacity(25, &config, hud);
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

        self.last_icon_key = (mode, 9999);
    }

    fn set_hud_opacity(&mut self, pct: u8, config: &Arc<RwLock<AppConfig>>, hud: &FloatingHud) {
        self.hud_opacity_100.set_checked(pct == 100);
        self.hud_opacity_90.set_checked(pct == 90);
        self.hud_opacity_75.set_checked(pct == 75);
        self.hud_opacity_50.set_checked(pct == 50);
        self.hud_opacity_25.set_checked(pct == 25);

        hud.set_opacity(pct);

        let mut cfg = config.write();
        cfg.hud_opacity = pct;
        let _ = cfg.save();
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
    let msg = format!(
        "VeroStat v{}\n\n\
        A lightweight native Windows system-tray utility.\n\n\
        Features:\n\
        • Real-time CPU, GPU, RAM, & Network stats\n\
        • In-Game Floating HUD Overlay (Ctrl+Shift+O)\n\
        • Customizable HUD Elements & Opacity\n\
        • Dynamic Hardware Brand Theming (Intel/AMD/NVIDIA)\n\
        • Global Hotkeys (Win+Shift+V / Ctrl+Shift+O)\n\
        • Customizable taskbar icon\n\
        • Top 3 Resource Hogs process viewer\n\
        • Benchmark session CSV logging & peak reporting\n\
        • Native Win32 dark dashboard\n\
        • Ultra-low resource usage (< 45 MB RAM, 0% CPU)\n\n\
        Press Esc or Close button to minimize to tray.\0",
        env!("CARGO_PKG_VERSION")
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
        • In-Game HUD: CPU {}, GPU {}, RAM {}, Net {} (Opacity: {}%)\n\
        • Refresh Interval: {} ms\n\
        • Start with Windows: {}\n\
        • High Temp Warning: {:.0}°C\n\
        • Show Storage Stats: {}\n\n\
        Hotkeys:\n\
        • Win + Shift + V : Toggle Dashboard\n\
        • Ctrl + Shift + O : Toggle In-Game HUD Overlay\n\n\
        Configuration file is stored at:\n\
        {}\0",
        mode_str,
        if cfg.hud_show_cpu { "ON" } else { "OFF" },
        if cfg.hud_show_gpu { "ON" } else { "OFF" },
        if cfg.hud_show_ram { "ON" } else { "OFF" },
        if cfg.hud_show_network { "ON" } else { "OFF" },
        cfg.hud_opacity,
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
