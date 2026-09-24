use std::ffi::c_void;
use std::sync::Arc;
use parking_lot::RwLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush,
    DeleteDC, DeleteObject, EndPaint, FillRect, FrameRect, SelectObject, SetBkMode,
    SetTextColor, DT_CENTER, DT_LEFT, DT_RIGHT, DT_SINGLELINE, HDC, HFONT, PAINTSTRUCT,
    SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, GetClientRect, IsWindowVisible,
    RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, GWLP_USERDATA, HCURSOR, HWND_TOPMOST, ICON_BIG, ICON_SMALL, SWP_SHOWWINDOW,
    SW_HIDE, SW_SHOW, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_PAINT, WM_SETICON,
    WNDCLASSEXW, WS_CAPTION, WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
};

use crate::app::SystemMetrics;
use crate::monitor::WM_METRICS_UPDATED;
use super::{
    create_font, draw_line, draw_progress_bar, draw_text_line, get_embedded_app_icon,
    shorten_name, HardwareBrand, COLOR_BG, COLOR_BORDER, COLOR_TEXT_DIM,
    COLOR_TEXT_LABEL, COLOR_TEXT_PRIMARY, COLOR_TRACK,
};

const INSPECTOR_WIDTH: i32 = 460;
const INSPECTOR_CLASS_NAME: PCWSTR = w!("VeroStatCoreInspectorClass");
const INSPECTOR_TITLE: PCWSTR = w!("VeroStat - CPU Core Inspector");
struct InspectorContext {
    metrics: Arc<RwLock<SystemMetrics>>,
    font_title: HFONT,
    font_label: HFONT,
    font_value: HFONT,
    font_core: HFONT,
    font_small: HFONT,
}

impl Drop for InspectorContext {
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
            if !self.font_core.is_invalid() {
                let _ = DeleteObject(self.font_core);
            }
            if !self.font_small.is_invalid() {
                let _ = DeleteObject(self.font_small);
            }
        }
    }
}

pub struct CoreInspectorWindow {
    pub hwnd: HWND,
    pub width: i32,
    pub height: i32,
}

impl CoreInspectorWindow {
    pub fn new(metrics: Arc<RwLock<SystemMetrics>>) -> Result<Self, String> {
        unsafe {
            let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map_err(|e| format!("Failed to get module handle: {:?}", e))?;

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(inspector_window_proc);
            wc.hInstance = hinstance.into();
            wc.lpszClassName = INSPECTOR_CLASS_NAME;
            wc.hCursor = HCURSOR::default();

            let app_icon = get_embedded_app_icon();
            if let Some(icon) = app_icon {
                wc.hIcon = icon;
                wc.hIconSm = icon;
            }

            let _ = RegisterClassExW(&wc);
            let detected_cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
            let cores_count = metrics.read().cpu_logical_cores.max(detected_cores);
            let rows = ((cores_count + 1) / 2) as i32;
            let window_height = (180 + rows * 42 + 75).clamp(400, 780);

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                INSPECTOR_CLASS_NAME,
                INSPECTOR_TITLE,
                WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                120,
                120,
                INSPECTOR_WIDTH,
                window_height,
                None,
                None,
                hinstance,
                None,
            )
            .map_err(|e| format!("Failed to create inspector window: {:?}", e))?;

            if let Some(icon) = app_icon {
                let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.0 as isize));
                let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(icon.0 as isize));
            }

            // Dark Mode Titlebar
            let dark_mode: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const _ as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );

            let font_title = create_font(15, 700);
            let font_label = create_font(12, 600);
            let font_value = create_font(16, 700);
            let font_core = create_font(11, 600);
            let font_small = create_font(10, 400);

            let context = Box::new(InspectorContext {
                metrics,
                font_title,
                font_label,
                font_value,
                font_core,
                font_small,
            });

            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(context) as isize);

            position_inspector_window(hwnd, INSPECTOR_WIDTH, window_height);

            Ok(Self { hwnd, width: INSPECTOR_WIDTH, height: window_height })
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
            let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(self.hwnd, GWLP_USERDATA);
            let target_h = if ptr != 0 {
                let ctx = &*(ptr as *const InspectorContext);
                let detected = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
                let count = ctx.metrics.read().cpu_logical_cores.max(detected);
                let rows = ((count + 1) / 2) as i32;
                (180 + rows * 42 + 75).clamp(400, 780)
            } else {
                self.height
            };
            position_inspector_window(self.hwnd, self.width, target_h);
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }
    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }
}

