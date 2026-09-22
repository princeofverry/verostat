use std::sync::Arc;

use parking_lot::RwLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, LOGFONTW,
    SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_NOCLIP, DT_SINGLELINE, DT_VCENTER,
    HDC, HFONT, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetSystemMetrics, IsWindowVisible,
    RegisterClassExW, SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA, HCURSOR, HTCAPTION,
    HWND_TOPMOST, LWA_ALPHA, SM_CXSCREEN, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_DESTROY,
    WM_ERASEBKGND, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_PAINT,
    WM_RBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::app::SystemMetrics;
use crate::monitor::network::format_speed;
use crate::monitor::WM_METRICS_UPDATED;

const HUD_WIDTH: i32 = 360;
const HUD_HEIGHT: i32 = 34;
const HUD_CLASS_NAME: PCWSTR = w!("VeroStatFloatingHudClass");
const HUD_TITLE: PCWSTR = w!("VeroStatHUD");

const COLOR_HUD_BG: COLORREF = COLORREF(0x00141212); // Deep Charcoal #121214
const COLOR_HUD_BORDER: COLORREF = COLORREF(0x00332D2D); // Subtle Border #2D2D33
const COLOR_TEXT_WHITE: COLORREF = COLORREF(0x00F8FAFC); // White
const COLOR_ACCENT_RED: COLORREF = COLORREF(0x004444EF); // Red #EF4444
const COLOR_ACCENT_ORANGE: COLORREF = COLORREF(0x000B9EF5); // Amber #F59E0B

struct HudContext {
    metrics: Arc<RwLock<SystemMetrics>>,
    font: HFONT,
}

impl Drop for HudContext {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font);
            }
        }
    }
}

pub struct FloatingHud {
    pub hwnd: HWND,
}

impl FloatingHud {
    pub fn new(metrics: Arc<RwLock<SystemMetrics>>) -> Result<Self, String> {
        unsafe {
            let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .map_err(|e| format!("Failed to get module handle: {:?}", e))?;

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(hud_window_proc);
            wc.hInstance = hinstance.into();
            wc.lpszClassName = HUD_CLASS_NAME;
            wc.hCursor = HCURSOR::default();

            let _ = RegisterClassExW(&wc);

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let start_x = (screen_w - HUD_WIDTH) / 2;
            let start_y = 12;

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                HUD_CLASS_NAME,
                HUD_TITLE,
                WS_POPUP,
                start_x,
                start_y,
                HUD_WIDTH,
                HUD_HEIGHT,
                None,
                None,
                hinstance,
                None,
            )
            .map_err(|e| format!("Failed to create HUD window: {:?}", e))?;

            // 90% opacity (230 / 255)
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 230, LWA_ALPHA);

            let font = create_font(13, 600);
            let context = Box::new(HudContext { metrics, font });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(context) as isize);

            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                start_x,
                start_y,
                HUD_WIDTH,
                HUD_HEIGHT,
                SWP_SHOWWINDOW,
            );

            Ok(Self { hwnd })
        }
    }

    pub fn toggle_visibility(&self) {
        unsafe {
            if IsWindowVisible(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            } else {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
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

unsafe extern "system" fn hud_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    let ctx = if ptr != 0 {
        &mut *(ptr as *mut HudContext)
    } else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };

    match msg {
        WM_LBUTTONDOWN => {
            // Allow dragging the HUD anywhere
            let _ = ReleaseCapture();
            let _ = SendMessageW(
                hwnd,
                WM_NCLBUTTONDOWN,
                WPARAM(HTCAPTION as usize),
                LPARAM(0),
            );
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK | WM_RBUTTONUP => {
            // Double-click or Right-click hides the HUD
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
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

            render_hud(mem_dc, width, height, ctx);

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
        WM_DESTROY => {
            let _ = Box::from_raw(ctx as *mut HudContext);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn render_hud(
    hdc: HDC,
    width: i32,
    height: i32,
    ctx: &HudContext,
) {
    // 1. Background Pill
    let bg_brush = CreateSolidBrush(COLOR_HUD_BG);
    let full_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    FillRect(hdc, &full_rect, bg_brush);
    let _ = DeleteObject(bg_brush);

    // Border
    let border_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: 1,
    };
    let border_brush = CreateSolidBrush(COLOR_HUD_BORDER);
    FillRect(hdc, &border_rect, border_brush);
    let _ = DeleteObject(border_brush);

    SetBkMode(hdc, TRANSPARENT);
    SelectObject(hdc, ctx.font);

    let m = ctx.metrics.read().clone();

    // Stats
    let cpu_u = m.cpu_usage.unwrap_or(0.0);
    let cpu_t = m.cpu_temperature.map(|t| format!("{:.0}°C", t)).unwrap_or_else(|| "N/A".to_string());
    let gpu_u = m.gpu_usage.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "N/A".to_string());
    let gpu_t = m.gpu_temperature.map(|t| format!("{:.0}°C", t)).unwrap_or_else(|| "N/A".to_string());
    let ram_gb = m.ram_used as f64 / 1024.0 / 1024.0 / 1024.0;
    let dl_str = format_speed(m.download_speed);

    let hud_text = format!(
        "CPU {:.0}% {}   |   GPU {} {}   |   RAM {:.1}G   |   ↓{}",
        cpu_u, cpu_t,
        gpu_u, gpu_t,
        ram_gb,
        dl_str
    );

    let mut text_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };

    // Color based on temperatures
    let text_color = if m.cpu_temperature.unwrap_or(0.0) >= 85.0 || m.gpu_temperature.unwrap_or(0.0) >= 85.0 {
        COLOR_ACCENT_RED
    } else if m.cpu_temperature.unwrap_or(0.0) >= 75.0 || m.gpu_temperature.unwrap_or(0.0) >= 75.0 {
        COLOR_ACCENT_ORANGE
    } else {
        COLOR_TEXT_WHITE
    };

    SetTextColor(hdc, text_color);
    let mut wide: Vec<u16> = hud_text.encode_utf16().collect();
    let _ = DrawTextW(
        hdc,
        &mut wide,
        &mut text_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP,
    );
}
