use std::str::FromStr;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tracing::{error, info, warn};

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::{PICKER_CONFIRM_EVENT, PICKER_NAVIGATE_EVENT, PICKER_SELECT_INDEX_EVENT},
    },
    services::window_coordinator::WindowCoordinator,
};

pub struct ShortcutManager;

const PICKER_SESSION_SHORTCUTS: [&str; 14] = [
    "Up", "Down", "Enter", "Escape", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6",
    "Digit7", "Digit8", "Digit9", "Tab",
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

        if picker_is_active(app) || has_registered_picker_session_shortcut(app) {
            if let Err(error) = Self::register_picker_session_shortcuts(app) {
                warn!("重新注册 Picker 会话快捷键失败，将保留鼠标可用的降级路径: {error}");
            }
        }

        info!("已注册全局快捷键: {shortcut}");
        Ok(())
    }

    pub fn register_picker_session_shortcuts(app: &AppHandle) -> Result<(), AppError> {
        let manager = app.global_shortcut();
        Self::unregister_picker_session_shortcuts(app);
        let mut failures = Vec::new();
        for shortcut in PICKER_SESSION_SHORTCUTS {
            if let Err(error) = manager.register(shortcut) {
                failures.push(format!("{shortcut}: {error}"));
            }
        }

        if !failures.is_empty() {
            return Err(AppError::Message(format!(
                "注册 Picker 会话快捷键失败: {}",
                failures.join("; ")
            )));
        }

        info!(
            "已注册 Picker 会话快捷键: {}",
            PICKER_SESSION_SHORTCUTS.join(", ")
        );
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
        let state = state.inner().clone();

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
            info!("命中主快捷键: {normalized}");
            let is_active = state.is_picker_active();
            let app_handle = app.clone();
            let state_clone = state.clone();

            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(10));
                let app_clone = app_handle.clone();
                let _ = app_handle.run_on_main_thread(move || {
                    if is_active {
                        Self::unregister_picker_session_shortcuts(&app_clone);
                        if let Err(error) = WindowCoordinator::hide_picker_and_restore_target(
                            &app_clone,
                            &state_clone,
                        ) {
                            error!("关闭 Picker 失败: {error}");
                        }
                    } else {
                        if let Err(error) = WindowCoordinator::show_picker(&app_clone, &state_clone)
                        {
                            error!("显示 Picker 失败: {error}");
                        } else if let Err(error) =
                            Self::register_picker_session_shortcuts(&app_clone)
                        {
                            warn!("打开 Picker 后注册会话快捷键失败: {error}");
                        }
                    }
                });
            });
            return;
        }

        if !state.is_picker_active() {
            if is_picker_session_shortcut(normalized.as_str()) {
                warn!("检测到 Picker 已隐藏但会话快捷键仍在注册，正在自动释放这些快捷键");
                let app_handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    Self::unregister_picker_session_shortcuts(&app_handle);
                });
            }
            return;
        }

        let emit_result = match normalized.as_str() {
            "up" | "arrowup" => app.emit(PICKER_NAVIGATE_EVENT, "up"),
            "down" | "arrowdown" => app.emit(PICKER_NAVIGATE_EVENT, "down"),
            "enter" => app.emit(PICKER_CONFIRM_EVENT, ()),
            "escape" | "esc" => {
                info!("命中 Picker 关闭快捷键: {normalized}");
                let app_handle = app.clone();
                let state_clone = state.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let app_clone = app_handle.clone();
                    let _ = app_handle.run_on_main_thread(move || {
                        Self::unregister_picker_session_shortcuts(&app_clone);
                        if let Err(error) = WindowCoordinator::hide_picker_and_restore_target(
                            &app_clone,
                            &state_clone,
                        ) {
                            error!("关闭 Picker 失败: {error}");
                        }
                    });
                });
                return;
            }
            "tab" => {
                info!("命中 Tab 键切换到 Manager");
                let app_handle = app.clone();
                let state_clone = state.clone();
                std::thread::spawn(move || {
                    let app_clone = app_handle.clone();
                    let _ = app_handle.run_on_main_thread(move || {
                        Self::unregister_picker_session_shortcuts(&app_clone);
                        if let Err(error) = WindowCoordinator::hide_picker_and_open_manager(
                            &app_clone,
                            &state_clone,
                        ) {
                            error!("从 Picker 切换到 Manager 失败: {error}");
                        }
                    });
                });
                return;
            }
            "digit1" => app.emit(PICKER_SELECT_INDEX_EVENT, 0),
            "digit2" => app.emit(PICKER_SELECT_INDEX_EVENT, 1),
            "digit3" => app.emit(PICKER_SELECT_INDEX_EVENT, 2),
            "digit4" => app.emit(PICKER_SELECT_INDEX_EVENT, 3),
            "digit5" => app.emit(PICKER_SELECT_INDEX_EVENT, 4),
            "digit6" => app.emit(PICKER_SELECT_INDEX_EVENT, 5),
            "digit7" => app.emit(PICKER_SELECT_INDEX_EVENT, 6),
            "digit8" => app.emit(PICKER_SELECT_INDEX_EVENT, 7),
            "digit9" => app.emit(PICKER_SELECT_INDEX_EVENT, 8),
            _ => return,
        };

        if let Err(error) = emit_result {
            error!("向 Picker 发送快捷键事件失败: {error}");
        }
    }
}

fn picker_is_active(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.is_picker_active())
        .unwrap_or(false)
}

fn has_registered_picker_session_shortcut(app: &AppHandle) -> bool {
    let manager = app.global_shortcut();
    PICKER_SESSION_SHORTCUTS
        .iter()
        .any(|shortcut| manager.is_registered(*shortcut))
}

fn is_picker_session_shortcut(shortcut: &str) -> bool {
    matches!(
        shortcut,
        "up" | "arrowup"
            | "down"
            | "arrowdown"
            | "enter"
            | "escape"
            | "esc"
            | "digit1"
            | "digit2"
            | "digit3"
            | "digit4"
            | "digit5"
            | "digit6"
            | "digit7"
            | "digit8"
            | "digit9"
            | "tab"
    )
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
