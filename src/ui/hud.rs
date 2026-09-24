use std::sync::Arc;

use parking_lot::RwLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect,
    GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, HDC, HFONT, LOGFONTW, PAINTSTRUCT,
    SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetSystemMetrics, GetWindowRect,
    IsWindowVisible, RegisterClassExW, SendMessageW, SetForegroundWindow,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    GWLP_USERDATA, HCURSOR, HTCAPTION, HWND_TOPMOST, LWA_ALPHA, SM_CXSCREEN,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW,
    WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN,
    WM_PAINT, WM_RBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::app::{AppConfig, SystemMetrics};
use crate::monitor::network::format_speed;
use crate::monitor::WM_METRICS_UPDATED;

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
    config: Arc<RwLock<AppConfig>>,
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
    pub fn new(
        metrics: Arc<RwLock<SystemMetrics>>,
        config: Arc<RwLock<AppConfig>>,
    ) -> Result<Self, String> {
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
            let font = create_font(13, 600);

            let screen_dc = GetDC(HWND(std::ptr::null_mut()));
            let initial_text = build_hud_text(&metrics.read(), &config.read());
            let initial_width = if !screen_dc.is_invalid() {
                let w = calculate_hud_width(screen_dc, font, &initial_text);
                let _ = ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);
                w
            } else {
                380
            };

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let start_x = (screen_w - initial_width) / 2;
            let start_y = 12;

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                HUD_CLASS_NAME,
                HUD_TITLE,
                WS_POPUP,
                start_x,
                start_y,
                initial_width,
                HUD_HEIGHT,
                None,
                None,
                hinstance,
                None,
            )
            .map_err(|e| format!("Failed to create HUD window: {:?}", e))?;

            let opacity_pct = config.read().hud_opacity.clamp(10, 100);
            let alpha = ((opacity_pct as f32 / 100.0) * 255.0).round() as u8;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
            let context = Box::new(HudContext {
                metrics,
                config,
                font,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(context) as isize);

            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                start_x,
                start_y,
                initial_width,
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
                let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(self.hwnd, GWLP_USERDATA);
                if ptr != 0 {
                    let ctx = &*(ptr as *const HudContext);
                    adjust_hud_size_if_needed(self.hwnd, ctx);
                }
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }
    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }

    pub fn set_opacity(&self, opacity_pct: u8) {
        let clamped = opacity_pct.clamp(10, 100);
        let alpha = ((clamped as f32 / 100.0) * 255.0).round() as u8;
        unsafe {
            let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), alpha, LWA_ALPHA);
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

            render_hud(hwnd, mem_dc, width, height, ctx);

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
                adjust_hud_size_if_needed(hwnd, ctx);
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

pub fn build_hud_text(m: &SystemMetrics, cfg: &AppConfig) -> String {
    let mut segments = Vec::new();

    if cfg.hud_show_cpu {
        let cpu_u = m.cpu_usage.unwrap_or(0.0);
        let cpu_t = m.cpu_temperature.map(|t| format!("{:.0}°C", t)).unwrap_or_else(|| "N/A".to_string());
        segments.push(format!("CPU {:.0}% {}", cpu_u, cpu_t));
    }

    if cfg.hud_show_gpu {
        let gpu_u = m.gpu_usage.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "N/A".to_string());
        let gpu_t = m.gpu_temperature.map(|t| format!("{:.0}°C", t)).unwrap_or_else(|| "N/A".to_string());
        segments.push(format!("GPU {} {}", gpu_u, gpu_t));
    }

    if cfg.hud_show_ram {
        let ram_gb = m.ram_used as f64 / 1024.0 / 1024.0 / 1024.0;
        segments.push(format!("RAM {:.1}G", ram_gb));
    }

    if cfg.hud_show_network {
        let dl_str = format_speed(m.download_speed);
        segments.push(format!("↓{}", dl_str));
    }

    if !segments.is_empty() {
        segments.join("   |   ")
    } else {
        "VeroStat HUD (Select elements in tray)".to_string()
    }
}

