use std::sync::Mutex;
use std::collections::HashMap;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tracing::{info, warn};

#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, HWND_TOPMOST, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW};

/// Return the cursor position in physical screen coordinates.
#[cfg(target_os = "windows")]
fn get_physical_cursor_position() -> (i32, i32) {
    let mut pt = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

#[cfg(not(target_os = "windows"))]
fn get_physical_cursor_position() -> (i32, i32) {
    (0, 0)
}

pub const TOOLTIP_WINDOW_LABEL: &str = "tooltip";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PendingTooltipRequest {
    pub request_id: u32,
    pub x: f64,
    pub y: f64,
}

static PENDING_TOOLTIP_REQUEST: Mutex<Option<PendingTooltipRequest>> = Mutex::new(None);

fn set_pending_tooltip_request(request: Option<PendingTooltipRequest>) {
    *PENDING_TOOLTIP_REQUEST.lock().unwrap() = request;
}

#[cfg(test)]
fn take_pending_tooltip_request() -> Option<PendingTooltipRequest> {
    PENDING_TOOLTIP_REQUEST.lock().unwrap().take()
}

fn take_matching_pending_tooltip_request(request_id: u32) -> Option<PendingTooltipRequest> {
    let mut pending_request = PENDING_TOOLTIP_REQUEST.lock().unwrap();
    match *pending_request {
        Some(request) if request.request_id == request_id => {
            *pending_request = None;
            Some(request)
        }
        _ => None,
    }
}

fn clear_pending_tooltip_request() {
    set_pending_tooltip_request(None);
}

pub struct TooltipWindow;

impl TooltipWindow {
    pub fn ensure_window(app: &AppHandle) -> Result<WebviewWindow, String> {
        if let Some(window) = app.get_webview_window(TOOLTIP_WINDOW_LABEL) {
            return Ok(window);
        }

        let window = WebviewWindowBuilder::new(
            app,
            TOOLTIP_WINDOW_LABEL,
            WebviewUrl::App("tooltip.html".into()),
        )
        .title("FloatPaste · Tooltip")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|e| {
            let msg = format!("创建 tooltip 窗口失败: {e}");
            warn!("{msg}");
            msg
        })?;

        info!("tooltip 窗口已创建");
        configure_tooltip_window(&window);
        Ok(window)
    }

    pub fn show_tooltip(
        app: &AppHandle,
        request_id: u32,
        x: f64,
        y: f64,
        html: String,
        theme: &str,
        theme_vars: &HashMap<String, String>,
    ) -> Result<(), String> {
        let window = Self::ensure_window(app)?;

        set_pending_tooltip_request(Some(PendingTooltipRequest { request_id, x, y }));

        let json_request_id = serde_json::to_string(&request_id)
            .map_err(|e| format!("Tooltip requestId 序列化失败: {e}"))?;
        let json_html = serde_json::to_string(&html)
            .map_err(|e| format!("Tooltip HTML 序列化失败: {e}"))?;
        let json_theme = serde_json::to_string(theme)
            .map_err(|e| format!("Tooltip theme 序列化失败: {e}"))?;
        let json_theme_vars = serde_json::to_string(theme_vars)
            .map_err(|e| format!("Tooltip themeVars 序列化失败: {e}"))?;
        window.eval(&format!(
            "window.showTooltip({}, {}, {}, {})",
            json_request_id, json_html, json_theme, json_theme_vars
        ))
            .map_err(|e| {
                clear_pending_tooltip_request();
                warn!("tooltip JS eval 失败: {e}");
                e.to_string()
            })?;

        Ok(())
    }

    pub fn on_tooltip_ready(
        app: &AppHandle,
        request_id: u32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let Some(request) = take_matching_pending_tooltip_request(request_id) else {
            return Ok(());
        };

        let Some(window) = app.get_webview_window(TOOLTIP_WINDOW_LABEL) else {
            return Ok(());
        };

        // Add padding to prevent scrollbar clipping
        let w = width + 4;
        let h = height + 4;
        if let Err(e) = window.set_size(Size::Physical(PhysicalSize::new(w, h))) {
            warn!("tooltip 窗口设置大小失败: {e}");
        }

        // Resolve tooltip position, flipping when it would overflow the work area
        let (x, y) = Self::resolve_clamped_position(app, request.x, request.y, w, h);

        if let Err(e) = window.set_position(Position::Physical(PhysicalPosition::new(x, y))) {
            warn!("tooltip 窗口设置位置失败: {e}");
        }

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::window_utils::set_window_click_through(&window)?;

            let tauri_hwnd = window.hwnd().map_err(|e| e.to_string())?;
            let hwnd = windows::Win32::Foundation::HWND(tauri_hwnd.0 as isize as *mut _);
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            window.show().map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn hide_tooltip(app: &AppHandle) -> Result<(), String> {
        clear_pending_tooltip_request();

        let Some(window) = app.get_webview_window(TOOLTIP_WINDOW_LABEL) else {
            return Ok(());
        };

        if let Err(e) = window.eval("window.hideTooltip()") {
            warn!("tooltip hide eval 失败: {e}");
        }

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::window_utils::hide_window(&window)?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            window.hide().map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    /// Resolve tooltip position, flipping to the opposite side of the cursor when
    /// the default placement would overflow the monitor work area.
    fn resolve_clamped_position(app: &AppHandle, x: f64, y: f64, w: u32, h: u32) -> (i32, i32) {
        let monitors = app.available_monitors().unwrap_or_default();
        let primary = app.primary_monitor().ok().flatten();

        let work_area = monitors
            .iter()
            .find(|m| {
                let pos = m.position();
                let sz = m.size();
                x >= pos.x as f64
                    && x < (pos.x + sz.width as i32) as f64
                    && y >= pos.y as f64
                    && y < (pos.y + sz.height as i32) as f64
            })
            .or(primary.as_ref())
            .map(|m| m.work_area().clone());

        let Some(wa) = work_area else {
            return (x as i32, y as i32);
        };

        let wa_right = wa.position.x + wa.size.width as i32;
        let wa_bottom = wa.position.y + wa.size.height as i32;

        // Get cursor position so we can flip to the opposite side when needed
        let (cx, cy) = get_physical_cursor_position();

        // When flipping, leave a gap between cursor and tooltip edge
        const FLIP_GAP: i32 = 4;

        let mut tx = x as i32;
        let mut ty = y as i32;

        // Flip horizontally if tooltip would overflow the right edge
        if tx + w as i32 > wa_right {
            tx = cx - w as i32 - FLIP_GAP;
        }
        // Flip vertically if tooltip would overflow the bottom edge
        if ty + h as i32 > wa_bottom {
            ty = cy - h as i32 - FLIP_GAP;
        }

        (tx, ty)
    }
}

pub(crate) fn configure_tooltip_window(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    if let Err(error) = crate::platform::windows::window_utils::remove_window_system_menu(window) {
        warn!("移除 tooltip 系统菜单失败: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_pending_tooltip_request, set_pending_tooltip_request,
        take_matching_pending_tooltip_request, take_pending_tooltip_request,
        PendingTooltipRequest,
    };

    #[test]
    fn hide_clears_pending_tooltip_request_for_late_ready_callbacks() {
        set_pending_tooltip_request(Some(PendingTooltipRequest {
            request_id: 7,
            x: 320.0,
            y: 240.0,
        }));

        clear_pending_tooltip_request();

        assert_eq!(take_pending_tooltip_request(), None);
    }

    #[test]
    fn stale_request_id_does_not_consume_newer_pending_tooltip_request() {
        set_pending_tooltip_request(Some(PendingTooltipRequest {
            request_id: 9,
            x: 512.0,
            y: 288.0,
        }));

        assert_eq!(take_matching_pending_tooltip_request(8), None);
        assert_eq!(
            take_pending_tooltip_request(),
            Some(PendingTooltipRequest {
                request_id: 9,
                x: 512.0,
                y: 288.0,
            })
        );
    }
}
