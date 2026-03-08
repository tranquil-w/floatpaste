use std::str::FromStr;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tracing::{error, info, warn};

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::{
            PICKER_CONFIRM_EVENT, PICKER_NAVIGATE_EVENT, PICKER_SELECT_INDEX_EVENT,
            SETTINGS_CHANGED_EVENT,
        },
    },
    services::window_coordinator::WindowCoordinator,
};

pub struct ShortcutManager;

const PICKER_SESSION_SHORTCUTS: [&str; 13] = [
    "Up", "Down", "Enter", "Escape", "1", "2", "3", "4", "5", "6", "7", "8", "9",
];

impl ShortcutManager {
    pub fn sync_registered_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), AppError> {
        let shortcut = normalize_shortcut(shortcut)?;
        if shortcut.is_empty() {
            return Err(AppError::Message("快捷键不能为空".to_string()));
        }

        let manager = app.global_shortcut();
        manager
            .unregister_all()
            .map_err(|error| AppError::Message(format!("清理旧快捷键失败: {error}")))?;
        manager
            .register(shortcut.as_str())
            .map_err(|error| AppError::Message(format!("注册快捷键失败: {error}")))?;

        if picker_is_visible(app) {
            Self::register_picker_session_shortcuts(app)?;
        }

        info!("已注册全局快捷键: {shortcut}");
        Ok(())
    }

    pub fn register_picker_session_shortcuts(app: &AppHandle) -> Result<(), AppError> {
        let manager = app.global_shortcut();
        Self::unregister_picker_session_shortcuts(app);
        for shortcut in PICKER_SESSION_SHORTCUTS {
            manager.register(shortcut).map_err(|error| {
                AppError::Message(format!("注册 Picker 会话快捷键失败: {shortcut}: {error}"))
            })?;
        }
        Ok(())
    }

    pub fn unregister_picker_session_shortcuts(app: &AppHandle) {
        let manager = app.global_shortcut();
        for shortcut in PICKER_SESSION_SHORTCUTS {
            let _ = manager.unregister(shortcut);
        }
    }

    pub fn handle_shortcut_event(app: &AppHandle, shortcut: String, event: &ShortcutEvent) {
        if event.state != ShortcutState::Pressed {
            return;
        }

        let Some(state) = app.try_state::<AppState>() else {
            warn!("快捷键事件触发时应用状态尚未就绪");
            return;
        };

        let normalized = shortcut.to_lowercase();
        let settings_shortcut = match state.current_settings() {
            Ok(settings) => match normalize_shortcut(&settings.shortcut) {
                Ok(value) => value,
                Err(error) => {
                    error!("规范化当前快捷键失败: {error}");
                    return;
                }
            },
            Err(error) => {
                error!("读取当前快捷键设置失败: {error}");
                return;
            }
        };

        if normalized == settings_shortcut {
            let result = if picker_is_visible(app) {
                WindowCoordinator::hide_picker_and_restore_target(app, &state)
            } else {
                WindowCoordinator::show_picker(app, &state)
            };

            if let Err(error) = result {
                error!("处理主快捷键失败: {error}");
            }
            return;
        }

        if !picker_is_visible(app) {
            return;
        }

        let emit_result = match normalized.as_str() {
            "up" | "arrowup" => app.emit(PICKER_NAVIGATE_EVENT, "up"),
            "down" | "arrowdown" => app.emit(PICKER_NAVIGATE_EVENT, "down"),
            "enter" => app.emit(PICKER_CONFIRM_EVENT, ()),
            "escape" | "esc" => {
                if let Err(error) = WindowCoordinator::hide_picker_and_restore_target(app, &state) {
                    error!("关闭 Picker 失败: {error}");
                }
                return;
            }
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                let index = normalized.parse::<usize>().unwrap_or(1) - 1;
                app.emit(PICKER_SELECT_INDEX_EVENT, index)
            }
            _ => return,
        };

        if let Err(error) = emit_result {
            error!("向 Picker 发送快捷键事件失败: {error}");
        }
    }

    pub fn update_from_settings(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        let settings = state.current_settings()?;
        Self::sync_registered_shortcut(app, &settings.shortcut)?;
        app.emit(SETTINGS_CHANGED_EVENT, &settings)
            .map_err(|error| AppError::Message(error.to_string()))?;
        Ok(())
    }
}

fn picker_is_visible(app: &AppHandle) -> bool {
    app.get_webview_window("picker")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

fn normalize_shortcut(shortcut: &str) -> Result<String, AppError> {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    Shortcut::from_str(trimmed)
        .map(|value| value.into_string().to_lowercase())
        .map_err(|error| AppError::Message(format!("无效快捷键格式: {error}")))
}
