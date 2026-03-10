use std::mem::size_of;

use windows::core::Error;
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    },
    UI::WindowsAndMessaging::{
        GetCursorPos, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

pub fn current_cursor_point() -> Result<ScreenPoint, String> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.map_err(windows_error_to_string)?;
    Ok(ScreenPoint {
        x: point.x,
        y: point.y,
    })
}

pub fn caret_point_for_window(hwnd: isize) -> Result<ScreenPoint, String> {
    let hwnd = HWND(hwnd as *mut _);
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, None) };
    if thread_id == 0 {
        return Err("未找到目标窗口线程".to_string());
    }

    let mut gui_info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread_id, &mut gui_info) }.map_err(windows_error_to_string)?;

    if gui_info.hwndCaret.0.is_null() {
        return Err("目标线程没有可用插入符".to_string());
    }

    let mut point = POINT {
        x: gui_info.rcCaret.left + rect_width(gui_info.rcCaret) / 2,
        y: gui_info.rcCaret.bottom,
    };
    unsafe { ClientToScreen(gui_info.hwndCaret, &mut point) }
        .ok()
        .map_err(windows_error_to_string)?;

    Ok(ScreenPoint {
        x: point.x,
        y: point.y,
    })
}

pub fn work_area_from_point(point: ScreenPoint) -> Result<ScreenRect, String> {
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: point.x,
                y: point.y,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    if monitor.0.is_null() {
        return Err("未找到目标显示器".to_string());
    }

    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut monitor_info as *mut _ as *mut MONITORINFO) }
        .ok()
        .map_err(windows_error_to_string)?;

    Ok(ScreenRect {
        left: monitor_info.rcWork.left,
        top: monitor_info.rcWork.top,
        right: monitor_info.rcWork.right,
        bottom: monitor_info.rcWork.bottom,
    })
}

fn rect_width(rect: RECT) -> i32 {
    rect.right - rect.left
}

fn windows_error_to_string(error: Error) -> String {
    error.to_string()
}
