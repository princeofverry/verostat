# VeroStat 📊

**VeroStat** is an ultra-lightweight, native Windows system-tray utility written in **Rust**. It provides real-time hardware metrics and network statistics from a compact, modern dark dashboard and your Windows taskbar.

Prioritizes: **Low resource usage > reliability > simplicity > clean aesthetics**.

---

## Features

- 🖥️ **CPU Monitoring**:
  - Global CPU usage percentage
  - Model name detection
  - Dynamic frequency (GHz)
  - Temperature sensor support (with graceful `N/A` fallback)
- 🎮 **GPU Monitoring**:
  - Direct zero-cost dynamic NVIDIA NVML integration (`nvml.dll`)
  - Real-time GPU core usage percentage
  - GPU temperature (°C)
  - VRAM usage and total capacity (GB)
  - Full GPU model name
  - Extensible provider abstraction with generic/DXGI fallback
- 🧠 **RAM Monitoring**:
  - Memory usage percentage
  - Used and total system memory in GB
- 🌐 **Real-Time Network**:
  - Real-time download speed (`↓ MB/s`)
  - Real-time upload speed (`↑ KB/s`)
  - Computed purely from network byte deltas without external CLI spawns
- 💾 **Storage Status**:
  - Total drive capacity and used space across local disks
  - Visual capacity bar
- 🪟 **Native System Tray**:
  - Lives in Windows notification area (system tray)
  - Left-click tray icon to toggle dashboard
  - Right-click menu:
    - **Open Dashboard**
    - **Refresh**
    - **▶ Start Benchmark Log** / **⏹ Stop Benchmark Log (REC)**
    - **Tray Icon Display ▸** (Select what appears on the taskbar icon: CPU %, CPU Temp, GPU %, GPU Temp, RAM %, or Default Logo)
    - **Settings...**
    - **Start with Windows** (toggle registry autostart)
    - **About**
    - **Exit**
  - **Customizable Dynamic Taskbar Icon**: Choose between real-time CPU %, CPU Temp (°C), GPU %, GPU Temp (°C), RAM %, or Default Logo directly from the tray menu with persistent configuration.
  - Dynamic tray icon tooltip showing live CPU temp, GPU temp, RAM %, and network speed on hover.
  - **Benchmark Session Logger**: Click *Start Benchmark Log* in the tray menu to record hardware metrics every second to a CSV file; stopping displays a full summary with peak temperatures and averages.
  - Displays the 3 most CPU-heavy active processes directly on the dashboard with their CPU % and RAM usage.
- 🎮 **In-Game Floating HUD Overlay**:
  - Mini translucent horizontal bar (`CPU 34% 52°C | GPU 72% 61°C | RAM 11.2G | ↓2.4M`)
  - Always on top, draggable anywhere on screen, double-click or right-click to hide
  - Toggle via menu or Global Hotkey `Ctrl + Shift + O`
- ⌨️ **Global Hotkeys**:
  - `Win + Shift + V`: Instantly toggle Dashboard
  - `Ctrl + Shift + O`: Instantly toggle In-Game Floating HUD
- 🎨 **Minimal Dark Dashboard**:
  - Ultra-compact, clean layout (280 × 470 px) with native Win32 GDI double-buffered rendering
  - Windows 10/11 immersive dark mode caption bar
  - Auto-docked above system tray notification area
  - Press `Esc` or close button to minimize back to tray
---

## Architecture

```text
                    VeroStat
                       │
              ┌────────┴────────┐
              │                 │
        Monitoring Layer     Tray/UI
              │                 │
      ┌───────┼────────┐        │
      │       │        │        │
     CPU     GPU      RAM       │
      │       │        │        │
      └───────┼────────┘        │
              │                 │
           Network              │
              │                 │
              ▼                 ▼
        Metrics Snapshot ──► Dashboard
```

- **Thread Isolation**: Metrics are polled periodically (default: 1 second) on a background thread and cached into a thread-safe `Arc<RwLock<SystemMetrics>>`.
- **Zero-Flicker GDI**: Rendering uses memory device context double buffering only when the dashboard is visible.
- **Dynamic NVML Loading**: Loads `nvml.dll` dynamically at runtime without linking against NVIDIA SDK headers. Gracefully falls back if NVML is not available.
- **Instant Wake/Sleep**: Responsive shutdown and configurable sampling intervals.

---

## Project Structure

```text
verostat/
├── Cargo.toml
├── README.md
├── assets/
│   └── icon.ico
└── src/
    ├── main.rs
    ├── app/
    │   ├── mod.rs
    │   ├── config.rs
    │   └── state.rs
    ├── monitor/
    │   ├── mod.rs
    │   ├── cpu.rs
    │   ├── gpu.rs
    │   ├── memory.rs
    │   ├── network.rs
    │   └── sensors.rs
    ├── tray/
    │   ├── mod.rs
    │   └── icon_gen.rs
    │
    └── ui/
        └── mod.rs
```

---

## Requirements & Building

### Prerequisites

- Windows 10 / Windows 11 x64
- Rust stable (or nightly) toolchain (`rustup`)

### Build Release Binary

```bash
cargo build --release
```

The compiled binary will be placed at:
```text
target/release/verostat.exe
```

The release executable is optimized for small binary size (~680 KB) and minimal memory consumption (< 45 MB working set).

---

## Configuration

Settings are saved in TOML format at:
```text
%APPDATA%\VeroStat\config.toml
```

Example configuration:
```toml
refresh_interval_ms = 1000
start_with_windows = false
high_temp_threshold = 85.0
show_disk_stats = true
```

---

## Controls

| Action | Shortcut / Trigger |
|---|---|
| Toggle Dashboard | Left-click tray icon |
| Open Menu | Right-click tray icon |
| Hide Dashboard | Press `Esc` or click `X` |
| Autostart with Windows | Toggle in tray menu |
| Exit VeroStat | Select `Exit` from tray menu |

---

## License

MIT License.
