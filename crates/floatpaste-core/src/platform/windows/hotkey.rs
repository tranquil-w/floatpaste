//! 全局快捷键：`RegisterHotKey` + 独立消息循环线程。
//!
//! 与 GUI 快捷键插件（tauri-plugin-global-shortcut）无依赖关系，供原生壳
//! 直接使用。所有快捷键在同一线程注册并接收 `WM_HOTKEY`，触发时以快捷键
//! id 回调；回调在快捷键线程上执行，需自行转发到 UI 线程。含 Alt 的组合
//! 命中时先注入菜单屏蔽键再回调（见 `send_alt_menu_mask`）。

use std::sync::Mutex;
use std::thread::JoinHandle;

use tracing::{error, info, warn};
use windows::Win32::Foundation::{GetLastError, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, SendInput, UnregisterHotKey, HOT_KEY_MODIFIERS, INPUT, INPUT_KEYBOARD,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG,
    PM_NOREMOVE, WM_QUIT,
};

use crate::domain::error::AppError;

struct HotkeyThread {
    id: u32,
    handle: JoinHandle<()>,
}

static HOTKEY_THREAD: Mutex<Option<HotkeyThread>> = Mutex::new(None);

/// 持锁完成注销与回收后才能启动下一轮，避免旧线程与新注册争用组合键。
fn stop_thread(slot: &mut Option<HotkeyThread>) -> Result<(), AppError> {
    if let Some(thread) = slot.as_ref() {
        if !thread.handle.is_finished() {
            // 投递失败仅剩「线程恰好在此间隙退出」一种解释，join 才是退出真相
            let _ = unsafe { PostThreadMessageW(thread.id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
    }
    if let Some(thread) = slot.take() {
        if thread.handle.join().is_err() {
            return Err(AppError::Message("全局快捷键线程异常退出".to_string()));
        }
    }
    Ok(())
}

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

/// 菜单屏蔽键取未分配的虚拟键（AutoHotkey 同用途机制的默认选择），
/// 注入它不产生任何常用按键功能。
const ALT_MENU_MASK_VK: u16 = 0xE8;

/// 含 Alt 的全局快捷键命中后注入菜单屏蔽键。
///
/// `RegisterHotKey` 只吞掉组合键第二键的 keydown，Alt 的按下与抬起仍会
/// 送达前台窗口；当 Alt 比第二键先释放时，前台窗口只见 Alt 孤立的按下
/// 与抬起，其默认窗口过程据此次操作在 Alt 释放时激活菜单栏
/// （`SC_KEYMENU`）。补一对屏蔽键事件后，无论松键顺序，前台窗口都已在
/// Alt 释放前见过另一个按键的按下，不再识别成单按 Alt；正常单按 Alt、
/// Alt+Tab 与其他软件的菜单快捷键不经此路径，不受影响。前台为更高权限
/// 窗口时注入被系统丢弃，屏蔽不生效。
fn send_alt_menu_mask() {
    let inputs = alt_menu_mask_inputs();
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != 2 {
        warn!(sent, "Alt 菜单屏蔽键未完整注入，原软件可能仍进入菜单模式");
    }
}

fn alt_menu_mask_inputs() -> [INPUT; 2] {
    unsafe {
        let mut inputs: [INPUT; 2] = std::mem::zeroed();
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki.wVk = VIRTUAL_KEY(ALT_MENU_MASK_VK);
        inputs[0].Anonymous.ki.dwFlags = KEYBD_EVENT_FLAGS(0);

        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki.wVk = VIRTUAL_KEY(ALT_MENU_MASK_VK);
        inputs[1].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
        inputs
    }
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
/// 替换注册前同步回收旧线程；回调只做事件转发，不可在回调内注册或停止热键。
/// 至少一个 id 注册成功即进入消息循环；全部失败时线程直接退出并清理。
/// 线程启动失败/超时返回 `Err`。
pub fn register_hotkeys(
    specs: Vec<(u32, HotkeySpec)>,
    on_trigger: impl Fn(u32) + Send + 'static,
) -> Result<HotkeyOutcome, AppError> {
    let mut slot = HOTKEY_THREAD.lock()?;
    stop_thread(&mut slot)?;
    if specs.is_empty() {
        return Ok(HotkeyOutcome::default());
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let handle = std::thread::Builder::new().name("hotkey".into()).spawn(move || unsafe {
        // 确认消息队列存在后才报告就绪，首个停止/触发消息即可投递。
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
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

        let registered_count = outcome.registered.len();
        let failed_count = outcome.failed.len();
        let thread_id = GetCurrentThreadId();
        // 等待方超时/退出时也要注销，不能留下无人管理的热键线程。
        if ready_tx.send((thread_id, outcome)).is_ok() && registered_count > 0 {
            info!("全局快捷键已注册（{registered_count} 组，{failed_count} 组失败，线程 id={thread_id}）");
            loop {
                let result = GetMessageW(&mut message, None, 0, 0);
                if result.0 == -1 {
                    error!("全局快捷键消息循环失败，线程 id={thread_id}: {:?}", GetLastError());
                    break;
                }
                if result.0 == 0 {
                    break;
                }
                if message.message == WM_HOTKEY {
                    let id = message.wParam.0 as u32;
                    // 屏蔽键必须在回调前注入：列表准备与窗口显示都在回调之后，
                    // 等它们完成会错过 Alt 释放的时机
                    if specs.iter().any(|(spec_id, spec)| *spec_id == id && spec.alt) {
                        send_alt_menu_mask();
                    }
                    on_trigger(id);
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        for (id, _) in &specs {
            let _ = UnregisterHotKey(None, *id as i32);
        }
        info!("全局快捷键线程已退出，线程 id={thread_id}");
    })?;

    let (id, outcome) = ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| AppError::Message("等待全局快捷键初始化超时".to_string()))?;
    if outcome.registered.is_empty() {
        let _ = handle.join();
    } else {
        *slot = Some(HotkeyThread { id, handle });
    }
    Ok(outcome)
}

/// 等待快捷键线程退出并注销全部快捷键，幂等；返回后可立即注册相同组合。
pub fn stop_hotkeys() {
    let result = HOTKEY_THREAD
        .lock()
        .map_err(AppError::from)
        .and_then(|mut slot| stop_thread(&mut slot));
    if let Err(error) = result {
        error!("停止全局快捷键失败: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn alt_menu_mask_pairs_keydown_and_keyup_of_unassigned_key() {
        let inputs = alt_menu_mask_inputs();
        // 读取 INPUT 的联合体字段需要 unsafe；值来自本模块刚构造的载荷
        unsafe {
            assert_eq!(inputs[0].r#type, INPUT_KEYBOARD);
            assert_eq!(inputs[0].Anonymous.ki.wVk, VIRTUAL_KEY(ALT_MENU_MASK_VK));
            assert_eq!(inputs[0].Anonymous.ki.dwFlags, KEYBD_EVENT_FLAGS(0));
            assert_eq!(inputs[1].r#type, INPUT_KEYBOARD);
            assert_eq!(inputs[1].Anonymous.ki.wVk, VIRTUAL_KEY(ALT_MENU_MASK_VK));
            assert_eq!(inputs[1].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
        }
    }

    #[test]
    fn stop_releases_hotkeys_before_immediate_reregistration() {
        use std::time::Duration;

        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                stop_hotkeys();
            }
        }
        let _cleanup = Cleanup;
        // 组合不含 Alt：本测试直接投递 WM_HOTKEY，含 Alt 会让菜单屏蔽键
        // 向当前桌面注入真实按键
        let shortcut = parse_hotkey("Ctrl+Shift+F23").unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let outcome = register_hotkeys(vec![(1, shortcut)], move |_| {
            entered_tx.send(()).unwrap();
            // 暂停回调，让停止请求必然先于旧线程的注销，固定调度竞态。
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
        })
        .unwrap();
        assert_eq!(outcome.registered, vec![1]);
        let thread_id = HOTKEY_THREAD.lock().unwrap().as_ref().unwrap().id;
        unsafe { PostThreadMessageW(thread_id, WM_HOTKEY, WPARAM(1), LPARAM(0)) }.unwrap();
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let _ = release_tx.send(());
        });

        stop_hotkeys();
        let (trigger_tx, trigger_rx) = std::sync::mpsc::channel();
        let outcome = register_hotkeys(vec![(1, shortcut)], move |id| {
            let _ = trigger_tx.send(id);
        })
        .unwrap();
        release.join().unwrap();
        assert_eq!(outcome.failed, vec![], "停止返回后旧组合必须已经释放");
        assert_eq!(outcome.registered, vec![1]);
        let thread_id = HOTKEY_THREAD.lock().unwrap().as_ref().unwrap().id;
        unsafe { PostThreadMessageW(thread_id, WM_HOTKEY, WPARAM(1), LPARAM(0)) }.unwrap();
        assert_eq!(trigger_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        stop_hotkeys();
        stop_hotkeys();
    }
}
