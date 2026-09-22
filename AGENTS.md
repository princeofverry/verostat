# AGENTS.md — Developer & Agent Guide for VeroStat

VeroStat is an ultra-lightweight, native Windows system-tray utility written in **Rust**.
Primary design priority: **Low resource usage > reliability > simplicity > clean aesthetics**.

---

## 1. Core Engineering Invariants

- **No Heavy Runtimes**: Never introduce Electron, Node.js, Python, Tauri, or WGPU/OpenGL frameworks. The release binary must remain standalone and ultra-compact (< 1 MB).
- **Pure Win32 GDI**: The Dashboard and HUD are native Win32 windows rendered using GDI double-buffering.
- **Zero Subprocess Spawning**: Never spawn external commands (`powershell`, `ipconfig`, `netstat`, `wmic`) inside the periodic monitoring loop. All metrics must be gathered in-process via native C/Win32 APIs or library calls.
- **Graceful Hardware Fallback**: Hardware availability varies across machines. Missing sensors (temperatures, unsupported GPUs) must gracefully report `N/A` or fallback without panicking or crashing.

---

## 2. Architecture & Module Map

```text
src/
├── main.rs              # Win32 message loop, COM init, global hotkeys, thread coordination
├── app/
│   ├── mod.rs           # Re-exports configuration and state types
│   ├── config.rs        # TOML config (%APPDATA%\VeroStat\config.toml) & HKCU Run registry autostart
│   └── state.rs         # SystemMetrics snapshot, BenchmarkSession, and AppState
├── monitor/
│   ├── mod.rs           # Background thread coordinator, process hogs, disk polling
│   ├── cpu.rs           # CPU usage percentage, brand model, frequency
│   ├── gpu.rs           # Dynamic NVML loader (nvml.dll) + DXGI (dxgi.dll) fallback
│   ├── memory.rs        # RAM usage and capacity
│   ├── network.rs       # Real-time network speed deltas (without subprocesses)
│   └── sensors.rs       # CPU temperature via native Windows PDH (pdh.dll) thermal counters
├── tray/
│   ├── mod.rs           # System tray icon, menu, tooltips, click handling & debouncing
│   └── icon_gen.rs      # Dynamic 32x32 pixel font icon generator (0-99 numbers & logo)
└── ui/
    ├── mod.rs           # Compact dark dashboard window (280x470, Segoe UI, double-buffered GDI)
    └── hud.rs           # Mini In-Game Floating HUD overlay (360x34, 90% alpha, draggable)
```

---

## 3. Critical Gotchas & Patterns

### Window Management & Z-Order
- **Always on Top**: Both the Dashboard and the Floating HUD must use `WS_EX_TOPMOST`.
- **SetWindowPos Flag**: When repositioning with `HWND_TOPMOST`, **never** pass `SWP_NOZORDER`. Windows ignores `HWND_TOPMOST` if `SWP_NOZORDER` is supplied. Use `SWP_SHOWWINDOW`.
- **Bring to Front**: Call `BringWindowToTop(hwnd)` and `SetForegroundWindow(hwnd)` when opening windows so they do not get trapped behind the Windows taskbar.
- **Embedded Icon**: Window icons are embedded directly via `include_bytes!("../../assets/icon.ico")` and created with `CreateIconFromResourceEx` so `.exe` portability never breaks if separated from the `assets/` directory.

### Tray Icon & Event Debouncing
- **Event Queue Draining**: When handling `TrayIconEvent`, always drain the receiver using `while let Ok(event) = TrayIconEvent::receiver().try_recv()` rather than `if let Ok(...)`.
- **Click Debouncing**: Enforce a minimum 250ms debounce threshold on tray icon toggles (`last_toggle_time`). Without this, Windows sends rapid multi-click events that toggle the window open and immediately closed.
- **Menu Clicks**: When clicking "Open Dashboard" from the context menu, always drain trailing tray icon events before calling `dashboard.show()`.

### Hardware Sensors
- **NVIDIA GPU**: Loaded dynamically via `LoadLibraryW("nvml.dll")` without static SDK linking.
- **AMD & Intel GPUs**: When NVML is absent, `GenericGpuProvider` queries DXGI via `CreateDXGIFactory1()` to enumerate adapters, skip software renderers (`DXGI_ADAPTER_FLAG_SOFTWARE`), and pick the discrete adapter with the highest dedicated video memory.
- **CPU Temperature**: Polled in-process via `pdh.dll` counter `\Thermal Zone Information(*)\Temperature` with fallback to `sysinfo::Components`.

---

## 4. Development & Build Commands

```bash
# Check code & warnings
cargo check

# Run in debug mode (shows console output)
cargo run

# Build optimized release binary
cargo build --release

# Kill any existing background instance before release build (avoids OS Error 5 Access Denied)
powershell -Command "Get-Process -Name verostat -ErrorAction SilentlyContinue | Stop-Process -Force"
```

### CI/CD Workflows
- `.github/workflows/ci.yml`: Runs on push and pull requests to `main` (`cargo check`, `cargo build --release`).
- `.github/workflows/release.yml`: Runs on git tags matching `v*`. Builds release binary, creates `verostat-windows-x64.zip`, computes SHA256 checksums, and publishes a GitHub Release.

---

## 5. Hotkeys Reference

- `Win + Shift + V`: Toggle Dashboard window.
- `Ctrl + Shift + O`: Toggle In-Game Floating HUD overlay.
