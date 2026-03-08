use std::{thread, time::Duration};

use arboard::Clipboard;
use tauri::AppHandle;
use tracing::warn;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

use crate::{
    app_bootstrap::AppState,
    domain::{
        clip_item::{PasteOption, PasteResult},
        error::AppError,
    },
    platform::windows::active_app::ActiveAppResolver,
    services::{
        normalize_service::NormalizeService, shortcut_manager::ShortcutManager,
        window_coordinator::WindowCoordinator,
    },
};

pub struct PasteExecutor;

impl PasteExecutor {
    pub fn paste_item(
        app: &AppHandle,
        state: &AppState,
        id: &str,
        option: PasteOption,
    ) -> Result<PasteResult, AppError> {
        let detail = state.repository.get_item_detail(id)?;
        let mut clipboard =
            Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
        let previous_text = clipboard.get_text().ok();

        if let Some(normalized) = NormalizeService::normalize_text(&detail.full_text, None) {
            state
                .self_write_guard()
                .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
        }

        clipboard
            .set_text(detail.full_text.clone())
            .map_err(|error| AppError::Clipboard(error.to_string()))?;

        ShortcutManager::unregister_picker_session_shortcuts(app);
        WindowCoordinator::hide_picker(app)?;

        let picker_session = state.picker_session()?;
        let paste_result = if let Some(target_hwnd) = picker_session.target_window_hwnd {
            thread::sleep(Duration::from_millis(90));
            if ActiveAppResolver::restore_foreground_window(target_hwnd) {
                thread::sleep(Duration::from_millis(60));
                if trigger_ctrl_v() {
                    PasteResult {
                        success: true,
                        code: "paste_injected".to_string(),
                        message: "已将内容回贴到目标窗口，并按设置安排恢复原始剪贴板。".to_string(),
                    }
                } else {
                    PasteResult {
                        success: false,
                        code: "paste_injection_failed".to_string(),
                        message: "已写入系统剪贴板，但系统按键注入失败。你仍可手动执行 Ctrl+V。"
                            .to_string(),
                    }
                }
            } else {
                PasteResult {
                    success: false,
                    code: "target_window_restore_failed".to_string(),
                    message: "已写入系统剪贴板，但未能恢复到原目标窗口。你仍可手动执行 Ctrl+V。"
                        .to_string(),
                }
            }
        } else {
            PasteResult {
                success: false,
                code: "target_window_missing".to_string(),
                message:
                    "已写入系统剪贴板，但当前没有可恢复的目标窗口句柄。你仍可手动执行 Ctrl+V。"
                        .to_string(),
            }
        };

        if option.restore_clipboard_after_paste {
            if let Some(snapshot) = previous_text {
                if let Some(normalized) = NormalizeService::normalize_text(&snapshot, None) {
                    state
                        .self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(550));
                    if let Ok(mut restore_clipboard) = Clipboard::new() {
                        let _ = restore_clipboard.set_text(snapshot);
                    }
                });
            }
        }

        state.repository.mark_used(id)?;

        Ok(paste_result)
    }
}

fn trigger_ctrl_v() -> bool {
    unsafe {
        let mut inputs: [INPUT; 4] = std::mem::zeroed();
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki.wVk = VK_CONTROL;
        inputs[0].Anonymous.ki.dwFlags = KEYBD_EVENT_FLAGS(0);

        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki.wVk = VK_V;
        inputs[1].Anonymous.ki.dwFlags = KEYBD_EVENT_FLAGS(0);

        inputs[2].r#type = INPUT_KEYBOARD;
        inputs[2].Anonymous.ki.wVk = VK_V;
        inputs[2].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        inputs[3].r#type = INPUT_KEYBOARD;
        inputs[3].Anonymous.ki.wVk = VK_CONTROL;
        inputs[3].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent != 4 {
            warn!("SendInput 只发送了 {sent} 个键盘事件");
        }
        sent == 4
    }
}
