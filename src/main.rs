#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod monitor;
mod tray;
mod ui;

use std::sync::Arc;
use parking_lot::RwLock;

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostQuitMessage, TranslateMessage, MSG,
};

use app::{AppConfig, AppState};
use monitor::spawn_monitoring_thread;
use tray::TrayManager;
use ui::DashboardWindow;

fn main() {
    // 1. Initialize COM
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    // 2. Load Configuration
    let config = AppConfig::load();
    let state = AppState::new(config.clone());

    // 3. Create Dashboard Window
    let dashboard = match DashboardWindow::new(state.metrics.clone(), state.is_running.clone()) {
        Ok(win) => win,
        Err(e) => {
            eprintln!("Failed to create dashboard window: {}", e);
            return;
        }
    };

    let ui_hwnd = Arc::new(RwLock::new(Some(dashboard.hwnd.0 as isize)));

    // 4. Spawn Background Monitoring Thread
    let monitor_handle = spawn_monitoring_thread(
        state.metrics.clone(),
        state.config.clone(),
        state.is_running.clone(),
        ui_hwnd.clone(),
    );

    // 5. Initialize System Tray
    let mut tray_manager = match TrayManager::new(state.config.clone()) {
        Ok(tm) => tm,
        Err(e) => {
            eprintln!("Failed to initialize tray icon: {}", e);
            state.is_running.store(false, std::sync::atomic::Ordering::Relaxed);
            let _ = monitor_handle.join();
            return;
        }
    };

    // 6. Windows Message Loop
    unsafe {
        let mut msg = MSG::default();

        while state.is_running.load(std::sync::atomic::Ordering::Relaxed) {
            // Check Windows messages
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if !ret.as_bool() || ret.0 == -1 {
                break;
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);

            // Check tray & menu events
            let keep_running = tray_manager.handle_events(
                &dashboard,
                state.config.clone(),
                state.metrics.clone(),
                state.is_running.clone(),
            );

            if !keep_running {
                state.is_running.store(false, std::sync::atomic::Ordering::Relaxed);
                PostQuitMessage(0);
                break;
            }
        }
    }

    // 7. Cleanup
    *ui_hwnd.write() = None;
    state.is_running.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = monitor_handle.join();

    unsafe {
        CoUninitialize();
    }
}
