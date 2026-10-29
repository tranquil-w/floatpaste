//! 速贴会话的旁路退出监控，只观察输入，不拦截或重放按键。
//!
//! 会话键仍由 RegisterHotKey 分发；Raw Input 观察其他按键，前台事件兜底任务切换。
//! 装卸均在窗口事件循环线程进行。复用并恢复进程已有的键盘 Raw Input 注册，
//! 避免覆盖 winit 的设备事件接收窗口；回调只投递退出操作，不同步销毁监控。

use std::cell::RefCell;
use std::mem::size_of;

use tracing::warn;
use windows::{
    core::Error,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
            Input::{
                GetRawInputData, GetRegisteredRawInputDevices, KeyboardAndMouse::GetAsyncKeyState,
                RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
                RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEKEYBOARD,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                GetForegroundWindow, EVENT_SYSTEM_FOREGROUND, RI_KEY_BREAK, RI_KEY_E0,
                WINEVENT_OUTOFCONTEXT, WM_INPUT,
            },
        },
    },
};

use super::session_keyboard::{parse_session_combo, SessionCombo, SessionKeyConfig};
use crate::domain::settings::UserSetting;

const SUBCLASS_ID: usize = 0x4650_0103;

struct Monitor {
    receiver: HWND,
    picker: HWND,
    foreground: HWND,
    previous: Option<RAWINPUTDEVICE>,
    foreground_hook: HWINEVENTHOOK,
    keys: KeyPolicy,
    dismissed: bool,
    callback: Box<dyn Fn()>,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        unsafe {
            if !self.foreground_hook.is_invalid() {
                let _ = UnhookWinEvent(self.foreground_hook);
            }
            let device = self.previous.unwrap_or(RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 6,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: HWND::default(),
            });
            if let Err(error) =
                RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32)
            {
                warn!(%error, "恢复速贴键盘 Raw Input 注册失败");
            }
            let _ = RemoveWindowSubclass(self.receiver, Some(input_proc), SUBCLASS_ID);
        }
    }
}

thread_local! {
    static MONITOR: RefCell<Option<Monitor>> = const { RefCell::new(None) };
}

/// 只在速贴实际显屏后调用；失败时回收已装配资源，保留原有会话键与鼠标路径。
pub fn begin_session(
    picker: isize,
    settings: &UserSetting,
    callback: Box<dyn Fn()>,
) -> Result<(), Error> {
    end_session();
    unsafe {
        let previous = registered_keyboard()?;
        let picker = HWND(picker as *mut _);
        let receiver = previous
            .map(|device| device.hwndTarget)
            .filter(|hwnd| !hwnd.is_invalid())
            .unwrap_or(picker);
        if !SetWindowSubclass(receiver, Some(input_proc), SUBCLASS_ID, 0).as_bool() {
            return Err(Error::from_thread());
        }
        let mut monitor = Monitor {
            receiver,
            picker,
            foreground: GetForegroundWindow(),
            previous,
            foreground_hook: HWINEVENTHOOK::default(),
            keys: KeyPolicy::new(settings),
            dismissed: false,
            callback,
        };
        // 原有注册的设备通知标志继续有效，不使用 NOLEGACY / NOHOTKEYS。
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 6,
            dwFlags: previous.map(|d| d.dwFlags).unwrap_or_default() | RIDEV_INPUTSINK,
            hwndTarget: receiver,
        };
        RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32)?;
        monitor.foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        if monitor.foreground_hook.is_invalid() {
            return Err(Error::from_thread());
        }
        MONITOR.with(|slot| *slot.borrow_mut() = Some(monitor));
    }
    Ok(())
}

pub fn end_session() {
    // 先释放借用，再卸载原生资源，避免窗口消息重入 RefCell。
    let monitor = MONITOR.with(|slot| slot.borrow_mut().take());
    drop(monitor);
}

/// 供窗口生命周期契约核对显屏装载与停屏释放。
pub fn is_session_active() -> bool {
    MONITOR.with(|slot| slot.borrow().is_some())
}

unsafe fn registered_keyboard() -> Result<Option<RAWINPUTDEVICE>, Error> {
    let mut count = 0;
    let size = size_of::<RAWINPUTDEVICE>() as u32;
    if GetRegisteredRawInputDevices(None, &mut count, size) == u32::MAX {
        return Err(Error::from_thread());
    }
    let mut devices = vec![RAWINPUTDEVICE::default(); count as usize];
    if count != 0 {
        let copied = GetRegisteredRawInputDevices(Some(devices.as_mut_ptr()), &mut count, size);
        if copied == u32::MAX {
            return Err(Error::from_thread());
        }
        devices.truncate(copied as usize);
    }
    Ok(devices
        .into_iter()
        .find(|d| d.usUsagePage == 1 && d.usUsage == 6))
}

