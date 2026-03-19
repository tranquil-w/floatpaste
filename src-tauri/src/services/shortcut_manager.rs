use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tracing::{error, info, warn};

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::{
            PICKER_CONFIRM_EVENT, PICKER_NAVIGATE_EVENT, PICKER_SELECT_INDEX_EVENT,
            WORKBENCH_NAVIGATE_EVENT,
        },
    },
    services::window_coordinator::WindowCoordinator,
};

pub struct ShortcutManager;

const PICKER_SESSION_SHORTCUTS: [&str; 14] = [
    "Up", "Down", "Enter", "Escape", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6",
    "Digit7", "Digit8", "Digit9", "Tab",
];
const WORKBENCH_SESSION_SHORTCUTS: [&str; 8] = [
    "Up", "Down", "Enter", "Escape", "Ctrl+E", "Ctrl+F", "Ctrl+S", "Delete",
];
const PICKER_NAV_REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(280);
const PICKER_NAV_REPEAT_INTERVAL: Duration = Duration::from_millis(85);

static PICKER_NAV_REPEAT_DIRECTION: Mutex<Option<&str>> = Mutex::new(None);
static PICKER_NAV_REPEAT_TOKEN: AtomicU64 = AtomicU64::new(0);

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
        Self::stop_picker_navigation_repeat(None);
        let manager = app.global_shortcut();
        for shortcut in PICKER_SESSION_SHORTCUTS {
            let _ = manager.unregister(shortcut);
        }
    }

    pub fn register_workbench_session_shortcuts(app: &AppHandle) -> Result<(), AppError> {
        let manager = app.global_shortcut();
        Self::unregister_workbench_session_shortcuts(app);
        let mut failures = Vec::new();
        for shortcut in WORKBENCH_SESSION_SHORTCUTS {
            if let Err(error) = manager.register(shortcut) {
                failures.push(format!("{shortcut}: {error}"));
            }
        }

        if !failures.is_empty() {
            return Err(AppError::Message(format!(
                "注册 Workbench 会话快捷键失败: {}",
                failures.join("; ")
            )));
        }

        info!(
            "已注册 Workbench 会话快捷键: {}",
            WORKBENCH_SESSION_SHORTCUTS.join(", ")
        );
        Ok(())
    }

    pub fn unregister_workbench_session_shortcuts(app: &AppHandle) {
        let manager = app.global_shortcut();
        for shortcut in WORKBENCH_SESSION_SHORTCUTS {
            let _ = manager.unregister(shortcut);
        }
    }

    pub fn sync_registered_shortcuts(
        app: &AppHandle,
        main_shortcut: &str,
        workbench_shortcut: Option<&str>,
    ) -> Result<(), AppError> {
        let main_shortcut = normalize_shortcut(main_shortcut)?;
        if main_shortcut.is_empty() {
            return Err(AppError::Message("主快捷键不能为空".to_string()));
        }

        let manager = app.global_shortcut();
        manager
            .unregister_all()
            .map_err(|error| AppError::Message(format!("清理旧快捷键失败: {error}")))?;

        // 注册主快捷键
        manager
            .register(main_shortcut.as_str())
            .map_err(|error| AppError::Message(format!("注册主快捷键失败: {error}")))?;

        // 注册工作窗快捷键（如果启用）
        if let Some(workbench) = workbench_shortcut {
            let workbench = normalize_shortcut(workbench)?;
            if !workbench.is_empty() && workbench != main_shortcut {
                manager
                    .register(workbench.as_str())
                    .map_err(|error| AppError::Message(format!("注册工作窗快捷键失败: {error}")))?;
                info!("已注册工作窗快捷键: {workbench}");
            }
        }

        // 重新注册 Picker 会话快捷键
        if picker_is_active(app) || has_registered_picker_session_shortcut(app) {
            if let Err(error) = Self::register_picker_session_shortcuts(app) {
                warn!("重新注册 Picker 会话快捷键失败，将保留鼠标可用的降级路径: {error}");
            }
        }

        // 重新注册 Workbench 会话快捷键
        if workbench_is_active(app) || has_registered_workbench_session_shortcut(app) {
            if let Err(error) = Self::register_workbench_session_shortcuts(app) {
                warn!("重新注册 Workbench 会话快捷键失败，将保留鼠标可用的降级路径: {error}");
            }
        }

        info!(
            "已注册全局快捷键: 主={main_shortcut}, 工作窗={:?}",
            workbench_shortcut
        );
        Ok(())
    }

    pub fn handle_shortcut_event(app: &AppHandle, shortcut: String, event: &ShortcutEvent) {
        if event.state != ShortcutState::Pressed && event.state != ShortcutState::Released {
            return;
        }

        let Some(state) = app.try_state::<AppState>() else {
            warn!("快捷键事件触发时应用状态尚未就绪");
            return;
        };
        let state = state.inner().clone();

        let normalized = shortcut.to_lowercase();
        if let Some(direction) = picker_navigation_direction(normalized.as_str()) {
            if event.state == ShortcutState::Released {
                Self::stop_picker_navigation_repeat(Some(direction));
                return;
            }
        }

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
            if event.state != ShortcutState::Pressed {
                return;
            }

            info!("命中主快捷键: {normalized}");
            let is_active = state.is_picker_active();
            let app_handle = app.clone();
            let state_clone = state.clone();

            thread::spawn(move || {
                thread::sleep(Duration::from_millis(10));
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

        // 检查是否为工作窗快捷键
        let workbench_shortcut = state
            .current_settings()
            .ok()
            .filter(|s| s.workbench_shortcut_enabled)
            .map(|s| s.workbench_shortcut);

        if let Some(ref ws) = workbench_shortcut {
            if normalized == normalize_shortcut(ws).unwrap_or_default() {
                if event.state != ShortcutState::Pressed {
                    return;
                }

                info!("命中工作窗快捷键: {normalized}");
                let app_handle = app.clone();
                let state_clone = state.clone();
                if state.is_workbench_active() {
                    // 已打开则关闭
                    thread::spawn(move || {
                        let app_clone = app_handle.clone();
                        let _ = app_handle.run_on_main_thread(move || {
                            Self::unregister_workbench_session_shortcuts(&app_clone);
                            if let Err(error) =
                                WindowCoordinator::hide_workbench_and_restore_target(
                                    &app_clone,
                                    &state_clone,
                                )
                            {
                                error!("关闭 Workbench 失败: {error}");
                            }
                        });
                    });
                } else {
                    // 未打开则打开
                    thread::spawn(move || {
                        let app_clone = app_handle.clone();
                        let _ = app_handle.run_on_main_thread(move || {
                            if let Err(error) =
                                WindowCoordinator::open_workbench_global(&app_clone, &state_clone)
                            {
                                error!("打开 Workbench 失败: {error}");
                            } else if let Err(error) =
                                Self::register_workbench_session_shortcuts(&app_clone)
                            {
                                warn!("打开 Workbench 后注册会话快捷键失败: {error}");
                            }
                        });
                    });
                }
                return;
            }
        }

        // 处理 Workbench 会话内快捷键
        if state.is_workbench_active() {
            if event.state != ShortcutState::Pressed {
                return;
            }

            match normalized.as_str() {
                "up" | "arrowup" => {
                    if let Err(error) = app.emit(WORKBENCH_NAVIGATE_EVENT, "up") {
                        error!("向 Workbench 发送导航事件失败: {error}");
                    }
                }
                "down" | "arrowdown" => {
                    if let Err(error) = app.emit(WORKBENCH_NAVIGATE_EVENT, "down") {
                        error!("向 Workbench 发送导航事件失败: {error}");
                    }
                }
                "escape" | "esc" => {
                    info!("命中 Workbench 关闭快捷键: {normalized}");
                    let app_handle = app.clone();
                    let state_clone = state.clone();
                    thread::spawn(move || {
                        let app_clone = app_handle.clone();
                        let _ = app_handle.run_on_main_thread(move || {
                            Self::unregister_workbench_session_shortcuts(&app_clone);
                            if let Err(error) =
                                WindowCoordinator::hide_workbench_and_restore_target(
                                    &app_clone,
                                    &state_clone,
                                )
                            {
                                error!("关闭 Workbench 失败: {error}");
                            }
                        });
                    });
                }
                _ => {
                    // Enter、Ctrl+E、Ctrl+F、Ctrl+S、Delete 等事件由前端监听处理
                }
            }
            return;
        }

        if !state.is_workbench_active() && is_workbench_session_shortcut(normalized.as_str()) {
            if event.state == ShortcutState::Pressed {
                warn!("检测到 Workbench 已隐藏但会话快捷键仍在注册，正在自动释放这些快捷键");
                let app_handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    Self::unregister_workbench_session_shortcuts(&app_handle);
                });
            }
            return;
        }

        if !state.is_picker_active() {
            if event.state == ShortcutState::Pressed
                && is_picker_session_shortcut(normalized.as_str())
            {
                warn!("检测到 Picker 已隐藏但会话快捷键仍在注册，正在自动释放这些快捷键");
                let app_handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    Self::unregister_picker_session_shortcuts(&app_handle);
                });
            }
            return;
        }

        if let Some(direction) = picker_navigation_direction(normalized.as_str()) {
            if event.state != ShortcutState::Pressed {
                return;
            }

            if Self::start_picker_navigation_repeat(app, direction) {
                if let Err(error) = app.emit(PICKER_NAVIGATE_EVENT, direction) {
                    error!("向 Picker 发送快捷键事件失败: {error}");
                }
            }
            return;
        }

        if event.state != ShortcutState::Pressed {
            return;
        }

        Self::stop_picker_navigation_repeat(None);

        let emit_result = match normalized.as_str() {
            "enter" => app.emit(PICKER_CONFIRM_EVENT, ()),
            "escape" | "esc" => {
                info!("命中 Picker 关闭快捷键: {normalized}");
                let app_handle = app.clone();
                let state_clone = state.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(10));
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
                thread::spawn(move || {
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

    fn start_picker_navigation_repeat(app: &AppHandle, direction: &'static str) -> bool {
        let mut active_direction = match PICKER_NAV_REPEAT_DIRECTION.lock() {
            Ok(value) => value,
            Err(error) => {
                error!("读取 Picker 长按导航状态失败: {error}");
                return true;
            }
        };

        if *active_direction == Some(direction) {
            return false;
        }

        *active_direction = Some(direction);
        let token = PICKER_NAV_REPEAT_TOKEN.fetch_add(1, Ordering::SeqCst) + 1;
        let app_handle = app.clone();

        thread::spawn(move || {
            thread::sleep(PICKER_NAV_REPEAT_INITIAL_DELAY);

            loop {
                if PICKER_NAV_REPEAT_TOKEN.load(Ordering::SeqCst) != token {
                    break;
                }

                let is_active = app_handle
                    .try_state::<AppState>()
                    .map(|state| state.is_picker_active())
                    .unwrap_or(false);
                if !is_active || current_picker_navigation_direction() != Some(direction) {
                    break;
                }

                let app_clone = app_handle.clone();
                let _ = app_handle.run_on_main_thread(move || {
                    if app_clone
                        .try_state::<AppState>()
                        .map(|state| state.is_picker_active())
                        .unwrap_or(false)
                    {
                        let _ = app_clone.emit(PICKER_NAVIGATE_EVENT, direction);
                    }
                });

                thread::sleep(PICKER_NAV_REPEAT_INTERVAL);
            }
        });

        true
    }

    fn stop_picker_navigation_repeat(direction: Option<&'static str>) {
        let mut active_direction = match PICKER_NAV_REPEAT_DIRECTION.lock() {
            Ok(value) => value,
            Err(error) => {
                error!("停止 Picker 长按导航失败: {error}");
                PICKER_NAV_REPEAT_TOKEN.fetch_add(1, Ordering::SeqCst);
                return;
            }
        };

        if direction.is_some() && *active_direction != direction {
            return;
        }

        *active_direction = None;
        PICKER_NAV_REPEAT_TOKEN.fetch_add(1, Ordering::SeqCst);
    }
}

fn picker_is_active(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.is_picker_active())
        .unwrap_or(false)
}

fn workbench_is_active(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.is_workbench_active())
        .unwrap_or(false)
}

fn has_registered_workbench_session_shortcut(app: &AppHandle) -> bool {
    let manager = app.global_shortcut();
    WORKBENCH_SESSION_SHORTCUTS
        .iter()
        .any(|shortcut| manager.is_registered(*shortcut))
}

fn is_workbench_session_shortcut(shortcut: &str) -> bool {
    matches!(
        shortcut,
        "up" | "arrowup"
            | "down"
            | "arrowdown"
            | "enter"
            | "escape"
            | "esc"
            | "ctrl+e"
            | "ctrl+f"
            | "ctrl+s"
            | "delete"
    )
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

fn picker_navigation_direction(shortcut: &str) -> Option<&'static str> {
    match shortcut {
        "up" | "arrowup" => Some("up"),
        "down" | "arrowdown" => Some("down"),
        _ => None,
    }
}

fn current_picker_navigation_direction() -> Option<&'static str> {
    PICKER_NAV_REPEAT_DIRECTION
        .lock()
        .ok()
        .and_then(|value| *value)
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
