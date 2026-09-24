//! 速贴面板会话期键盘拦截：低级键盘钩子 + 长按导航连发。
//!
//! 原版行为（Tauri global-shortcut 会话热键）的等价复刻：
//! - 面板可见期间全局接管 ↑↓ / Enter / Shift+Enter / Esc / Ctrl+Space /
//!   Ctrl+Enter / 数字 1-9（可关），按键不再落到目标应用；
//! - ↑↓ 按住 280ms 后每 85ms 连发一次导航，松开立即停止
//!   （RegisterHotKey 拿不到松键事件，这里用 WH_KEYBOARD_LL 的
//!   keydown/keyup 精确复刻 press/release 语义）；
//! - 其余按键一律放行（主快捷键、搜索快捷键仍由 RegisterHotKey 线程处理）。
//!
//! 钩子常驻：WH_KEYBOARD_LL 只在首次会话时装载、进程退出随安装线程
//! 回收，会话结束只翻转 HOOK_ACTIVE（回调在关闭态对每个按键一次原子
//! 读即放行，无拦截开销）。不得在会话间 Unhook/重装：实测本进程中
//! LL 键盘钩子经卸载→重装循环后，第二次安装虽返回成功句柄，系统却
//! 静默不再调用——速贴每轮显隐/进出编辑器都装卸一次，一轮后快捷键
//! 即全灭（WH_MOUSE_LL 无此问题，实测可照常装卸）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use tracing::{error, warn};

use crate::domain::settings::UserSetting;

const NAV_REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(280);
const NAV_REPEAT_INTERVAL: Duration = Duration::from_millis(85);

/// 会话键盘动作（对齐原版 picker:// 事件族）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    NavigateUp,
    NavigateDown,
    Confirm,
    /// 次级上屏（Shift+Enter）：按条目类型粘贴为路径（图片→图片路径、
    /// 文件→路径列表文本）
    ConfirmAsPath,
    Dismiss,
    ToggleFavorite,
    OpenEditor,
    SelectIndex(u8),
}

/// 会话键组合：键名 + 修饰键。匹配要求「组合含有的修饰键按下、未含的
/// 抬起」（精确集合匹配），避免把目标应用自身的 Ctrl+Enter / Shift+Enter
/// 误吞，也与旧版 classify 的修饰键分支语义一致
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCombo {
    /// 规范键名（用户书写形式，匹配时忽略大小写），如 "Enter"、"K"
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

/// 解析 "Shift+Enter" / "Ctrl+Space" / "K" 形式的会话键描述；无法识别
/// 时返回 `None`。键名支持：A~Z、0~9、F1~F24、反引号、Space、Enter、
/// Escape/Esc、Tab、Backspace、Delete/Del、Insert、Home、End、
/// PageUp/PageDown、Up/Down/Left/Right（含 ArrowUp 等别名）、
/// Comma/Period/Slash。修饰键支持 Ctrl/Control、Alt、Shift、
/// Win/Windows/Super，大小写不敏感
pub fn parse_session_combo(text: &str) -> Option<SessionCombo> {
    let mut combo = SessionCombo {
        key: String::new(),
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
    };
    let mut key: Option<String> = None;

    for token in text.split('+') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return None;
        }
        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => combo.ctrl = true,
            "alt" => combo.alt = true,
            "shift" => combo.shift = true,
            "win" | "windows" | "super" => combo.win = true,
            _ => {
                if key.is_some() {
                    // 出现第二个非修饰键 token，视为非法描述
                    return None;
                }
                key = Some(trimmed.to_string());
            }
        }
    }

    let key = key?;
    virtual_key_for(&key.to_ascii_lowercase())?;
    combo.key = key;
    Some(combo)
}

impl SessionCombo {
    /// 虚拟键码（LL 钩子按虚拟键匹配）
    pub fn virtual_key(&self) -> Option<u32> {
        virtual_key_for(&self.key.to_ascii_lowercase())
    }

    /// 当前修饰键状态是否精确匹配组合（供 LL 钩子判定）：
    /// 组合含有的修饰键须按下，未含的须抬起
    fn modifiers_match(&self) -> bool {
        self.ctrl == modifier_down(VK_CONTROL)
            && self.alt == modifier_down(VK_ALT)
            && self.shift == modifier_down(VK_SHIFT)
            && self.win == (modifier_down(VK_LWIN) || modifier_down(VK_RWIN))
    }
}