unsafe extern "system" fn input_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_INPUT {
        let mut input = RAWINPUT::default();
        let mut size = size_of::<RAWINPUT>() as u32;
        let copied = GetRawInputData(
            HRAWINPUT(lparam.0 as *mut _),
            RID_INPUT,
            Some((&mut input as *mut RAWINPUT).cast()),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        );
        if copied != u32::MAX
            && copied >= size_of::<RAWINPUTHEADER>() as u32
            && input.header.dwType == RIM_TYPEKEYBOARD.0
        {
            let key = input.data.keyboard;
            MONITOR.with(|slot| {
                let mut slot = slot.borrow_mut();
                if let Some(monitor) = slot.as_mut().filter(|m| !m.dismissed) {
                    let vk = modifier_key(key.VKey, key.MakeCode, key.Flags);
                    if monitor
                        .keys
                        .should_dismiss(vk, key.Flags & RI_KEY_BREAK as u16 != 0)
                    {
                        monitor.dismissed = true;
                        (monitor.callback)();
                    }
                }
            });
        }
    }
    // 交还原接收窗口，也让 DefWindowProc 完成前台 WM_INPUT 的系统清理。
    DefSubclassProc(hwnd, message, wparam, lparam)
}

unsafe extern "system" fn foreground_proc(
    hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    MONITOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(monitor) = slot
            .as_mut()
            .filter(|m| !m.dismissed && m.foreground_hook == hook)
        {
            if !hwnd.is_invalid()
                && hwnd != monitor.foreground
                && hwnd != monitor.picker
                && hwnd == GetForegroundWindow()
            {
                monitor.dismissed = true;
                (monitor.callback)();
            }
        }
    });
}

/// Raw Input 的通用 Shift/Ctrl/Alt 虚拟键归一到左右键，避免双侧同时按住时误判。
fn modifier_key(vk: u16, scan: u16, flags: u16) -> u16 {
    match vk {
        0x10 => {
            if scan == 0x36 {
                0xA1
            } else {
                0xA0
            }
        }
        0x11 => {
            if flags & RI_KEY_E0 as u16 != 0 {
                0xA3
            } else {
                0xA2
            }
        }
        0x12 => {
            if flags & RI_KEY_E0 as u16 != 0 {
                0xA5
            } else {
                0xA4
            }
        }
        _ => vk,
    }
}

struct KeyPolicy {
    allowed: Vec<SessionCombo>,
    digits: bool,
    pressed: [bool; 256],
    win_used: bool,
}

impl KeyPolicy {
    fn new(settings: &UserSetting) -> Self {
        let config = SessionKeyConfig::from_settings(settings);
        let mut allowed: Vec<_> = config
            .bindings
            .into_iter()
            .map(|(_, combo)| combo)
            .collect();
        allowed.extend(parse_session_combo(&settings.shortcut));
        if settings.search_shortcut_enabled {
            allowed.extend(parse_session_combo(&settings.search_shortcut));
        }
        let mut pressed = [false; 256];
        for vk in [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C] {
            pressed[vk] = unsafe { GetAsyncKeyState(vk as i32) } < 0;
        }
        Self {
            allowed,
            digits: config.digit_shortcuts_enabled,
            pressed,
            win_used: true,
        }
    }

