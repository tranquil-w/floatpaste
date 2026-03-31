use std::sync::Mutex;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tracing::{info, warn};

#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{HWND_TOPMOST, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW};

pub const TOOLTIP_WINDOW_LABEL: &str = "tooltip";

static PENDING_TOOLTIP_POS: Mutex<Option<(f64, f64)>> = Mutex::new(None);

fn set_pending_tooltip_pos(pos: Option<(f64, f64)>) {
    *PENDING_TOOLTIP_POS.lock().unwrap() = pos;
}

#[cfg(test)]
fn take_pending_tooltip_pos() -> Option<(f64, f64)> {
    PENDING_TOOLTIP_POS.lock().unwrap().take()
}

fn clear_pending_tooltip_pos() {
    set_pending_tooltip_pos(None);
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

    pub fn show_tooltip(app: &AppHandle, x: f64, y: f64, html: String, theme: &str) -> Result<(), String> {
        let window = Self::ensure_window(app)?;

        set_pending_tooltip_pos(Some((x, y)));

        let json_html = serde_json::to_string(&html)
            .map_err(|e| format!("Tooltip HTML 序列化失败: {e}"))?;
        let json_theme = serde_json::to_string(theme)
            .map_err(|e| format!("Tooltip theme 序列化失败: {e}"))?;
        window.eval(&format!("window.showTooltip({}, {})", json_html, json_theme))
            .map_err(|e| {
                clear_pending_tooltip_pos();
                warn!("tooltip JS eval 失败: {e}");
                e.to_string()
            })?;

        Ok(())
    }

    pub fn on_tooltip_ready(app: &AppHandle, width: u32, height: u32) -> Result<(), String> {
        let pos = PENDING_TOOLTIP_POS
            .lock()
            .unwrap()
            .take()
            .ok_or("tooltip_ready: 无待处理的位置")?;

        let Some(window) = app.get_webview_window(TOOLTIP_WINDOW_LABEL) else {
            return Ok(());
        };

        // Add padding to prevent scrollbar clipping
        let w = width + 4;
        let h = height + 4;
        if let Err(e) = window.set_size(Size::Physical(PhysicalSize::new(w, h))) {
            warn!("tooltip 窗口设置大小失败: {e}");
        }
        if let Err(e) = window.set_position(Position::Physical(PhysicalPosition::new(
            pos.0 as i32,
            pos.1 as i32,
        ))) {
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
        clear_pending_tooltip_pos();

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
}

pub(crate) fn configure_tooltip_window(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    if let Err(error) = crate::platform::windows::window_utils::remove_window_system_menu(window) {
        warn!("移除 tooltip 系统菜单失败: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::{clear_pending_tooltip_pos, set_pending_tooltip_pos, take_pending_tooltip_pos};

    #[test]
    fn hide_clears_pending_tooltip_position_for_late_ready_callbacks() {
        set_pending_tooltip_pos(Some((320.0, 240.0)));

        clear_pending_tooltip_pos();

        assert_eq!(take_pending_tooltip_pos(), None);
    }
}
