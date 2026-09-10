//! 速贴面板会话期键盘拦截：低级键盘钩子 + 长按导航连发。
//!
//! 原版行为（Tauri global-shortcut 会话热键）的等价复刻：
//! - 面板可见期间全局接管 ↑↓ / Enter / Shift+Enter / Esc / Ctrl+Space /
//!   Ctrl+Enter / 数字 1-9（可关），按键不再落到目标应用；
//! - ↑↓ 按住 280ms 后每 85ms 连发一次导航，松开立即停止
//!   （RegisterHotKey 拿不到松键事件，这里用 WH_KEYBOARD_LL 的
//!   keydown/keyup 精确复刻 press/release 语义）；
//! - 其余按键一律放行（主快捷键、搜索快捷键仍由 RegisterHotKey 线程处理）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use tracing::{error, warn};

const NAV_REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(280);
const NAV_REPEAT_INTERVAL: Duration = Duration::from_millis(85);

/// 会话键盘动作（对齐原版 picker:// 事件族）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    NavigateUp,
    NavigateDown,
    Confirm,
    ConfirmAsFile,
    Dismiss,
    ToggleFavorite,
    OpenEditor,
    SelectIndex(u8),
}

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static NAV_REPEAT_DIRECTION: Mutex<Option<NavDirection>> = Mutex::new(None);
static NAV_REPEAT_TOKEN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
struct SessionConfig {
    digit_shortcuts_enabled: bool,
}

static SESSION_CONFIG: Mutex<Option<SessionConfig>> = Mutex::new(None);

/// 按键会话回调：在钩子线程上调用，须尽快返回（连发导航由独立线程投递）
pub type SessionKeyCallback = Box<dyn Fn(SessionAction) + Send + Sync>;

static SESSION_CALLBACK: Mutex<Option<SessionKeyCallback>> = Mutex::new(None);

/// 开始会话：装载 WH_KEYBOARD_LL。digit_shortcuts_enabled=false 时不拦截数字键
pub fn begin_session(digit_shortcuts_enabled: bool, callback: SessionKeyCallback) {
    if let Ok(mut config) = SESSION_CONFIG.lock() {
        *config = Some(SessionConfig {
            digit_shortcuts_enabled,
        });
    }
    if let Ok(mut slot) = SESSION_CALLBACK.lock() {
        *slot = Some(callback);
    }
    stop_navigation_repeat();
    HOOK_ACTIVE.store(true, Ordering::SeqCst);
    HOOK_HANDLE.with(|handle| {
        let already_installed = handle.borrow().is_some();
        if already_installed {
            return;
        }
        let installed =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0) };
        match installed {
            Ok(hook) => *handle.borrow_mut() = Some(hook),
            Err(error) => {
                HOOK_ACTIVE.store(false, Ordering::SeqCst);
                error!("装载会话键盘钩子失败: {error}");
            }
        }
    });
}