    fn should_dismiss(&mut self, vk: u16, released: bool) -> bool {
        if vk >= 255 || vk == 0 {
            return false;
        }
        let vk = vk as usize;
        let was_pressed = self.pressed[vk];
        self.pressed[vk] = !released;
        if matches!(vk, 0x5B | 0x5C) {
            if released {
                return was_pressed && !self.win_used;
            }
            if !was_pressed {
                self.win_used = false;
            }
            // Win 按下时保留面板，等待组合键；单按只在释放时退出。
            return false;
        }
        if released || matches!(vk, 0x10..=0x12 | 0xA0..=0xA5) {
            return false;
        }
        let shift = self.pressed[0xA0] || self.pressed[0xA1];
        let ctrl = self.pressed[0xA2] || self.pressed[0xA3];
        let alt = self.pressed[0xA4] || self.pressed[0xA5];
        let win = self.pressed[0x5B] || self.pressed[0x5C];
        if win {
            self.win_used = true;
        }
        let allowed =
            self.allowed.iter().any(|combo| {
                combo.virtual_key() == Some(vk as u32)
                    && (combo.ctrl, combo.alt, combo.shift, combo.win) == (ctrl, alt, shift, win)
            }) || (self.digits && (0x31..=0x39).contains(&vk) && !(ctrl || alt || shift || win));
        // 音量、媒体、亮度（设备可能使用无 VK 的 HID）不打断选择；浏览器键仍退出。
        !allowed && !(0xAD..=0xB7).contains(&vk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(settings: &UserSetting) -> KeyPolicy {
        let mut policy = KeyPolicy::new(settings);
        policy.pressed = [false; 256];
        policy
    }

    #[test]
    fn dismisses_single_win_only_on_release() {
        for vk in [0x5B, 0x5C] {
            let mut keys = policy(&UserSetting::default());
            assert!(!keys.should_dismiss(vk, false), "按下 Win 保持显示");
            assert!(!keys.should_dismiss(vk, false), "按住 Win 重复按下保持显示");
            assert!(keys.should_dismiss(vk, true), "松开单按 Win 才退出");
            assert!(!keys.should_dismiss(vk, true), "未按下的 Win 释放不退出");

            assert!(!keys.should_dismiss(vk, false));
            assert!(keys.should_dismiss(0x45, false), "Win+E 在 E 按下时退出");

            let mut keys = policy(&UserSetting::default());
            keys.pressed[vk as usize] = true;
            assert!(
                !keys.should_dismiss(vk, true),
                "呼出时已按住 Win 的释放不退出"
            );
        }
    }

    #[test]
    fn preserves_session_keys_but_dismisses_external_input() {
        let mut keys = policy(&UserSetting::default());
        for vk in [0x26, 0x28, 0x0D, 0x1B, 0x2E, 0x31, 0x39] {
            assert!(!keys.should_dismiss(vk, false), "会话键 {vk:x}");
            keys.should_dismiss(vk, true);
        }
        for vk in [0x41, 0x08, 0x25, 0x09] {
            assert!(keys.should_dismiss(vk, false), "外部输入 {vk:x}");
            keys.should_dismiss(vk, true);
        }
        assert!(!keys.should_dismiss(0xA2, false));
        assert!(keys.should_dismiss(0x43, false), "Ctrl+C 交还原应用");
        assert!(!keys.should_dismiss(0x44, false), "Ctrl+D 收藏");
        assert!(!keys.should_dismiss(0x0D, false), "Ctrl+Enter 次级上屏");
        assert!(keys.should_dismiss(0x31, false), "Ctrl+1 不是数字直达");
    }

    #[test]
    fn respects_custom_bindings_and_win_prefix_without_hiding_on_release() {
        let mut settings = UserSetting::default();
        settings.shortcut = "Win+F".into();
        settings.session_keys.toggle_favorite = "Win+D".into();
        settings.picker_digit_shortcuts_enabled = false;
        let mut keys = policy(&settings);
        assert!(keys.should_dismiss(0x31, false));
        assert!(!keys.should_dismiss(0x5B, false));
        assert!(!keys.should_dismiss(0x44, false));
        assert!(
            !keys.should_dismiss(0x5B, true),
            "执行会话键后松 Win 不退出"
        );
        assert!(!keys.should_dismiss(0x5B, false));
        assert!(keys.should_dismiss(0x5B, true), "单按 Win 仍退出");
        assert!(!keys.should_dismiss(0x5B, false));
        assert!(keys.should_dismiss(0x45, false), "Win+E 正常退出");
    }

    #[test]
    fn ignores_modifiers_media_and_opening_shortcut_release() {
        let mut keys = policy(&UserSetting::default());
        for vk in [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xAD, 0xAF, 0xB3, 255] {
            assert!(!keys.should_dismiss(vk, false));
            assert!(!keys.should_dismiss(vk, true));
        }
        assert!(!keys.should_dismiss(0xA4, false));
        assert!(!keys.should_dismiss(0xC0, false), "呼出键由原入口处理");
        assert!(!keys.should_dismiss(0xC0, true));
        assert!(!keys.should_dismiss(0xA4, true));
        assert_eq!(modifier_key(0x11, 0, RI_KEY_E0 as u16), 0xA3);
        assert_eq!(modifier_key(0x10, 0x36, 0), 0xA1);
        assert!(!keys.should_dismiss(0xA2, false));
        assert!(!keys.should_dismiss(0xA3, false));
        assert!(!keys.should_dismiss(0xA2, true));
        assert!(!keys.should_dismiss(0x44, false), "右 Ctrl 仍按住");
    }
}