unsafe extern "system" fn inspector_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    let ctx = if ptr != 0 {
        &mut *(ptr as *mut InspectorContext)
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

            render_inspector(mem_dc, width, height, ctx);

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
                let detected = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
                let count = ctx.metrics.read().cpu_logical_cores.max(detected);
                let rows = ((count + 1) / 2) as i32;
                let target_h = (180 + rows * 42 + 75).clamp(400, 780);

                let mut rc = RECT::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rc);
                let current_h = rc.bottom - rc.top;
                if (current_h - target_h).abs() > 10 {
                    position_inspector_window(hwnd, INSPECTOR_WIDTH, target_h);
                }

                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = Box::from_raw(ctx as *mut InspectorContext);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn render_inspector(
    hdc: HDC,
    width: i32,
    height: i32,
    ctx: &InspectorContext,
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

    let m = ctx.metrics.read().clone();
    let pad_x = 16;
    let content_w = width - (pad_x * 2);
    let mut y = 14;

    // Header: "CPU Core Inspector"
    SelectObject(hdc, ctx.font_title);
    SetTextColor(hdc, COLOR_TEXT_PRIMARY);
    draw_text_line(hdc, pad_x, y, "CPU Core Inspector", DT_LEFT);

    // CPU Brand & Accent Color
    let cpu_brand = HardwareBrand::detect_cpu(&m.cpu_name);
    let brand_color = cpu_brand.color().unwrap_or(super::COLOR_BAR_FILL);

    // Row 2: CPU Model (own dedicated line)
    y += 24;
    SelectObject(hdc, ctx.font_label);
    SetTextColor(hdc, brand_color);
    let short_name = shorten_name(&m.cpu_name, 38);
    draw_text_line(hdc, pad_x, y, &short_name, DT_LEFT);

    // Row 3: Physical Cores / Logical Threads Spec (own dedicated line)
    y += 18;
    let phys = m.cpu_physical_cores.unwrap_or(0);
    let log = m.cpu_logical_cores.max(m.cpu_cores.len());
    let freq_base = m.cpu_frequency.map(|f| format!(" • Base {:.1} GHz", f)).unwrap_or_default();
    let cores_spec_str = if phys > 0 {
        format!("{} Physical Cores • {} Logical Threads{}", phys, log, freq_base)
    } else {
        format!("{} Logical Threads{}", log, freq_base)
    };
    SelectObject(hdc, ctx.font_small);
    SetTextColor(hdc, COLOR_TEXT_DIM);
    draw_text_line(hdc, pad_x, y, &cores_spec_str, DT_LEFT);

    y += 22;

    // Summary Metrics: 4 Clean Micro-Cards
    let cpu_u = m.cpu_usage.unwrap_or(0.0);
    let cpu_t = m.cpu_temperature.map(|t| format!("{:.0}°C", t)).unwrap_or_else(|| "N/A".to_string());
    let avg_ghz = m.cpu_frequency.map(|f| format!("{:.2} GHz", f)).unwrap_or_else(|| "N/A".to_string());

    // Find peak core clock
    let max_mhz = m.cpu_cores.iter().map(|c| c.frequency_mhz).max().unwrap_or(0);
    let peak_str = if max_mhz >= 1000 {
        format!("{:.2} GHz", max_mhz as f64 / 1000.0)
    } else if max_mhz > 0 {
        format!("{} MHz", max_mhz)
    } else {
        "N/A".to_string()
    };

    let card_gap = 8;
    let card_w = (content_w - (card_gap * 3)) / 4;
    let card_h = 42;

    let cards = [
        ("LOAD", format!("{:.0}%", cpu_u), COLOR_TEXT_PRIMARY),
        ("TEMP", cpu_t, if m.cpu_temperature.unwrap_or(0.0) >= 85.0 { super::COLOR_BAR_WARN } else { COLOR_TEXT_PRIMARY }),
        ("AVG CLOCK", avg_ghz, COLOR_TEXT_PRIMARY),
        ("PEAK CLOCK", peak_str, brand_color),
    ];

    for (idx, (label, val, val_color)) in cards.iter().enumerate() {
        let cx = pad_x + (idx as i32) * (card_w + card_gap);
        let cy = y;

        let card_rect = RECT {
            left: cx,
            top: cy,
            right: cx + card_w,
            bottom: cy + card_h,
        };

        let card_bg = CreateSolidBrush(COLOR_TRACK);
        FillRect(hdc, &card_rect, card_bg);
        let _ = DeleteObject(card_bg);

        let border_brush = CreateSolidBrush(COLOR_BORDER);
        let _ = FrameRect(hdc, &card_rect, border_brush);
        let _ = DeleteObject(border_brush);

        // Card Label (Top)
        SelectObject(hdc, ctx.font_small);
        SetTextColor(hdc, COLOR_TEXT_DIM);
        let mut top_rc = RECT {
            left: cx,
            top: cy + 4,
            right: cx + card_w,
            bottom: cy + 18,
        };
        let mut w_label: Vec<u16> = label.encode_utf16().collect();
        let _ = windows::Win32::Graphics::Gdi::DrawTextW(hdc, &mut w_label, &mut top_rc, DT_CENTER | DT_SINGLELINE);

        // Card Value (Bottom)
        SelectObject(hdc, ctx.font_core);
        SetTextColor(hdc, *val_color);
        let mut btm_rc = RECT {
            left: cx,
            top: cy + 20,
            right: cx + card_w,
            bottom: cy + 38,
        };
        let mut w_val: Vec<u16> = val.encode_utf16().collect();
        let _ = windows::Win32::Graphics::Gdi::DrawTextW(hdc, &mut w_val, &mut btm_rc, DT_CENTER | DT_SINGLELINE);
    }

    y += card_h + 12;
    draw_line(hdc, pad_x, y, width - pad_x, COLOR_BORDER);
    y += 10;

    // Section title
    SelectObject(hdc, ctx.font_small);
    SetTextColor(hdc, COLOR_TEXT_LABEL);
    draw_text_line(hdc, pad_x, y, &format!("LOGICAL PROCESSORS ({} THREADS)", log), DT_LEFT);
    y += 18;
    // Core Grid (2 columns)
    let cols = 2;
    let col_w = (content_w - 10) / 2;
    let row_h = 38;

    for (i, core) in m.cpu_cores.iter().enumerate() {
        let col = (i % cols) as i32;
        let row = (i / cols) as i32;
        let item_x = pad_x + col * (col_w + 10);
        let item_y = y + row * row_h;

        let item_rect = RECT {
            left: item_x,
            top: item_y,
            right: item_x + col_w,
            bottom: item_y + 34,
        };

        // Item background card
        let item_bg = CreateSolidBrush(COLOR_TRACK);
        FillRect(hdc, &item_rect, item_bg);
        let _ = DeleteObject(item_bg);

        let item_border = CreateSolidBrush(COLOR_BORDER);
        let _ = FrameRect(hdc, &item_rect, item_border);
        let _ = DeleteObject(item_border);

        // 1. Thread Label (Left, subtle slate like card labels)
        SelectObject(hdc, ctx.font_core);
        SetTextColor(hdc, COLOR_TEXT_DIM);
        let mut t_rc = RECT {
            left: item_x + 8,
            top: item_y + 4,
            right: item_x + 36,
            bottom: item_y + 20,
        };
        let mut w_tid: Vec<u16> = format!("T{}", core.id).encode_utf16().collect();
        let _ = windows::Win32::Graphics::Gdi::DrawTextW(hdc, &mut w_tid, &mut t_rc, DT_LEFT | DT_SINGLELINE);

        let freq_str = if core.frequency_mhz >= 1000 {
            format!("{:.2} GHz", core.frequency_mhz as f64 / 1000.0)
        } else if core.frequency_mhz > 0 {
            format!("{} MHz", core.frequency_mhz)
        } else {
            "-".to_string()
        };

        // 2. Frequency (Center, clean crisp white)
        SetTextColor(hdc, COLOR_TEXT_PRIMARY);
        let mut f_rc = RECT {
            left: item_x + 38,
            top: item_y + 4,
            right: item_x + col_w - 52,
            bottom: item_y + 20,
        };
        let mut w_freq: Vec<u16> = freq_str.encode_utf16().collect();
        let _ = windows::Win32::Graphics::Gdi::DrawTextW(hdc, &mut w_freq, &mut f_rc, DT_LEFT | DT_SINGLELINE);

        // 3. Usage % (Right, strictly bounded)
        SetTextColor(hdc, COLOR_TEXT_PRIMARY);
        let mut u_rc = RECT {
            left: item_x + col_w - 50,
            top: item_y + 4,
            right: item_x + col_w - 8,
            bottom: item_y + 20,
        };
        let mut w_usage: Vec<u16> = format!("{:.0}%", core.usage).encode_utf16().collect();
        let _ = windows::Win32::Graphics::Gdi::DrawTextW(hdc, &mut w_usage, &mut u_rc, DT_RIGHT | DT_SINGLELINE);

        // 4. Progress bar for core usage
        let bar_w = col_w - 16;
        draw_progress_bar(
            hdc,
            item_x + 8,
            item_y + 24,
            bar_w,
            3,
            core.usage / 100.0,
            brand_color,
        );
    }

    // Footer
    let footer_y = height - 20;
    SelectObject(hdc, ctx.font_small);
    SetTextColor(hdc, COLOR_TEXT_DIM);
    draw_text_line(hdc, pad_x, footer_y, "Esc to close • Real-time 1s poll", DT_LEFT);
}

fn position_inspector_window(hwnd: HWND, width: i32, height: i32) {
    unsafe {
        let mut work_area = RECT::default();
        if windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut _ as *mut c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let x = (work_area.right - width - 16).max(work_area.left);
            let y = (work_area.bottom - height - 16).max(work_area.top);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                width,
                height,
                SWP_SHOWWINDOW,
            );
        }
    }
}
