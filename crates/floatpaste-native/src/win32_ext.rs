//! Slint 窗口的 Win32 侧装配：HWND 提取、无边框置顶工具窗样式、
//! DWM 悬浮阴影与系统圆角。

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWM_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// 取 Slint 窗口的原始 HWND（窗口创建后可用）
pub fn window_hwnd<W: ComponentHandle>(window: &W) -> Option<isize> {
    let handle = window.window().window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as isize),
        _ => None,
    }
}

/// 窗口物理 DPI（100%=96）
pub fn window_dpi(hwnd: isize) -> u32 {
    unsafe { GetDpiForWindow(HWND(hwnd as *mut _)) }.max(96)
}

pub fn physical_rect(hwnd: isize) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut rect) }.ok()?;
    Some(rect)
}

/// 无边框浮层样式：跳过任务栏（TOOLWINDOW）+ 不抢焦点（NOACTIVATE）。
/// winit 默认还挂 WS_EX_APPWINDOW——「窗口可见即强制给任务栏按钮」，
/// 与 TOOLWINDOW 相冲，必须显式清掉跳任务栏才生效。
/// 透明渲染由 Slint/winit 的 DWM 合成负责，不要手工加 WS_EX_LAYERED——
/// 未调 SetLayeredWindowAttributes 的分层窗口既不绘制也不命中鼠标
pub fn apply_overlay_style(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            ex_style & !(WS_EX_APPWINDOW.0 as isize)
                | WS_EX_TOOLWINDOW.0 as isize
                | WS_EX_NOACTIVATE.0 as isize,
        );
    }
}

/// 给无边框窗口挂系统悬浮阴影（原版 Tauri shadow(true) 的等效）：
/// DWM 非客户区渲染 + 1px 底部外框即可让合成器绘制标准投影
pub fn apply_dwm_shadow(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    let margins = MARGINS {
        cxLeftWidth: 0,
        cxRightWidth: 0,
        cyTopHeight: 0,
        cyBottomHeight: 1,
    };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }
}

/// Win11 系统圆角（跟随系统半径与投影轮廓，对齐原版观感）：
/// tao 的无边框阴影窗保留 WS_CAPTION/WS_THICKFRAME 样式（WM_NCCALCSIZE
/// 吃掉非客户区），合成器按默认策略给这类窗口自动加圆角；winit 无边框
/// 窗是纯 WS_POPUP，须显式声明。Win10 无此属性，调用失败即无圆角
pub fn apply_dwm_rounded_corners(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const DWM_WINDOW_CORNER_PREFERENCE as *const _,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}
