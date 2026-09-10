use std::{thread, time::Duration};

use arboard::Clipboard;
use tauri::{AppHandle, Manager};

use crate::{
    app_bootstrap::AppState,
    domain::{
        clip_item::{PasteOption, PasteResult},
        error::AppError,
    },
    platform::windows::active_app::ActiveAppResolver,
    services::{
        paste_support, shortcut_manager::ShortcutManager, window_coordinator::WindowCoordinator,
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
        let previous_clipboard = paste_support::capture_snapshot_if_needed(&option)?;
        let mut clipboard =
            Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;

        // 写剪贴板要趁自身窗口仍可见时进行：OpenClipboard 需要一个存活窗口作为属主
        paste_support::write_item_to_clipboard(
            state,
            &mut clipboard,
            &detail,
            option.as_file,
            resolve_clipboard_owner_window(app),
        )?;

        let clip_type_label = paste_support::clip_type_label(&detail.r#type);

        // 资料库窗口调用时仅写入剪贴板，不恢复目标窗口、不发送 Ctrl+V
        if !option.paste_to_target {
            state.repository.mark_used(id)?;
            return Ok(PasteResult {
                success: true,
                code: format!("{}_clipboard_only", detail.r#type),
                message: format!("已将{clip_type_label}写入系统剪贴板，可手动粘贴到目标位置。"),
            });
        }

        let (target_hwnd, target_focus_hwnd) = if state.is_picker_active() {
            let session = state.picker_session()?;
            ShortcutManager::unregister_picker_session_shortcuts(app);
            WindowCoordinator::hide_picker(app)?;
            (session.target_window_hwnd, session.target_focus_hwnd)
        } else if state.is_search_active() {
            let hwnd = state
                .search_session()?
                .and_then(|session| session.target_window_hwnd);
            WindowCoordinator::hide_search_and_restore_target(app, state)?;
            (hwnd, None)
        } else {
            let session = state.picker_session()?;
            (session.target_window_hwnd, session.target_focus_hwnd)
        };

        let paste_result = if let Some(target_hwnd) = target_hwnd {
            thread::sleep(Duration::from_millis(90));
            if ActiveAppResolver::restore_foreground_window_with_focus(
                target_hwnd,
                target_focus_hwnd,
            ) {
                WindowCoordinator::resume_search_input_if_target(app, Some(target_hwnd));
                thread::sleep(Duration::from_millis(60));
                if paste_support::trigger_ctrl_v() {
                    PasteResult {
                        success: true,
                        code: format!("{}_paste_injected", detail.r#type),
                        message: format!("已将{clip_type_label}写入系统剪贴板，并回贴到目标窗口。"),
                    }
                } else {
                    PasteResult {
                        success: false,
                        code: format!("{}_paste_injection_failed", detail.r#type),
                        message: format!(
                            "已将{clip_type_label}写入系统剪贴板，但系统按键注入失败。你仍可手动执行 Ctrl+V。"
                        ),
                    }
                }
            } else {
                PasteResult {
                    success: false,
                    code: format!("{}_target_window_restore_failed", detail.r#type),
                    message: format!(
                        "已将{clip_type_label}写入系统剪贴板，但未能恢复到原目标窗口。你仍可手动执行 Ctrl+V。"
                    ),
                }
            }
        } else {
            PasteResult {
                success: false,
                code: format!("{}_target_window_missing", detail.r#type),
                message: format!(
                    "已将{clip_type_label}写入系统剪贴板，但当前没有可恢复的目标窗口句柄。你仍可手动执行 Ctrl+V。"
                ),
            }
        };

        if let Some(snapshot) = previous_clipboard {
            paste_support::schedule_clipboard_restore(
                state,
                snapshot,
                resolve_clipboard_owner_window(app),
            )?;
        }

        state.repository.mark_used(id)?;

        Ok(paste_result)
    }
}

fn resolve_clipboard_owner_window(app: &AppHandle) -> Option<isize> {
    for label in [
        crate::services::window_coordinator::PICKER_WINDOW_LABEL,
        crate::services::window_coordinator::SEARCH_WINDOW_LABEL,
        crate::services::window_coordinator::SETTINGS_WINDOW_LABEL,
        crate::services::window_coordinator::EDITOR_WINDOW_LABEL,
    ] {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        let Ok(hwnd) = window.hwnd() else {
            continue;
        };
        return Some(hwnd.0 as isize);
    }

    None
}
