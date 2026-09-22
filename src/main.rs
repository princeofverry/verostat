#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod monitor;
mod tray;
mod ui;

use std::sync::Arc;
use parking_lot::RwLock;

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostQuitMessage, TranslateMessage, MSG, WM_HOTKEY,
};

use app::{AppConfig, AppState};
use monitor::spawn_monitoring_thread;
use tray::TrayManager;
use ui::{DashboardWindow, FloatingHud};

const HOTKEY_ID_DASHBOARD: i32 = 1001;
const HOTKEY_ID_OVERLAY: i32 = 1002;

fn main() {
    // 1. Initialize COM
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    // 2. Load Configuration
    let config = AppConfig::load();
    let state = AppState::new(config.clone());

    // 3. Create Dashboard Window & In-Game Floating HUD
    let dashboard = match DashboardWindow::new(state.metrics.clone(), state.is_running.clone()) {
        Ok(win) => win,
        Err(e) => {
            eprintln!("Failed to create dashboard window: {}", e);
            return;
        }
    };

    let hud = match FloatingHud::new(state.metrics.clone(), state.config.clone()) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to create Floating HUD: {}", e);
            return;
        }
    };

    let ui_hwnds = Arc::new(RwLock::new(vec![
        dashboard.hwnd.0 as isize,
        hud.hwnd.0 as isize,
    ]));

    // 4. Spawn Background Monitoring Thread
    let monitor_handle = spawn_monitoring_thread(
        state.metrics.clone(),
        state.config.clone(),
        state.is_running.clone(),
        ui_hwnds.clone(),
        state.benchmark_session.clone(),
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

    // 6. Register Global Hotkeys
    // Win + Shift + V => Toggle Dashboard
    // Ctrl + Shift + O => Toggle In-Game HUD Overlay
    unsafe {
        let _ = RegisterHotKey(
            None,
            HOTKEY_ID_DASHBOARD,
            MOD_WIN | MOD_SHIFT | MOD_NOREPEAT,
            0x56, // 'V'
        );
        let _ = RegisterHotKey(
            None,
            HOTKEY_ID_OVERLAY,
            MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
            0x4F, // 'O'
        );
    }

    // 7. Windows Message Loop
    unsafe {
        let mut msg = MSG::default();

        while state.is_running.load(std::sync::atomic::Ordering::Relaxed) {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if !ret.as_bool() || ret.0 == -1 {
                break;
            }

            // Handle Global Hotkeys
            if msg.message == WM_HOTKEY {
                if msg.wParam.0 == HOTKEY_ID_DASHBOARD as usize {
                    dashboard.toggle_visibility();
                } else if msg.wParam.0 == HOTKEY_ID_OVERLAY as usize {
                    hud.toggle_visibility();
                    tray_manager.set_hud_checked(hud.is_visible());
                }
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);

            // Check tray & menu events
            let keep_running = tray_manager.handle_events(
                &dashboard,
                &hud,
                state.config.clone(),
                state.metrics.clone(),
                state.is_running.clone(),
                state.benchmark_session.clone(),
            );

            if !keep_running {
                state.is_running.store(false, std::sync::atomic::Ordering::Relaxed);
                PostQuitMessage(0);
                break;
            }
        }
    }

    // 8. Cleanup Hotkeys & COM
    unsafe {
        let _ = UnregisterHotKey(None, HOTKEY_ID_DASHBOARD);
        let _ = UnregisterHotKey(None, HOTKEY_ID_OVERLAY);
    }

    ui_hwnds.write().clear();
    state.is_running.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = monitor_handle.join();

    unsafe {
        CoUninitialize();
    }
}
