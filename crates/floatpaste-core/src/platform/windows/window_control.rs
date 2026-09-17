//! 原生窗口控制：全部以裸 HWND 为参数，不绑定任何 GUI 框架。
//!
//! 覆盖最小尺寸约束（WM_GETMINMAXINFO 子类）、Alt 系统菜单拦截、
//! 无激活显示/隐藏、置顶、点击穿透、标题栏拖拽与八方向拉伸。

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::SetActiveWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetCursorPos, GetWindow, GetWindowLongPtrW, GetWindowRect, IsIconic,
    IsWindowVisible, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GW_HWNDPREV, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, LWA_ALPHA,
    SC_KEYMENU, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, WM_GETMINMAXINFO,
    WM_SYSCOMMAND, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
};

use crate::domain::error::AppError;

/// 窗口拉伸方向，对应 Tauri `ResizeDirection` / HTML `data-tauri-drag` 语义
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeDirection {
    North,
    South,
    West,
    East,
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

fn hwnd_of(hwnd: isize) -> HWND {
    HWND(hwnd as *mut _)
}

fn strip_system_menu_style(style: isize) -> isize {
    style & !(WS_SYSMENU.0 as isize) & !(WS_MINIMIZEBOX.0 as isize) & !(WS_MAXIMIZEBOX.0 as isize)
}

fn is_alt_menu_syscommand(wparam: usize) -> bool {
    (wparam & 0xFFF0) == SC_KEYMENU as usize
}

/// 移除系统菜单/最小化/最大化样式：无边框工具窗口按 Alt 或任务栏右键
/// 仍会弹出系统菜单，原版对所有浮层窗口做了同样的剥除
pub fn remove_window_system_menu(hwnd: isize) -> Result<(), AppError> {
    let hwnd = hwnd_of(hwnd);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, strip_system_menu_style(style));
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
    Ok(())
}

/// 隐藏到任务栏与 Alt+Tab 列表（WS_EX_TOOLWINDOW）
pub fn set_window_skip_taskbar(hwnd: isize) {
    let hwnd = hwnd_of(hwnd);
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_TOOLWINDOW.0 as isize);
    }
}

/// 无激活显示：先挂 WS_EX_NOACTIVATE，再 SW_SHOWNOACTIVATE + SWP_SHOWWINDOW。
/// 速贴面板依赖此语义——显示不抢目标窗口前台，键盘焦点留在目标应用
pub fn show_window_no_activate(hwnd: isize) -> Result<(), AppError> {
    let hwnd = hwnd_of(hwnd);
    unsafe {
        let original_ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            original_ex_style | WS_EX_NOACTIVATE.0 as isize,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
        );
    }
    Ok(())
}

pub fn hide_window(hwnd: isize) -> Result<(), AppError> {
    unsafe {
        let _ = ShowWindow(hwnd_of(hwnd), SW_HIDE);
    }
    Ok(())
}

/// 置顶但不激活（tooltip 显示路径）
pub fn set_window_topmost_no_activate(hwnd: isize) -> bool {
    let handle = hwnd_of(hwnd);
    let result = unsafe {
        SetWindowPos(
            handle,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        )
    };
    tracing::debug!("set_window_topmost hwnd={hwnd} ok={}", result.is_ok());
    result.is_ok()
}

/// 查询窗口是否仍带 WS_EX_TOPMOST 位（速贴会话期守护：位被剥即会被
/// 普通窗口覆盖）
pub fn is_topmost(hwnd: isize) -> bool {
    let handle = hwnd_of(hwnd);
    (unsafe { GetWindowLongPtrW(handle, GWL_EXSTYLE) } & WS_EX_TOPMOST.0 as isize) != 0
}

