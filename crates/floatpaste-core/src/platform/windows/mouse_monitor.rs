//! 速贴面板会话期低级鼠标钩子：点击面板窗口外任意位置即关闭面板。
//!
//! 对齐原版 `picker_mouse_monitor.rs`：会话期间装载 WH_MOUSE_LL，
//! 左/右/中键（含非客户区）按下时若命中点在面板窗口矩形之外，
//! 触发回调（由调用方关闭面板；是否还原前台由调用方决定，外击场景
//! 用户点击处已取得焦点，不再还原），事件本身放行。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetWindowRect, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCLBUTTONDOWN, WM_NCMBUTTONDOWN,
    WM_NCRBUTTONDOWN, WM_RBUTTONDOWN,
};

use tracing::error;

static MONITOR_WINDOW: AtomicIsize = AtomicIsize::new(0);
static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);

pub type OutsideClickCallback = Box<dyn Fn() + Send + Sync>;
static OUTSIDE_CLICK_CALLBACK: std::sync::Mutex<Option<OutsideClickCallback>> =
    std::sync::Mutex::new(None);

thread_local! {
    static HOOK_HANDLE: std::cell::RefCell<Option<HHOOK>> = const { std::cell::RefCell::new(None) };
}

/// 开始监听：picker_hwnd 为面板窗口句柄，外击时调用 callback（钩子线程上触发）
pub fn begin_session(picker_hwnd: isize, callback: OutsideClickCallback) {
    MONITOR_WINDOW.store(picker_hwnd, Ordering::SeqCst);
    if let Ok(mut slot) = OUTSIDE_CLICK_CALLBACK.lock() {
        *slot = Some(callback);
    }
    HOOK_ACTIVE.store(true, Ordering::SeqCst);

    HOOK_HANDLE.with(|handle| {
        if handle.borrow().is_some() {
            return;
        }
        let installed = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0) };
        match installed {
            Ok(hook) => *handle.borrow_mut() = Some(hook),
            Err(err) => {
                HOOK_ACTIVE.store(false, Ordering::SeqCst);
                error!("设置 WH_MOUSE_LL 钩子失败: {}", err);
            }
        }
    });
}

/// 结束监听（幂等）
pub fn end_session() {
    HOOK_ACTIVE.store(false, Ordering::SeqCst);
    MONITOR_WINDOW.store(0, Ordering::SeqCst);
    HOOK_HANDLE.with(|handle| {
        if let Some(hook) = handle.borrow_mut().take() {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
    });
    if let Ok(mut slot) = OUTSIDE_CLICK_CALLBACK.lock() {
        *slot = None;
    }
}

extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && HOOK_ACTIVE.load(Ordering::SeqCst) {
        let msg = wparam.0 as u32;
        if msg == WM_LBUTTONDOWN
            || msg == WM_RBUTTONDOWN
            || msg == WM_MBUTTONDOWN
            || msg == WM_NCLBUTTONDOWN
            || msg == WM_NCRBUTTONDOWN
            || msg == WM_NCMBUTTONDOWN
        {
            if click_is_outside_picker(lparam) {
                if let Ok(slot) = OUTSIDE_CLICK_CALLBACK.lock() {
                    if let Some(callback) = slot.as_ref() {
                        callback();
                    }
                }
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn click_is_outside_picker(lparam: LPARAM) -> bool {
    let hwnd_value = MONITOR_WINDOW.load(Ordering::SeqCst);
    if hwnd_value == 0 {
        return false;
    }
    let hwnd = HWND(hwnd_value as *mut _);

    let mut rect = RECT::default();
    if !unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
        return false;
    }

    let hook_struct = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    let pt: &POINT = &hook_struct.pt;
    pt.x < rect.left || pt.x > rect.right || pt.y < rect.top || pt.y > rect.bottom
}

/// 供测试查询监听是否装载
pub fn is_session_active() -> bool {
    HOOK_ACTIVE.load(Ordering::SeqCst)
}

// 回调直接在钩子线程同步触发（原版是 spawn 线程处理），
// 关闭操作经由调用方投递到事件循环，钩子立即返回，不阻塞输入链路。
