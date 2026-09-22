pub mod hud;
pub use hud::FloatingHud;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, LOGFONTW,
    SelectObject, SetBkMode, SetTextColor, DT_LEFT, DT_NOCLIP, DT_RIGHT, DT_SINGLELINE,
    HDC, HFONT, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, IsWindowVisible,
    PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, SystemParametersInfoW, BringWindowToTop, GWLP_USERDATA, HCURSOR, HICON, HWND_TOPMOST,
    SPI_GETWORKAREA, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_PAINT, WM_SETICON, ICON_BIG, ICON_SMALL,
    WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_EX_TOPMOST,
    CreateIconFromResourceEx, IMAGE_FLAGS,
};

use crate::app::SystemMetrics;
use crate::monitor::network::format_speed;
use crate::monitor::WM_METRICS_UPDATED;

// Compact, minimal window dimensions (280 x 390)
const WINDOW_WIDTH: i32 = 280;
const WINDOW_HEIGHT: i32 = 470;
const WINDOW_CLASS_NAME: PCWSTR = w!("VeroStatDashboardClass");
const WINDOW_TITLE: PCWSTR = w!("VeroStat");

// Clean Minimal Theme Colors
const COLOR_BG: COLORREF = COLORREF(0x00171515); // Neutral dark #151517 (0x00BBGGRR)
const COLOR_TRACK: COLORREF = COLORREF(0x002B2727); // Dark track #27272B
const COLOR_BORDER: COLORREF = COLORREF(0x002E2929); // Subtle divider #29292E
const COLOR_TEXT_PRIMARY: COLORREF = COLORREF(0x00F8FAFC); // Crisp white #FCFAF8
const COLOR_TEXT_LABEL: COLORREF = COLORREF(0x0094A3B8); // Slate label #94A3B8
const COLOR_TEXT_DIM: COLORREF = COLORREF(0x0064748B); // Dim gray #64748B
const COLOR_BAR_FILL: COLORREF = COLORREF(0x00F8BD38); // Calm sky blue #38BDF8
const COLOR_BAR_WARN: COLORREF = COLORREF(0x004444EF); // Crimson red on extreme temp #EF4444

struct DashboardContext {
    metrics: Arc<RwLock<SystemMetrics>>,
    is_running: Arc<AtomicBool>,
    font_title: HFONT,
    font_label: HFONT,
    font_value: HFONT,
    font_sub: HFONT,
    font_small: HFONT,
}

impl Drop for DashboardContext {
    fn drop(&mut self) {
        unsafe {
            if !self.font_title.is_invalid() {
                let _ = DeleteObject(self.font_title);
            }
            if !self.font_label.is_invalid() {
                let _ = DeleteObject(self.font_label);
            }
            if !self.font_value.is_invalid() {
                let _ = DeleteObject(self.font_value);
            }
            if !self.font_sub.is_invalid() {
                let _ = DeleteObject(self.font_sub);
            }
            if !self.font_small.is_invalid() {
                let _ = DeleteObject(self.font_small);
            }
        }
    }
}

pub struct DashboardWindow {
    pub hwnd: HWND,
}