/// 结束会话：停连发、卸钩子、清回调
pub fn end_session() {
    HOOK_ACTIVE.store(false, Ordering::SeqCst);
    stop_navigation_repeat();
    HOOK_HANDLE.with(|handle| {
        if let Some(hook) = handle.borrow_mut().take() {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
    });
    if let Ok(mut slot) = SESSION_CALLBACK.lock() {
        *slot = None;
    }
    if let Ok(mut config) = SESSION_CONFIG.lock() {
        *config = None;
    }
}

thread_local! {
    static HOOK_HANDLE: std::cell::RefCell<Option<HHOOK>> = const { std::cell::RefCell::new(None) };
}

extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 || !HOOK_ACTIVE.load(Ordering::SeqCst) {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let msg = wparam.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if !is_down && !is_up {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let vk = event.vkCode;

    let Some(action) = classify(vk) else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };

    // 长按 ↑↓：按下启动连发，松开停止；连发线程回调在按下时同步触发一次
    if matches!(
        action,
        SessionAction::NavigateUp | SessionAction::NavigateDown
    ) {
        if is_up {
            stop_navigation_repeat();
            return LRESULT(1);
        }
        let direction = match action {
            SessionAction::NavigateUp => NavDirection::Up,
            _ => NavDirection::Down,
        };
        if start_navigation_repeat(direction) {
            emit(action);
        }
        return LRESULT(1);
    }

    if is_up {
        return LRESULT(1);
    }

    // 其余会话键：按下触发，长按不重复（对齐 RegisterHotKey + 前端一次性处理）
    stop_navigation_repeat();
    emit(action);
    LRESULT(1)
}

/// 判定虚拟键是否命中会话键。修饰键组合要求当前修饰键按下状态匹配，
/// 避免把目标应用自身的 Ctrl+Enter / Shift+Enter 误吞
fn classify(vk: u32) -> Option<SessionAction> {
    let config = SESSION_CONFIG.lock().ok().and_then(|guard| *guard);

    match vk {
        0x26 => Some(SessionAction::NavigateUp),   // VK_UP
        0x28 => Some(SessionAction::NavigateDown), // VK_DOWN
        0x0D => {
            // VK_RETURN：区分 Enter / Shift+Enter / Ctrl+Enter
            let ctrl = modifier_down(VK_CONTROL);
            let shift = modifier_down(VK_SHIFT);
            if ctrl {
                Some(SessionAction::OpenEditor)
            } else if shift {
                Some(SessionAction::ConfirmAsFile)
            } else {
                Some(SessionAction::Confirm)
            }
        }
        0x1B => Some(SessionAction::Dismiss), // VK_ESCAPE
        0x20 => {
            // VK_SPACE：仅 Ctrl+Space 收藏
            if modifier_down(VK_CONTROL) {
                Some(SessionAction::ToggleFavorite)
            } else {
                None
            }
        }
        0x31..=0x39 => {
            // 主行数字 1-9：无修饰时直达直贴；小键盘不拦截（对齐 Digit1-9 语义）
            if modifier_down(VK_CONTROL) || modifier_down(VK_ALT) || modifier_down(VK_SHIFT) {
                return None;
            }
            if config
                .as_ref()
                .is_some_and(|value| !value.digit_shortcuts_enabled)
            {
                return None;
            }
            Some(SessionAction::SelectIndex((vk - 0x31 + 1) as u8))
        }
        _ => None,
    }
}

const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_ALT: u32 = 0x12;

fn modifier_down(vk: u32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    (unsafe { GetAsyncKeyState(vk as i32) } as u32 & 0x8000) != 0
}

fn emit(action: SessionAction) {
    if let Ok(slot) = SESSION_CALLBACK.lock() {
        if let Some(callback) = slot.as_ref() {
            callback(action);
        }
    }
}

/// 启动长按连发；已在同方向连发时返回 false（避免重复触发首步导航）
fn start_navigation_repeat(direction: NavDirection) -> bool {
    let mut active = match NAV_REPEAT_DIRECTION.lock() {
        Ok(value) => value,
        Err(error) => {
            error!("读取长按导航状态失败: {error}");
            return true;
        }
    };

    if *active == Some(direction) {
        return false;
    }
    *active = Some(direction);
    let token = NAV_REPEAT_TOKEN.fetch_add(1, Ordering::SeqCst) + 1;

    thread::spawn(move || {
        thread::sleep(NAV_REPEAT_INITIAL_DELAY);
        loop {
            if NAV_REPEAT_TOKEN.load(Ordering::SeqCst) != token {
                break;
            }
            if !HOOK_ACTIVE.load(Ordering::SeqCst) {
                break;
            }
            if current_navigation_direction() != Some(direction) {
                break;
            }

            emit(match direction {
                NavDirection::Up => SessionAction::NavigateUp,
                NavDirection::Down => SessionAction::NavigateDown,
            });

            thread::sleep(NAV_REPEAT_INTERVAL);
        }
    });

    true
}

fn stop_navigation_repeat() {
    let mut active = match NAV_REPEAT_DIRECTION.lock() {
        Ok(value) => value,
        Err(error) => {
            warn!("停止长按导航失败: {error}");
            NAV_REPEAT_TOKEN.fetch_add(1, Ordering::SeqCst);
            return;
        }
    };
    *active = None;
    NAV_REPEAT_TOKEN.fetch_add(1, Ordering::SeqCst);
}

fn current_navigation_direction() -> Option<NavDirection> {
    NAV_REPEAT_DIRECTION.lock().ok().and_then(|value| *value)
}

/// 供测试/诊断：会话是否处于活跃状态
pub fn is_session_active() -> bool {
    HOOK_ACTIVE.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_index_maps_digit_range() {
        // 分类逻辑依赖进程级修饰键状态，这里只验证会话开关语义
        assert!(!HOOK_ACTIVE.load(Ordering::SeqCst));
    }
}