unsafe fn calculate_hud_width(hdc: HDC, font: HFONT, text: &str) -> i32 {
    let old_font = SelectObject(hdc, font);
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut text_size = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &wide, &mut text_size);
    SelectObject(hdc, old_font);

    // Provide generous horizontal padding (20px left, 20px right = 40px)
    // and round up to a multiple of 16 to avoid resizing jitter on small digit changes
    let min_width = text_size.cx + 40;
    let aligned_width = ((min_width + 15) / 16) * 16;
    aligned_width.max(220)
}

unsafe fn adjust_hud_size_if_needed(hwnd: HWND, ctx: &HudContext) {
    let m = ctx.metrics.read().clone();
    let cfg = ctx.config.read().clone();
    let text = build_hud_text(&m, &cfg);

    let hdc = GetDC(hwnd);
    if hdc.is_invalid() {
        return;
    }
    let desired_width = calculate_hud_width(hdc, ctx.font, &text);
    let _ = ReleaseDC(hwnd, hdc);

    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let current_width = rect.right - rect.left;

    // Expand immediately if current width is too small to avoid clipping.
    // Shrink only if current width is notably larger (> 24px) to avoid jitter.
    if current_width < desired_width || (current_width - desired_width) > 24 {
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let mut cur_x = rect.left;
        let cur_y = rect.top;

        // Ensure window stays completely on-screen when expanding
        if cur_x + desired_width > screen_w {
            cur_x = (screen_w - desired_width).max(0);
        }

        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            cur_x,
            cur_y,
            desired_width,
            HUD_HEIGHT,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

unsafe fn render_hud(
    hwnd: HWND,
    hdc: HDC,
    width: i32,
    height: i32,
    ctx: &HudContext,
) {
    let bg_brush = CreateSolidBrush(COLOR_HUD_BG);
    let full_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    FillRect(hdc, &full_rect, bg_brush);
    let _ = DeleteObject(bg_brush);

    let border_brush = CreateSolidBrush(COLOR_HUD_BORDER);
    let _ = FrameRect(hdc, &full_rect, border_brush);
    let _ = DeleteObject(border_brush);

    SetBkMode(hdc, TRANSPARENT);
    SelectObject(hdc, ctx.font);

    let m = ctx.metrics.read().clone();
    let cfg = ctx.config.read().clone();

    let hud_text = build_hud_text(&m, &cfg);

    // Safety fallback: if for any reason current width is smaller than needed,
    // adjust window size so clipping is never persistent
    let desired_width = calculate_hud_width(hdc, ctx.font, &hud_text);
    if width < desired_width {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            desired_width,
            HUD_HEIGHT,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    let mut text_rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };

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
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_hud_text_all_enabled() {
        let mut m = SystemMetrics::default();
        m.cpu_usage = Some(18.0);
        m.cpu_temperature = Some(50.0);
        m.gpu_usage = Some(0.0);
        m.gpu_temperature = Some(51.0);
        m.ram_used = (14.6 * 1024.0 * 1024.0 * 1024.0) as u64;
        m.download_speed = 1024;

        let mut cfg = AppConfig::default();
        cfg.hud_show_network = true;
        let text = build_hud_text(&m, &cfg);
        assert!(text.contains("CPU 18% 50°C"));
        assert!(text.contains("GPU 0% 51°C"));
        assert!(text.contains("RAM 14.6G"));
        assert!(text.contains("↓1 KB/s"));
    }

    #[test]
    fn test_build_hud_text_empty_fallback() {
        let m = SystemMetrics::default();
        let mut cfg = AppConfig::default();
        cfg.hud_show_cpu = false;
        cfg.hud_show_gpu = false;
        cfg.hud_show_ram = false;
        cfg.hud_show_network = false;

        let text = build_hud_text(&m, &cfg);
        assert_eq!(text, "VeroStat HUD (Select elements in tray)");
    }
}