/// 置顶带内 hwnd 之上是否还有与它相交的可见外部窗口（skip 里的自身
/// 窗口如 tooltip 不算）。离开置顶带即停止——非置顶窗口盖不住置顶窗口
pub fn is_covered_by_visible_window(hwnd: isize, skip: &[isize]) -> bool {
    let handle = hwnd_of(hwnd);
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(handle, &mut rect) }.is_err() {
        return false;
    }
    let skip_handles: Vec<_> = skip.iter().map(|s| hwnd_of(*s)).collect();
    let mut cur = unsafe { GetWindow(handle, GW_HWNDPREV) }.ok();
    for _ in 0..64 {
        let Some(h) = cur else {
            break;
        };
        if unsafe { IsWindowVisible(h) }.as_bool()
            && !skip_handles.contains(&h)
        {
            let mut other = RECT::default();
            if unsafe { GetWindowRect(h, &mut other) }.is_ok() {
                let intersects = other.left < rect.right
                    && other.right > rect.left
                    && other.top < rect.bottom
                    && other.bottom > rect.top;
                let wide_enough = other.right - other.left > 4;
                let tall_enough = other.bottom - other.top > 4;
                if intersects && wide_enough && tall_enough {
                    return true;
                }
            }
        }
        // 上方窗口不带置顶位：它盖不住本窗口，无需继续
        let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) };
        if ex & WS_EX_TOPMOST.0 as isize == 0 {
            break;
        }
        cur = unsafe { GetWindow(h, GW_HWNDPREV) }.ok();
    }
    false
}

/// 点击穿透（tooltip）：鼠标命中与滚轮全部落到下层窗口。
/// Windows 命中测试只跳过 LAYERED+TRANSPARENT 组合的窗口——单设
/// WS_EX_TRANSPARENT 不生效；配合 SetLayeredWindowAttributes(alpha=255)
/// 保持完全不透明渲染（未调 SLWA 的分层窗口不会绘制，见 win32_ext 注释）
pub fn set_window_click_through(hwnd: isize) -> Result<(), AppError> {
    let hwnd = hwnd_of(hwnd);
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            ex_style
                | WS_EX_TRANSPARENT.0 as isize
                | WS_EX_NOACTIVATE.0 as isize
                | WS_EX_LAYERED.0 as isize,
        );
        // alpha=255：让分层生效（可穿透/可命中跳过）同时保持完全不透明
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    }
    Ok(())
}

pub fn is_window_minimized(hwnd: isize) -> bool {
    unsafe { IsIconic(hwnd_of(hwnd)).as_bool() }
}

pub fn is_window_visible(hwnd: isize) -> bool {
    unsafe { IsWindowVisible(hwnd_of(hwnd)).as_bool() }
}

pub fn window_rect(hwnd: isize) -> Result<RECT, AppError> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd_of(hwnd), &mut rect)? };
    Ok(rect)
}

pub fn is_cursor_inside_window(hwnd: isize) -> Result<bool, AppError> {
    let rect = window_rect(hwnd)?;
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor)? };
    Ok(cursor.x >= rect.left
        && cursor.x <= rect.right
        && cursor.y >= rect.top
        && cursor.y <= rect.bottom)
}