impl DashboardWindow {
    pub fn new(metrics: Arc<RwLock<SystemMetrics>>, is_running: Arc<AtomicBool>) -> Result<Self, String> {
        unsafe {
            let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map_err(|e| format!("Failed to get module handle: {:?}", e))?;

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(window_proc);
            wc.hInstance = hinstance.into();
            wc.lpszClassName = WINDOW_CLASS_NAME;
            wc.hCursor = HCURSOR::default();

            let app_icon = get_embedded_app_icon();
            if let Some(icon) = app_icon {
                wc.hIcon = icon;
                wc.hIconSm = icon;
            }

            let _ = RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                WINDOW_CLASS_NAME,
                WINDOW_TITLE,
                WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                100,
                100,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                None,
                None,
                hinstance,
                None,
            )
            .map_err(|e| format!("Failed to create window: {:?}", e))?;

            if let Some(icon) = app_icon {
                let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.0 as isize));
                let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon.0 as isize));
            }
            // Enable Windows 10/11 Dark Titlebar
            let dark_mode: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const _ as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );

            // Clean Segoe UI fonts
            let font_title = create_font(15, 600);
            let font_label = create_font(11, 600);
            let font_value = create_font(18, 700);
            let font_sub = create_font(12, 400);
            let font_small = create_font(11, 400);

            let context = Box::new(DashboardContext {
                metrics,
                is_running,
                font_title,
                font_label,
                font_value,
                font_sub,
                font_small,
            });

            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(context) as isize);

            position_near_tray(hwnd);

            Ok(Self { hwnd })
        }
    }

    pub fn toggle_visibility(&self) {
        unsafe {
            if IsWindowVisible(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            } else {
                self.show();
            }
        }
    }

    pub fn show(&self) {
        unsafe {
            position_near_tray(self.hwnd);
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    #[allow(dead_code)]
    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

fn create_font(height: i32, weight: i32) -> HFONT {
    let mut lf = LOGFONTW::default();
    lf.lfHeight = -height;
    lf.lfWeight = weight;
    let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    let copy_len = font_name.len().min(lf.lfFaceName.len());
    lf.lfFaceName[..copy_len].copy_from_slice(&font_name[..copy_len]);
    unsafe { CreateFontIndirectW(&lf) }
}

const EMBEDDED_ICON: &[u8] = include_bytes!("../../assets/icon.ico");

fn get_embedded_app_icon() -> Option<HICON> {
    unsafe {
        if EMBEDDED_ICON.len() > 22 {
            let res = CreateIconFromResourceEx(
                &EMBEDDED_ICON[22..],
                windows::Win32::Foundation::BOOL(1),
                0x00030000,
                32,
                32,
                IMAGE_FLAGS(0),
            );
            if let Ok(hicon) = res {
                return Some(hicon);
            }
        }
        None
    }
}

fn position_near_tray(hwnd: HWND) {
    unsafe {
        let mut work_area = RECT::default();
        if SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let x = work_area.right - WINDOW_WIDTH - 12;
            let y = work_area.bottom - WINDOW_HEIGHT - 12;
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                SWP_SHOWWINDOW,
            );
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    let ctx = if ptr != 0 {
        &mut *(ptr as *mut DashboardContext)
    } else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };

    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let mut client_rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut client_rect);
            let width = client_rect.right - client_rect.left;
            let height = client_rect.bottom - client_rect.top;

            let mem_dc = CreateCompatibleDC(hdc);
            let mem_bmp = CreateCompatibleBitmap(hdc, width, height);
            let old_bmp = SelectObject(mem_dc, mem_bmp);

            render_dashboard(mem_dc, width, height, ctx);

            let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);

            SelectObject(mem_dc, old_bmp);
            let _ = DeleteObject(mem_bmp);
            let _ = DeleteDC(mem_dc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_METRICS_UPDATED => {
            if IsWindowVisible(hwnd).as_bool() {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            ctx.is_running.store(false, Ordering::Relaxed);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn render_dashboard(
    hdc: HDC,
    width: i32,
    height: i32,
    ctx: &DashboardContext,
) {
    // 1. Background
    let bg_brush = CreateSolidBrush(COLOR_BG);
    let full_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    FillRect(hdc, &full_rect, bg_brush);
    let _ = DeleteObject(bg_brush);

    SetBkMode(hdc, TRANSPARENT);

    let metrics = ctx.metrics.read().clone();

    let pad_x = 16;
    let content_w = width - (pad_x * 2);
    let mut y = 12;

    // Header: "VeroStat"
    SelectObject(hdc, ctx.font_title);
    SetTextColor(hdc, COLOR_TEXT_PRIMARY);
    draw_text_line(hdc, pad_x, y, "VeroStat", DT_LEFT);

    y += 22;
    draw_line(hdc, pad_x, y, width - pad_x, COLOR_BORDER);
    y += 10;

    // ------------------------------------------------------------------------
    // CPU Section
    // ------------------------------------------------------------------------
    let cpu_usage = metrics.cpu_usage.unwrap_or(0.0);
    let cpu_temp_str = metrics
        .cpu_temperature
        .map(|t| format!("{:.0}°C", t))
        .unwrap_or_else(|| "N/A".to_string());

    let cpu_freq_str = metrics
        .cpu_frequency
        .map(|f| format!("{:.1}GHz", f))
        .unwrap_or_default();

    let cpu_right_str = if !cpu_freq_str.is_empty() {
        format!("{} • {}", cpu_temp_str, cpu_freq_str)
    } else {
        cpu_temp_str
    };

    let cpu_short_name = shorten_name(&metrics.cpu_name, 18);

    y = draw_stat_row(
        hdc,
        pad_x,
        y,
        content_w,
        "CPU",
        &cpu_short_name,
        &format!("{:.0}%", cpu_usage),
        &cpu_right_str,
        cpu_usage / 100.0,
        if metrics.cpu_temperature.unwrap_or(0.0) >= 85.0 { COLOR_BAR_WARN } else { COLOR_BAR_FILL },
        ctx,
    );

    // ------------------------------------------------------------------------
    // GPU Section
    // ------------------------------------------------------------------------
    let gpu_usage_str = metrics
        .gpu_usage
        .map(|u| format!("{:.0}%", u))
        .unwrap_or_else(|| "N/A".to_string());
    let gpu_temp_str = metrics
        .gpu_temperature
        .map(|t| format!("{:.0}°C", t))
        .unwrap_or_else(|| "N/A".to_string());

    let gpu_vram_str = match (metrics.gpu_memory_used, metrics.gpu_memory_total) {
        (Some(used), Some(total)) => format!(
            "{:.1}/{:.0}G",
            used as f64 / 1024.0 / 1024.0 / 1024.0,
            total as f64 / 1024.0 / 1024.0 / 1024.0
        ),
        _ => String::new(),
    };

    let gpu_right_str = if !gpu_vram_str.is_empty() && gpu_temp_str != "N/A" {
        format!("{} • {}", gpu_temp_str, gpu_vram_str)
    } else {
        gpu_temp_str
    };

    let gpu_short_name = shorten_name(metrics.gpu_name.as_deref().unwrap_or("GPU"), 18);
    let gpu_fill = metrics.gpu_usage.unwrap_or(0.0) / 100.0;

    y = draw_stat_row(
        hdc,
        pad_x,
        y,
        content_w,
        "GPU",
        &gpu_short_name,
        &gpu_usage_str,
        &gpu_right_str,
        gpu_fill,
        if metrics.gpu_temperature.unwrap_or(0.0) >= 85.0 { COLOR_BAR_WARN } else { COLOR_BAR_FILL },
        ctx,
    );

    // ------------------------------------------------------------------------
    // RAM Section
    // ------------------------------------------------------------------------
    let ram_used_gb = metrics.ram_used as f64 / 1024.0 / 1024.0 / 1024.0;
    let ram_total_gb = metrics.ram_total as f64 / 1024.0 / 1024.0 / 1024.0;
    let ram_str = format!("{:.1} / {:.0} GB", ram_used_gb, ram_total_gb);
    let ram_pct_str = format!("{:.0}%", metrics.ram_usage);

    y = draw_stat_row(
        hdc,
        pad_x,
        y,
        content_w,
        "RAM",
        "Memory",
        &ram_str,
        &ram_pct_str,
        metrics.ram_usage / 100.0,
        COLOR_BAR_FILL,
        ctx,
    );

    // ------------------------------------------------------------------------
    // STORAGE Section
    // ------------------------------------------------------------------------
    let disk_used_gb = metrics.disk_used as f64 / 1024.0 / 1024.0 / 1024.0;
    let disk_total_gb = metrics.disk_total as f64 / 1024.0 / 1024.0 / 1024.0;
    let disk_str = format!("{:.0} / {:.0} GB", disk_used_gb, disk_total_gb);
    let disk_pct_str = format!("{:.0}%", metrics.disk_usage);

    y = draw_stat_row(
        hdc,
        pad_x,
        y,
        content_w,
        "STORAGE",
        "Disks",
        &disk_str,
        &disk_pct_str,
        metrics.disk_usage / 100.0,
        COLOR_BAR_FILL,
        ctx,
    );

    // ------------------------------------------------------------------------
    // NETWORK Section
    // ------------------------------------------------------------------------
    SelectObject(hdc, ctx.font_label);
    SetTextColor(hdc, COLOR_TEXT_LABEL);
    draw_text_line(hdc, pad_x, y, "NETWORK", DT_LEFT);
    y += 16;

    SelectObject(hdc, ctx.font_sub);
    SetTextColor(hdc, COLOR_TEXT_PRIMARY);
    let net_text = format!(
        "↓ {}     ↑ {}",
        format_speed(metrics.download_speed),
        format_speed(metrics.upload_speed)
    );
    draw_text_line(hdc, pad_x, y, &net_text, DT_LEFT);
    y += 24;

    // ------------------------------------------------------------------------
    // TOP PROCESSES Section
    // ------------------------------------------------------------------------
    draw_line(hdc, pad_x, y, width - pad_x, COLOR_BORDER);
    y += 10;

    SelectObject(hdc, ctx.font_label);
    SetTextColor(hdc, COLOR_TEXT_LABEL);
    draw_text_line(hdc, pad_x, y, "TOP PROCESSES", DT_LEFT);
    y += 18;

    if metrics.top_processes.is_empty() {
        SelectObject(hdc, ctx.font_small);
        SetTextColor(hdc, COLOR_TEXT_DIM);
        draw_text_line(hdc, pad_x, y, "Scanning processes...", DT_LEFT);
    } else {
        for (i, proc) in metrics.top_processes.iter().enumerate() {
            let mem_mb = proc.memory_bytes / 1024 / 1024;
            let right_str = if mem_mb >= 1024 {
                format!("{:.0}% • {:.1} GB", proc.cpu_usage, mem_mb as f64 / 1024.0)
            } else {
                format!("{:.0}% • {} MB", proc.cpu_usage, mem_mb)
            };
            let proc_display = format!("{}. {}", i + 1, shorten_name(&proc.name, 14));

            SelectObject(hdc, ctx.font_small);
            SetTextColor(hdc, COLOR_TEXT_PRIMARY);
            draw_text_line(hdc, pad_x, y, &proc_display, DT_LEFT);

            SetTextColor(hdc, COLOR_TEXT_DIM);
            draw_text_line(hdc, pad_x + content_w, y, &right_str, DT_RIGHT);
            y += 17;
        }
    }

    // Footer
    let footer_y = height - 18;
    SelectObject(hdc, ctx.font_small);
    SetTextColor(hdc, COLOR_TEXT_DIM);
    draw_text_line(hdc, pad_x, footer_y, "Esc to close", DT_LEFT);
}

unsafe fn draw_stat_row(
    hdc: HDC,
    x: i32,
    y: i32,
    w: i32,
    label: &str,
    sub_label: &str,
    val_left: &str,
    val_right: &str,
    fill_ratio: f32,
    fill_color: COLORREF,
    ctx: &DashboardContext,
) -> i32 {
    // Top line: Label ("CPU") on left, sub_label ("i5-11400H") on right
    SelectObject(hdc, ctx.font_label);
    SetTextColor(hdc, COLOR_TEXT_LABEL);
    draw_text_line(hdc, x, y, label, DT_LEFT);

    SetTextColor(hdc, COLOR_TEXT_DIM);
    draw_text_line(hdc, x + w, y, sub_label, DT_RIGHT);
    let mut cur_y = y + 15;

    // Middle line: Main metric ("23%") on left, sub metric ("52°C • 2.7GHz") on right
    SelectObject(hdc, ctx.font_value);
    SetTextColor(hdc, COLOR_TEXT_PRIMARY);
    draw_text_line(hdc, x, cur_y, val_left, DT_LEFT);

    SelectObject(hdc, ctx.font_sub);
    SetTextColor(hdc, COLOR_TEXT_LABEL);
    draw_text_line(hdc, x + w, cur_y + 3, val_right, DT_RIGHT);
    cur_y += 22;

    // Bottom line: Clean thin bar (3px)
    draw_progress_bar(hdc, x, cur_y, w, 3, fill_ratio, fill_color);
    cur_y += 14;

    cur_y
}

unsafe fn draw_progress_bar(
    hdc: HDC,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    ratio: f32,
    accent: COLORREF,
) {
    let clamped_ratio = ratio.clamp(0.0, 1.0);

    let track_rect = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let track_brush = CreateSolidBrush(COLOR_TRACK);
    FillRect(hdc, &track_rect, track_brush);
    let _ = DeleteObject(track_brush);

    let fill_w = (w as f32 * clamped_ratio).round() as i32;
    if fill_w > 0 {
        let fill_rect = RECT {
            left: x,
            top: y,
            right: x + fill_w,
            bottom: y + h,
        };
        let fill_brush = CreateSolidBrush(accent);
        FillRect(hdc, &fill_rect, fill_brush);
        let _ = DeleteObject(fill_brush);
    }
}

unsafe fn draw_text_line(
    hdc: HDC,
    x: i32,
    y: i32,
    text: &str,
    align_flag: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT {
        left: if align_flag == DT_RIGHT { x - 180 } else { x },
        top: y,
        right: if align_flag == DT_RIGHT { x } else { x + 240 },
        bottom: y + 25,
    };
    let _ = DrawTextW(
        hdc,
        &mut wide,
        &mut rect,
        DT_SINGLELINE | DT_NOCLIP | align_flag,
    );
}

unsafe fn draw_line(
    hdc: HDC,
    x1: i32,
    y1: i32,
    x2: i32,
    color: COLORREF,
) {
    let line_rect = RECT {
        left: x1,
        top: y1,
        right: x2,
        bottom: y1 + 1,
    };
    let brush = CreateSolidBrush(color);
    FillRect(hdc, &line_rect, brush);
    let _ = DeleteObject(brush);
}

fn shorten_name(name: &str, max_len: usize) -> String {
    let trimmed = name
        .replace("11th Gen ", "")
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("Intel Core ", "")
        .replace("NVIDIA GeForce ", "")
        .trim()
        .to_string();

    if trimmed.chars().count() > max_len {
        let short: String = trimmed.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", short)
    } else {
        trimmed
    }
}
