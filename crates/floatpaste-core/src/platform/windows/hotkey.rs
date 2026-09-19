//! 全局快捷键：`RegisterHotKey` + 独立消息循环线程。
//!
//! 与 GUI 快捷键插件（tauri-plugin-global-shortcut）无依赖关系，供原生壳
//! 直接使用。所有快捷键在同一线程注册并接收 `WM_HOTKEY`，触发时以快捷键
//! id 回调；回调在快捷键线程上执行，需自行转发到 UI 线程。

use std::sync::atomic::{AtomicU32, Ordering};

use tracing::{error, info};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_QUIT,
};

use crate::domain::error::AppError;

static HOTKEY_THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// 一组全局快捷键的修饰键与虚拟键码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

/// 解析 "Ctrl+Alt+Q" 形式的快捷键描述；无法识别时返回 `None`。
///
/// 支持的键位：A~Z、0~9、F1~F24、反引号（\`）。修饰键支持
/// Ctrl/Control、Alt、Shift、Win/Windows/Super，大小写不敏感。
pub fn parse_hotkey(text: &str) -> Option<HotkeySpec> {
    let mut spec = HotkeySpec {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        vk: 0,
    };
    let mut key: Option<u32> = None;

    for token in text.split('+') {
        let token = token.trim();
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => spec.ctrl = true,
            "alt" => spec.alt = true,
            "shift" => spec.shift = true,
            "win" | "windows" | "super" => spec.win = true,
            _ => {
                if key.is_some() {
                    // 出现第二个非修饰键 token，视为非法描述
                    return None;
                }
                key = Some(parse_virtual_key(&lower)?);
            }
        }
    }

    spec.vk = key?;
    Some(spec)
}

/// 单个键名 → 虚拟键码。
fn parse_virtual_key(lower: &str) -> Option<u32> {
    let bytes = lower.as_bytes();
    match bytes {
        [b] if b.is_ascii_lowercase() => Some(u32::from(b.to_ascii_uppercase())),
        [b] if b.is_ascii_digit() => Some(u32::from(*b)),
        [b'f', rest @ ..] if matches!(rest.len(), 1 | 2) && rest.iter().all(u8::is_ascii_digit) => {
            let number: u32 = std::str::from_utf8(rest).ok()?.parse().ok()?;
            (1..=24).contains(&number).then(|| 0x6F + number) // VK_F1 = 0x70
        }
        [b'`'] => Some(0xC0), // VK_OEM_3
        _ => None,
    }
}

fn modifiers_mask(spec: &HotkeySpec) -> HOT_KEY_MODIFIERS {
    let mut mask = MOD_NOREPEAT;
    if spec.ctrl {
        mask |= MOD_CONTROL;
    }
    if spec.alt {
        mask |= MOD_ALT;
    }
    if spec.shift {
        mask |= MOD_SHIFT;
    }
    if spec.win {
        mask |= MOD_WIN;
    }
    mask
}

/// 一批快捷键的注册结果：`registered` 为已注册并监听触发的 id，
/// `failed` 为注册失败的 (id, Win32 错误码)（如 1409=组合已被占用）。
/// 部分失败不阻塞其余热键照常工作
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotkeyOutcome {
    pub registered: Vec<u32>,
    pub failed: Vec<(u32, u32)>,
}

/// 在专用线程上注册一批全局快捷键并进入消息循环。
///
/// `specs` 为 `(id, 描述)` 列表，触发时以 id 调用 `on_trigger`。
/// 至少一个 id 注册成功即进入消息循环；全部失败时线程直接退出并清理。
/// 线程启动失败/超时返回 `Err`。
pub fn register_hotkeys(
    specs: Vec<(u32, HotkeySpec)>,
    on_trigger: impl Fn(u32) + Send + 'static,
) -> Result<HotkeyOutcome, AppError> {
    if specs.is_empty() {
        return Ok(HotkeyOutcome::default());
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || unsafe {
        let mut outcome = HotkeyOutcome::default();
        for (id, spec) in &specs {
            match RegisterHotKey(None, *id as i32, modifiers_mask(spec), spec.vk) {
                Ok(()) => outcome.registered.push(*id),
                Err(error) => {
                    // Win32 错误码在 HRESULT 低 16 位（HRESULT_FROM_WIN32）
                    let code = (error.code().0 as u32) & 0xFFFF;
                    error!("注册全局快捷键 id={id} 失败: {error}");
                    outcome.failed.push((*id, code));
                }
            }
        }

        // 全部失败：无线程驻留必要，直接退出（无孤儿注册残留）
        let registered_count = outcome.registered.len();
        let failed_count = outcome.failed.len();
        if registered_count == 0 {
            let _ = ready_tx.send(outcome);
            return;
        }

        HOTKEY_THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
        let _ = ready_tx.send(outcome);
        info!("全局快捷键已注册（{registered_count} 组，{failed_count} 组失败）");

        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0);
            if result.0 <= 0 {
                break;
            }
            if message.message == WM_HOTKEY {
                // wParam 即注册时的 id
                on_trigger(message.wParam.0 as u32);
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        for (id, _) in &specs {
            let _ = UnregisterHotKey(Some(HWND::default()), *id as i32);
        }
        HOTKEY_THREAD_ID.store(0, Ordering::SeqCst);
    });

    let outcome = ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| AppError::Message("等待全局快捷键初始化超时".to_string()))?;
    Ok(outcome)
}

/// 请求快捷键线程退出并注销全部快捷键，幂等。
pub fn stop_hotkeys() {
    let thread_id = HOTKEY_THREAD_ID.swap(0, Ordering::SeqCst);
    if thread_id != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_hotkey, HotkeySpec};

    fn spec(ctrl: bool, alt: bool, shift: bool, win: bool, vk: u32) -> HotkeySpec {
        HotkeySpec {
            ctrl,
            alt,
            shift,
            win,
            vk,
        }
    }

    #[test]
    fn parses_modifier_and_key_combinations() {
        assert_eq!(
            parse_hotkey("Alt+Q"),
            Some(spec(false, true, false, false, b'Q' as u32))
        );
        assert_eq!(
            parse_hotkey("ctrl+shift+a"),
            Some(spec(true, false, true, false, b'A' as u32))
        );
        assert_eq!(
            parse_hotkey("Win+F"),
            Some(spec(false, false, false, true, b'F' as u32))
        );
        assert_eq!(
            parse_hotkey("Ctrl+`"),
            Some(spec(true, false, false, false, 0xC0))
        );
        assert_eq!(
            parse_hotkey("F5"),
            Some(spec(false, false, false, false, 0x74))
        );
        assert_eq!(
            parse_hotkey("Alt+1"),
            Some(spec(false, true, false, false, b'1' as u32))
        );
    }

    #[test]
    fn rejects_invalid_hotkey_text() {
        assert_eq!(parse_hotkey(""), None);
        assert_eq!(parse_hotkey("Alt"), None);
        assert_eq!(parse_hotkey("Ctrl+Alt"), None);
        assert_eq!(parse_hotkey("Alt+Q+X"), None);
        assert_eq!(parse_hotkey("Alt+键"), None);
    }
}