/// 规范键名（小写）→ 虚拟键码
fn virtual_key_for(key_lower: &str) -> Option<u32> {
    let bytes = key_lower.as_bytes();
    match bytes {
        [b] if b.is_ascii_lowercase() => Some(u32::from(b.to_ascii_uppercase())),
        [b] if b.is_ascii_digit() => Some(u32::from(*b)),
        b"space" => Some(0x20),
        b"tab" => Some(0x09),
        b"enter" | b"return" => Some(0x0D),
        b"escape" | b"esc" => Some(0x1B),
        b"backspace" => Some(0x08),
        b"delete" | b"del" => Some(0x2E),
        b"insert" => Some(0x2D),
        b"home" => Some(0x24),
        b"end" => Some(0x23),
        b"pageup" => Some(0x21),
        b"pagedown" => Some(0x22),
        b"up" | b"arrowup" => Some(0x26),
        b"down" | b"arrowdown" => Some(0x28),
        b"left" | b"arrowleft" => Some(0x25),
        b"right" | b"arrowright" => Some(0x27),
        b"comma" => Some(0xBC),
        b"period" => Some(0xBE),
        b"slash" => Some(0xBF),
        b"`" | b"backquote" => Some(0xC0),
        [b'f', rest @ ..] if matches!(rest.len(), 1 | 2) && rest.iter().all(u8::is_ascii_digit) => {
            let number: u32 = std::str::from_utf8(rest).ok()?.parse().ok()?;
            (1..=24).contains(&number).then(|| 0x6F + number) // VK_F1 = 0x70
        }
        _ => None,
    }
}

/// 一次速贴会话的键位配置：数字直达开关 + 动作键绑定
#[derive(Debug, Clone)]
pub struct SessionKeyConfig {
    pub digit_shortcuts_enabled: bool,
    /// 匹配顺序即数组顺序（动作字段序），同组合先到先得
    pub bindings: Vec<(SessionAction, SessionCombo)>,
}

impl SessionKeyConfig {
    /// 从用户设置构建。删除条目键仅搜索窗口消费，不在 LL 钩子拦截范围
    pub fn from_settings(settings: &UserSetting) -> Self {
        let keys = &settings.session_keys;
        let bind = |text: &str, action: SessionAction| {
            parse_session_combo(text).map(|combo| (action, combo))
        };
        let bindings: Vec<(SessionAction, SessionCombo)> = [
            bind(&keys.navigate_up, SessionAction::NavigateUp),
            bind(&keys.navigate_down, SessionAction::NavigateDown),
            bind(&keys.confirm, SessionAction::Confirm),
            bind(&keys.confirm_as_file, SessionAction::ConfirmAsPath),
            bind(&keys.open_editor, SessionAction::OpenEditor),
            bind(&keys.toggle_favorite, SessionAction::ToggleFavorite),
            bind(&keys.dismiss, SessionAction::Dismiss),
        ]
        .into_iter()
        .flatten()
        .collect();
        Self {
            digit_shortcuts_enabled: settings.picker_digit_shortcuts_enabled,
            bindings,
        }
    }
}

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static NAV_REPEAT_DIRECTION: Mutex<Option<NavDirection>> = Mutex::new(None);
static NAV_REPEAT_TOKEN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavDirection {
    Up,
    Down,
}

static SESSION_CONFIG: Mutex<Option<SessionKeyConfig>> = Mutex::new(None);

/// 按键会话回调：在钩子线程上调用，须尽快返回（连发导航由独立线程投递）
pub type SessionKeyCallback = Box<dyn Fn(SessionAction) + Send + Sync>;

static SESSION_CALLBACK: Mutex<Option<SessionKeyCallback>> = Mutex::new(None);

