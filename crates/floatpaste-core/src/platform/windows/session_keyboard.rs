//! 速贴面板会话期快捷键：RegisterHotKey（专用线程装载/泵送/注销）。
//!
//! 原版行为（Tauri global-shortcut 会话热键）的同源复刻：
//! - 面板可见期间全局接管 ↑↓ / Enter / Shift+Enter / Esc / Ctrl+Space /
//!   Ctrl+Enter / 数字 1-9（可关），命中即吞、不再落到目标应用；
//! - ↑↓ 注册时不带 MOD_NOREPEAT，按住连发走系统按键重复速率（对齐旧壳
//!   global-shortcut 行为）；其余键带 MOD_NOREPEAT，长按只触发一次；
//! - 会话热键全局生效，不依赖前台焦点——「编辑窗停屏后残留焦点吃掉
//!   按键」的形态随之消除。
//!
//! 历史教训：本模块曾用 WH_KEYBOARD_LL 复刻以获得 keydown/keyup 精确
//! 语义，但 LL 键盘钩子会被系统静默摘除（回调超时等诱因，不可观测、
//! 不可预防；实测卸载→重装、探针自愈重装后系统同样静默不调用），表现
//! 为「关闭编辑窗后会话快捷键全灭而鼠标路径正常」。会话快捷键回到
//! RegisterHotKey：装卸在本项目长期验证可靠（全局热键线程反复重注册
//! 从未失效），与全局热键共用同一套系统机制。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::thread;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostThreadMessageW, TranslateMessage, WM_HOTKEY, WM_QUIT,
};

use tracing::{info, warn};

use crate::domain::settings::UserSetting;

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
    /// 虚拟键码（热键注册按虚拟键匹配）
    pub fn virtual_key(&self) -> Option<u32> {
        virtual_key_for(&self.key.to_ascii_lowercase())
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
    /// 从用户设置构建。删除条目键仅搜索窗口消费，不在会话拦截范围
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

/// 会话热键线程 id：0=无会话，其余=线程 id（泵送 WM_HOTKEY）
static SESSION_THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// 会话代数：begin/end 各递增，供装载线程识别「注册完成前会话已被
/// 替换」的竞态，避免装卸交错导致热键泄漏
static SESSION_GEN: AtomicU32 = AtomicU32::new(0);
/// 会话热键 id 起始（避开 main.rs 全局热键的 1-3）
const SESSION_HOTKEY_ID_BASE: i32 = 64;

/// 按键会话回调：在会话热键线程上调用，须尽快返回
pub type SessionKeyCallback = Box<dyn Fn(SessionAction) + Send + Sync>;

static SESSION_CALLBACK: Mutex<Option<SessionKeyCallback>> = Mutex::new(None);

/// 开始会话：把会话组合键注册为全局热键（专用线程装载并泵送 WM_HOTKEY）。
/// 重复调用先结束上一会话（幂等）。全局热键命中与前台焦点无关
pub fn begin_session(config: SessionKeyConfig, callback: SessionKeyCallback) {
    end_session();
    if let Ok(mut slot) = SESSION_CALLBACK.lock() {
        *slot = Some(callback);
    }

    // 组合键展开为热键注册项：数字 1-9 直达（可关）+ 动作键绑定。
    // 导航键不带 MOD_NOREPEAT 以获得按住连发（系统重复速率，对齐旧壳），
    // 其余键带 MOD_NOREPEAT，长按只触发一次
    let mut hotkeys: Vec<(i32, HOT_KEY_MODIFIERS, u32, SessionAction)> = Vec::new();
    if config.digit_shortcuts_enabled {
        for digit in 0..9u32 {
            hotkeys.push((
                SESSION_HOTKEY_ID_BASE + hotkeys.len() as i32,
                HOT_KEY_MODIFIERS(0),
                0x31 + digit,
                SessionAction::SelectIndex(digit as u8 + 1),
            ));
        }
    }
    for (action, combo) in &config.bindings {
        let Some(vk) = combo.virtual_key() else {
            continue;
        };
        let mut modifiers = modifiers_mask(combo);
        if !matches!(action, SessionAction::NavigateUp | SessionAction::NavigateDown) {
            modifiers |= MOD_NOREPEAT;
        }
        hotkeys.push((
            SESSION_HOTKEY_ID_BASE + hotkeys.len() as i32,
            modifiers,
            vk,
            *action,
        ));
    }
    if hotkeys.is_empty() {
        warn!("会话快捷键配置为空，本次会话不拦截键盘");
        return;
    }

    let session_generation = SESSION_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || unsafe {
        let mut registered: Vec<(i32, SessionAction)> = Vec::new();
        for (id, modifiers, vk, action) in &hotkeys {
            match RegisterHotKey(None, *id, *modifiers, *vk) {
                Ok(()) => registered.push((*id, *action)),
                Err(reg_error) => {
                    let code = (reg_error.code().0 as u32) & 0xFFFF;
                    if code == 1409 {
                        warn!("会话快捷键 {action:?} 被其他程序占用，本次会话该键不生效");
                    } else {
                        warn!("会话快捷键 {action:?} 注册失败: {reg_error}");
                    }
                }
            }
        }
        if registered.is_empty() {
            warn!("会话快捷键全部注册失败，本次会话不拦截键盘");
            return;
        }
        let thread_id = GetCurrentThreadId();
        SESSION_THREAD_ID.store(thread_id, Ordering::SeqCst);
        info!(
            "会话快捷键已注册（{} 组，专用线程 id={thread_id}）",
            registered.len()
        );
        // 注册期间会话可能已被替换（快速开关面板），此时不进泵，
        // 立即注销退出，避免热键泄漏
        if SESSION_GEN.load(Ordering::SeqCst) != session_generation {
            for (id, _) in &registered {
                let _ = UnregisterHotKey(Some(HWND::default()), *id);
            }
            return;
        }

        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0);
            if result.0 <= 0 {
                break;
            }
            if message.message == WM_HOTKEY {
                let hotkey_id = message.wParam.0 as i32;
                if let Some(action) = registered
                    .iter()
                    .find(|(id, _)| *id == hotkey_id)
                    .map(|(_, action)| *action)
                {
                    if let Ok(slot) = SESSION_CALLBACK.lock() {
                        if let Some(callback) = slot.as_ref() {
                            callback(action);
                        }
                    }
                }
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        for (id, _) in &registered {
            let _ = UnregisterHotKey(Some(HWND::default()), *id);
        }
    });
}

/// 结束会话：注销全部会话热键并回收会话线程。幂等
pub fn end_session() {
    info!("键盘会话结束（拦截态关闭）");
    SESSION_GEN.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut slot) = SESSION_CALLBACK.lock() {
        *slot = None;
    }
    let thread_id = SESSION_THREAD_ID.swap(0, Ordering::SeqCst);
    if thread_id != 0 {
        unsafe {
            // 会话热键线程对 WM_QUIT 的响应点在消息泵，注册阶段的线程
            // 由代数检查自行注销退出
            let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// 组合键 → RegisterHotKey 修饰掩码（精确集合：未含的修饰键不置位）
fn modifiers_mask(combo: &SessionCombo) -> HOT_KEY_MODIFIERS {
    let mut mask = HOT_KEY_MODIFIERS(0);
    if combo.ctrl {
        mask |= MOD_CONTROL;
    }
    if combo.alt {
        mask |= MOD_ALT;
    }
    if combo.shift {
        mask |= MOD_SHIFT;
    }
    if combo.win {
        mask |= MOD_WIN;
    }
    mask
}

#[cfg(test)]
mod tests {
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
}