/// 显示并前置聚焦（搜索窗口路径）：置顶 → BringWindowToTop → 恢复/显示 → 激活
pub fn restore_window_and_focus(hwnd: isize) -> Result<(), AppError> {
    let hwnd = hwnd_of(hwnd);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = BringWindowToTop(hwnd);

        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let _ = SetActiveWindow(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

/* ───────────────── 窗口子类：最小尺寸 + Alt 菜单拦截 ───────────────── */

const WINDOW_CONTROL_SUBCLASS_ID: usize = 0x4650_0101;
const ALT_MENU_BLOCKER_SUBCLASS_ID: usize = 0x4650_0102;

#[derive(Debug, Clone, Copy)]
struct MinSizeState {
    min_width: i32,
    min_height: i32,
}

static MIN_SIZE_STATE: Mutex<Option<MinSizeState>> = Mutex::new(None);
static SUBCLASS_REF_COUNT: AtomicIsize = AtomicIsize::new(0);

/// 设置窗口最小尺寸（物理像素）。拉伸与 WM_GETMINMAXINFO 都遵守该约束
pub fn set_window_min_size(hwnd: isize, min_width: i32, min_height: i32) {
    if let Ok(mut state) = MIN_SIZE_STATE.lock() {
        *state = Some(MinSizeState {
            min_width,
            min_height,
        });
    }

    unsafe {
        let _ = SetWindowSubclass(
            hwnd_of(hwnd),
            Some(window_control_subclass_proc),
            WINDOW_CONTROL_SUBCLASS_ID,
            0,
        );
    }
    SUBCLASS_REF_COUNT.fetch_add(1, Ordering::SeqCst);
}

/// 拦截 Alt 键触发的系统菜单激活（搜索窗口输入期间按 Alt 不弹菜单）
pub fn block_alt_menu_activation(hwnd: isize) {
    unsafe {
        let _ = SetWindowSubclass(
            hwnd_of(hwnd),
            Some(alt_menu_blocker_subclass_proc),
            ALT_MENU_BLOCKER_SUBCLASS_ID,
            0,
        );
    }
}

unsafe extern "system" fn alt_menu_blocker_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if msg == WM_SYSCOMMAND && is_alt_menu_syscommand(wparam.0) {
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn window_control_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if msg == WM_GETMINMAXINFO {
        let state = MIN_SIZE_STATE.lock().ok().and_then(|guard| *guard);
        if let Some(state) = state {
            let info = &mut *(lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::MINMAXINFO);
            if info.ptMinTrackSize.x < state.min_width {
                info.ptMinTrackSize.x = state.min_width;
            }
            if info.ptMinTrackSize.y < state.min_height {
                info.ptMinTrackSize.y = state.min_height;
            }
            return LRESULT(0);
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// 卸载全部子类（窗口销毁前调用，幂等）
pub fn cleanup_window_subclasses(hwnd: isize) {
    let previous = SUBCLASS_REF_COUNT.swap(0, Ordering::SeqCst);
    if previous == 0 {
        return;
    }
    unsafe {
        let _ = RemoveWindowSubclass(
            hwnd_of(hwnd),
            Some(window_control_subclass_proc),
            WINDOW_CONTROL_SUBCLASS_ID,
        );
    }
    if let Ok(mut state) = MIN_SIZE_STATE.lock() {
        *state = None;
    }
}

/* ───────────────── 手势移动/拉伸（非模态） ───────────────── */

/// 手势模式：整体移动或八方向拉伸
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureMode {
    Move,
    Resize(ResizeDirection),
}

struct GestureState {
    mode: GestureMode,
    origin: POINT,
    rect: RECT,
    min_width: i32,
    min_height: i32,
}

impl GestureState {
    fn clone_state(&self) -> Self {
        Self {
            mode: self.mode,
            origin: self.origin,
            rect: self.rect,
            min_width: self.min_width,
            min_height: self.min_height,
        }
    }
}

static GESTURE: Mutex<Option<GestureState>> = Mutex::new(None);

/// 开始手势：记录光标与窗口矩形初值，后续 moved 事件按增量应用。
///
/// 不要用 WM_NCLBUTTONDOWN(HTCAPTION/HTBOTTOM…) 进入系统模态循环：模态循环
/// 会吞掉后续全部指针消息（包括松键），Slint 的 TouchArea 永远收不到抬起
/// 事件、永久卡在按下抢占态——面板此后收不到任何条目点击。
/// 这里全程由 Slint 的 down/moved/up 事件驱动，不干扰其输入状态。
pub fn begin_window_gesture(
    hwnd: isize,
    mode: GestureMode,
    min_width: i32,
    min_height: i32,
) -> bool {
    let mut cursor = POINT::default();
    let mut rect = RECT::default();
    let cursor_ok = unsafe { GetCursorPos(&mut cursor) }.is_ok();
    let rect_ok = unsafe { GetWindowRect(hwnd_of(hwnd), &mut rect) }.is_ok();
    if !cursor_ok || !rect_ok {
        tracing::warn!("begin_window_gesture 失败 hwnd={hwnd} mode={mode:?}");
        return false;
    }
    if let Ok(mut state) = GESTURE.lock() {
        *state = Some(GestureState {
            mode,
            origin: cursor,
            rect,
            min_width: min_width.max(1),
            min_height: min_height.max(1),
        });
    }
    tracing::debug!("begin_window_gesture hwnd={hwnd} mode={mode:?}");
    true
}

/// 手势更新（Slint TouchArea moved 事件驱动）：按光标增量移动/调整窗口
pub fn update_window_gesture(hwnd: isize) {
    let Some(state) = GESTURE
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(GestureState::clone_state))
    else {
        return;
    };
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return;
    }
    let dx = (cursor.x - state.origin.x) as i32;
    let dy = (cursor.y - state.origin.y) as i32;

    let mut left = state.rect.left;
    let mut top = state.rect.top;
    let mut right = state.rect.right;
    let mut bottom = state.rect.bottom;

    match state.mode {
        GestureMode::Move => {
            left += dx;
            right += dx;
            top += dy;
            bottom += dy;
        }
        GestureMode::Resize(direction) => {
            // 固定对边、拖动临边，宽度/高度不小于最小值（物理像素）
            let min_w = state.min_width;
            let min_h = state.min_height;
            if matches!(
                direction,
                ResizeDirection::North | ResizeDirection::NorthWest | ResizeDirection::NorthEast
            ) {
                top = (top + dy).min(bottom - min_h);
            }
            if matches!(
                direction,
                ResizeDirection::South | ResizeDirection::SouthWest | ResizeDirection::SouthEast
            ) {
                bottom = (bottom + dy).max(top + min_h);
            }
            if matches!(
                direction,
                ResizeDirection::West | ResizeDirection::NorthWest | ResizeDirection::SouthWest
            ) {
                left = (left + dx).min(right - min_w);
            }
            if matches!(
                direction,
                ResizeDirection::East | ResizeDirection::NorthEast | ResizeDirection::SouthEast
            ) {
                right = (right + dx).max(left + min_w);
            }
        }
    }

    unsafe {
        let _ = SetWindowPos(
            hwnd_of(hwnd),
            None,
            left,
            top,
            (right - left).max(1),
            (bottom - top).max(1),
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

/// 结束手势（Slint TouchArea 抬起事件驱动）；未开始的手势为空操作
pub fn end_window_gesture(hwnd: isize) {
    if let Ok(mut state) = GESTURE.lock() {
        if state.is_some() {
            tracing::debug!("end_window_gesture hwnd={hwnd}");
        }
        *state = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{is_alt_menu_syscommand, strip_system_menu_style, GestureMode};
    use windows::Win32::UI::WindowsAndMessaging::{
        SC_CLOSE, SC_KEYMENU, WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
    };

    #[test]
    fn strip_system_menu_style_removes_system_menu_related_flags() {
        let style = (WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0) as isize;
        let stripped = strip_system_menu_style(style);
        assert_ne!(stripped & WS_CAPTION.0 as isize, 0);
        assert_eq!(stripped & WS_SYSMENU.0 as isize, 0);
        assert_eq!(stripped & WS_MINIMIZEBOX.0 as isize, 0);
        assert_eq!(stripped & WS_MAXIMIZEBOX.0 as isize, 0);
    }

    #[test]
    fn alt_menu_syscommand_detection_only_matches_keymenu() {
        assert!(is_alt_menu_syscommand(SC_KEYMENU as usize));
        assert!(is_alt_menu_syscommand((SC_KEYMENU as usize) | 0x0001));
        assert!(!is_alt_menu_syscommand(SC_CLOSE as usize));
    }

    #[test]
    fn gesture_mode_is_copy_and_comparable() {
        let mode = GestureMode::Resize(super::ResizeDirection::SouthEast);
        assert_eq!(mode, mode);
        assert_ne!(GestureMode::Move, mode);
    }
}
