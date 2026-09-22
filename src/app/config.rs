use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_REG_NAME: &str = "VeroStat";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Refresh interval in milliseconds (default 1000)
    pub refresh_interval_ms: u64,
    /// Automatically start VeroStat when logging into Windows
    pub start_with_windows: bool,
    /// Temperature warning threshold in Celsius (default 85.0)
    pub high_temp_threshold: f32,
    /// Show disk statistics in dashboard
    pub show_disk_stats: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: 1000,
            start_with_windows: false,
            high_temp_threshold: 85.0,
            show_disk_stats: true,
        }
    }
}

impl AppConfig {
    /// Returns the path to the configuration file: `%APPDATA%\VeroStat\config.toml`
    pub fn config_path() -> Option<PathBuf> {
        let appdata = std::env::var_os("APPDATA")?;
        let mut path = PathBuf::from(appdata);
        path.push("VeroStat");
        path.push("config.toml");
        Some(path)
    }

    /// Load configuration from disk, or return default if missing or invalid.
    pub fn load() -> Self {
        let mut cfg = Self::default();

        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(parsed) = toml::from_str::<AppConfig>(&content) {
                        cfg = parsed;
                    }
                }
            }
        }

        // Synchronize with current registry state
        cfg.start_with_windows = is_autostart_enabled();
        cfg
    }

    /// Save configuration to disk.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or_else(|| "Could not locate APPDATA directory".to_string())?;
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        Ok(())
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Checks if VeroStat is configured in HKCU Run registry key.
pub fn is_autostart_enabled() -> bool {
    let subkey = to_wide(RUN_KEY_PATH);
    let mut hkey = HKEY::default();

    unsafe {
        let err = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );
        if !err.is_ok() {
            return false;
        }

        let val_name = to_wide(APP_REG_NAME);
        let mut data_len = 0u32;

        let status = RegQueryValueExW(
            hkey,
            PCWSTR(val_name.as_ptr()),
            None,
            None,
            None,
            Some(&mut data_len),
        );

        let _ = RegCloseKey(hkey);
        status.is_ok() && data_len > 0
    }
}

/// Enables or disables autostart with Windows via the HKCU Run registry key.
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let subkey = to_wide(RUN_KEY_PATH);
    let mut hkey = HKEY::default();

    unsafe {
        let open_err = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_WRITE,
            &mut hkey,
        );
        if !open_err.is_ok() {
            return Err(format!("Failed to open registry key: {:?}", open_err));
        }

        let val_name = to_wide(APP_REG_NAME);

        let result = if enable {
            let current_exe = std::env::current_exe()
                .map_err(|e| format!("Failed to determine executable path: {}", e))?;
            let exe_str = format!("\"{}\"", current_exe.to_string_lossy());
            let wide_exe = to_wide(&exe_str);

            let bytes_len = (wide_exe.len() * 2) as u32;
            let set_err = RegSetValueExW(
                hkey,
                PCWSTR(val_name.as_ptr()),
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    wide_exe.as_ptr() as *const u8,
                    bytes_len as usize,
                )),
            );
            if set_err.is_ok() {
                Ok(())
            } else {
                Err(format!("Failed to set registry value: {:?}", set_err))
            }
        } else {
            let _ = RegDeleteValueW(hkey, PCWSTR(val_name.as_ptr()));
            Ok(())
        };

        let _ = RegCloseKey(hkey);
        result
    }
}