/// 开始会话：装载 WH_KEYBOARD_LL（钩子常驻，重复调用只更新配置与回调）。
/// digit_shortcuts_enabled=false 时不拦截数字键
pub fn begin_session(config: SessionKeyConfig, callback: SessionKeyCallback) {
    if let Ok(mut slot) = SESSION_CONFIG.lock() {
        *slot = Some(config);
    }
    if let Ok(mut slot) = SESSION_CALLBACK.lock() {
        *slot = Some(callback);
    }
    stop_navigation_repeat();
    HOOK_ACTIVE.store(true, Ordering::SeqCst);
    HOOK_HANDLE.with(|handle| {
        if handle.borrow().is_some() {
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

/// 结束会话：停连发、清回调、关闭拦截开关。
/// 不卸载钩子（常驻语义见模块文档）；回调清空后即使钩子仍被调用，
/// 关闭态的早退分支也不触达回调
pub fn end_session() {
    HOOK_ACTIVE.store(false, Ordering::SeqCst);
    stop_navigation_repeat();
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

/// 判定虚拟键是否命中会话键：数字 1-9 直达（可关）外，其余按用户配置
/// 的动作键绑定精确匹配（键相同且修饰键集合一致）
fn classify(vk: u32) -> Option<SessionAction> {
    // 简化: 每键事件深拷贝配置（≤8 组合键）并在匹配时小写化键名，开销
    // 相对系统每击键自身处理可忽略；升级: 解析时预存虚拟键码 + Arc 共享
    let config = SESSION_CONFIG.lock().ok().and_then(|guard| guard.clone());

    if (0x31..=0x39).contains(&vk) {
        // 主行数字 1-9：无修饰时直达直贴；小键盘不拦截（对齐 Digit1-9 语义）
        let modifier_held = modifier_down(VK_CONTROL)
            || modifier_down(VK_ALT)
            || modifier_down(VK_SHIFT)
            || modifier_down(VK_LWIN)
            || modifier_down(VK_RWIN);
        if modifier_held {
            return None;
        }
        if config
            .as_ref()
            .is_some_and(|value| !value.digit_shortcuts_enabled)
        {
            return None;
        }
        return Some(SessionAction::SelectIndex((vk - 0x31 + 1) as u8));
    }

    let config = config?;
    config
        .bindings
        .iter()
        .find(|(_, combo)| combo.virtual_key() == Some(vk) && combo.modifiers_match())
        .map(|(action, _)| *action)
}

const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_ALT: u32 = 0x12;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

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
    use std::sync::atomic::Ordering;

    use super::{parse_session_combo, SessionKeyConfig};
    use crate::domain::settings::UserSetting;

    #[test]
    fn parses_session_combinations() {
        let combo = parse_session_combo("Shift+Enter").unwrap();
        assert_eq!(combo.key, "Enter");
        assert!(combo.shift && !combo.ctrl && !combo.alt && !combo.win);
        assert_eq!(combo.virtual_key(), Some(0x0D));

        let combo = parse_session_combo("ctrl+space").unwrap();
        assert_eq!(combo.key, "space");
        assert!(combo.ctrl);
        assert_eq!(combo.virtual_key(), Some(0x20));

        let combo = parse_session_combo("Escape").unwrap();
        assert_eq!(combo.virtual_key(), Some(0x1B));

        let combo = parse_session_combo("k").unwrap();
        assert_eq!(combo.virtual_key(), Some(0x4B));

        // Super 是 Win 的规范别名
        let combo = parse_session_combo("Super+K").unwrap();
        assert!(combo.win);
    }

    #[test]
    fn rejects_invalid_session_combinations() {
        assert_eq!(parse_session_combo(""), None);
        assert_eq!(parse_session_combo("Shift+"), None);
        assert_eq!(parse_session_combo("Ctrl+Alt+Q+X"), None);
        assert_eq!(parse_session_combo("不存在的键"), None);
    }

    #[test]
    fn config_from_settings_preserves_action_order_and_skips_invalid() {
        let settings = UserSetting {
            session_keys: crate::domain::settings::SessionKeys {
                navigate_up: "PageUp".to_string(),
                // 非法值：绑定被跳过，消费端不会命中
                dismiss: "不存在的键".to_string(),
                ..crate::domain::settings::SessionKeys::default()
            },
            ..UserSetting::default()
        };

        let config = SessionKeyConfig::from_settings(&settings);
        // 合法绑定 6 条（dismiss 非法被跳过）
        assert_eq!(config.bindings.len(), 6);
        assert_eq!(config.bindings[0].1.virtual_key(), Some(0x21)); // PageUp
        assert!(config.digit_shortcuts_enabled);
    }

    #[test]
    fn select_index_maps_digit_range() {
        // 分类逻辑依赖进程级修饰键状态，这里只验证会话开关语义
        assert!(!super::HOOK_ACTIVE.load(Ordering::SeqCst));
    }
}
