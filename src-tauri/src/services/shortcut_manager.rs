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
            PICKER_CONFIRM_AS_FILE_EVENT, PICKER_CONFIRM_EVENT, PICKER_FAVORITE_EVENT,
            PICKER_NAVIGATE_EVENT, PICKER_OPEN_EDITOR_EVENT, PICKER_SELECT_INDEX_EVENT,
        },
        settings::normalize_shortcut_for_registration,
    },
    services::window_coordinator::{WindowCoordinator, SEARCH_WINDOW_LABEL},
};

pub struct ShortcutManager;

const PICKER_SESSION_SHORTCUTS: [&str; 16] = [
    "Up",
    "Down",
    "Enter",
    "Shift+Enter",
    "Escape",
    "Ctrl+Space",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    "Ctrl+Enter",
];
const SHORTCUT_CALLBACK_DEFER_DELAY: Duration = Duration::from_millis(10);
const PICKER_NAV_REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(280);
const PICKER_NAV_REPEAT_INTERVAL: Duration = Duration::from_millis(85);

static PICKER_NAV_REPEAT_DIRECTION: Mutex<Option<&str>> = Mutex::new(None);
static PICKER_NAV_REPEAT_TOKEN: AtomicU64 = AtomicU64::new(0);

impl ShortcutManager {
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
            set_picker_shortcuts_registered(app, false);
            return Err(AppError::Message(format!(
                "注册 Picker 会话快捷键失败: {}",
                failures.join("; ")
            )));
        }

        set_picker_shortcuts_registered(app, true);
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
        set_picker_shortcuts_registered(app, false);
    }

    pub fn sync_registered_shortcuts(
        app: &AppHandle,
        main_shortcut: &str,
        search_shortcut: Option<&str>,
    ) -> Result<(), AppError> {
        let main_shortcut = normalize_shortcut(main_shortcut)?;
        if main_shortcut.is_empty() {
            return Err(AppError::Message("主快捷键不能为空".to_string()));
        }

        let manager = app.global_shortcut();
        manager
            .unregister_all()
            .map_err(|error| AppError::Message(format!("清理旧快捷键失败: {error}")))?;

        manager
            .register(main_shortcut.as_str())
            .map_err(|error| AppError::Message(format!("注册主快捷键失败: {error}")))?;

        if let Some(search) = search_shortcut {
            let search = normalize_shortcut(search)?;
            if !search.is_empty() && search != main_shortcut {
                manager
                    .register(search.as_str())
                    .map_err(|error| AppError::Message(format!("注册搜索快捷键失败: {error}")))?;
                info!("已注册搜索快捷键: {search}");
            }
        }

        if picker_is_active(app) || has_registered_picker_session_shortcut(app) {
            if let Err(error) = Self::register_picker_session_shortcuts(app) {
                warn!("重新注册 Picker 会话快捷键失败，将保留鼠标可用的降级路径: {error}");
            }
        }


        info!(
            "已注册全局快捷键: 主={main_shortcut}, 搜索={:?}",
            search_shortcut
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

        let settings = match state.current_settings() {
            Ok(settings) => settings,
            Err(error) => {
                error!("读取当前快捷键设置失败: {error}");
                return;
            }
        };

        let settings_shortcut = match normalize_shortcut(&settings.shortcut) {
            Ok(value) => value,
            Err(error) => {
                error!("规范化当前快捷键失败: {error}");
                return;
            }
        };
        let normalized_search_shortcut =
            normalize_enabled_search_shortcut(settings.search_shortcut_enabled, &settings.search_shortcut);

        if normalized == settings_shortcut {
            if event.state != ShortcutState::Pressed {
                return;
            }

            info!("命中主快捷键: {normalized}");
            let is_active = state.is_picker_active();
            let app_handle = app.clone();
            let state_clone = state.clone();

            thread::spawn(move || {
                thread::sleep(SHORTCUT_CALLBACK_DEFER_DELAY);
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
                        // 如果搜索窗口活跃，先隐藏它以避免焦点竞争导致闪烁
                        if state_clone.is_search_active() {
                            if let Err(error) = WindowCoordinator::hide_search_without_restore_target(
                                &app_clone,
                                &state_clone,
                            ) {
                                error!("隐藏搜索窗口失败: {error}");
                            }
                        }
                        if let Err(error) =
                            WindowCoordinator::show_picker(&app_clone, &state_clone)
                        {
                            error!("显示 Picker 失败: {error}");
                        } else if let Err(error) = Self::register_picker_session_shortcuts(&app_clone) {
                            warn!("打开 Picker 后注册会话快捷键失败: {error}");
                        }
                    }
                });
            });
            return;
        }

        if state.is_picker_active() {
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

            // 在 picker 活跃状态下也检查搜索快捷键
            if normalized_search_shortcut.as_deref() == Some(normalized.as_str()) {
                info!("在 Picker 活跃时命中搜索快捷键: {normalized}");
                let app_handle = app.clone();
                let state_clone = state.clone();
                defer_shortcut_main_thread_action(app_handle, move |app_clone| {
                    // 只隐藏 picker，不恢复目标窗口焦点（焦点将交给 search）
                    if let Err(error) = WindowCoordinator::hide_picker(&app_clone) {
                        error!("关闭 Picker 失败: {error}");
                    }
                    if let Err(error) =
                        WindowCoordinator::open_search_global(&app_clone, &state_clone)
                    {
                        error!("打开 Search 失败: {error}");
                    }
                });
                return;
            }

            let emit_result = match normalized.as_str() {
                "enter" => app.emit(PICKER_CONFIRM_EVENT, ()),
                "shift+enter" => app.emit(PICKER_CONFIRM_AS_FILE_EVENT, ()),
                "escape" | "esc" => {
                    info!("命中 Picker 关闭快捷键: {normalized}");
                    let app_handle = app.clone();
                    let state_clone = state.clone();
                    thread::spawn(move || {
                        thread::sleep(SHORTCUT_CALLBACK_DEFER_DELAY);
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
                "digit1" => app.emit(PICKER_SELECT_INDEX_EVENT, 0),
                "digit2" => app.emit(PICKER_SELECT_INDEX_EVENT, 1),
                "digit3" => app.emit(PICKER_SELECT_INDEX_EVENT, 2),
                "digit4" => app.emit(PICKER_SELECT_INDEX_EVENT, 3),
                "digit5" => app.emit(PICKER_SELECT_INDEX_EVENT, 4),
                "digit6" => app.emit(PICKER_SELECT_INDEX_EVENT, 5),
                "digit7" => app.emit(PICKER_SELECT_INDEX_EVENT, 6),
                "digit8" => app.emit(PICKER_SELECT_INDEX_EVENT, 7),
                "digit9" => app.emit(PICKER_SELECT_INDEX_EVENT, 8),
                "ctrl+enter" | "control+enter" => app.emit(PICKER_OPEN_EDITOR_EVENT, ()),
                "ctrl+space" | "control+space" => app.emit(PICKER_FAVORITE_EVENT, ()),
                _ => return,
            };

            if let Err(error) = emit_result {
                error!("向 Picker 发送快捷键事件失败: {error}");
            }
            return;
        }

        if normalized_search_shortcut.as_deref() == Some(normalized.as_str()) {
            if event.state != ShortcutState::Pressed {
                return;
            }

            let should_toggle = should_toggle_active_search_window(app, &state);
            info!("命中搜索快捷键: {normalized}");
            let app_handle = app.clone();
            let state_clone = state.clone();

            if should_toggle {
                defer_shortcut_main_thread_action(app_handle, move |app_clone| {
                    if let Err(error) = WindowCoordinator::hide_search_and_restore_target(
                        &app_clone,
                        &state_clone,
                    ) {
                        error!("关闭 Search 失败: {error}");
                    }
                });
            } else {
                defer_shortcut_main_thread_action(app_handle, move |app_clone| {
                    if let Err(error) =
                        WindowCoordinator::open_search_global(&app_clone, &state_clone)
                    {
                        error!("打开 Search 失败: {error}");
                    }
                });
            }
            return;
        }

        if event.state == ShortcutState::Pressed
            && should_release_stale_picker_shortcuts(
                state.is_picker_active(),
                has_registered_picker_session_shortcut(app),
                normalized.as_str(),
            )
        {
            warn!("检测到 Picker 已隐藏但会话快捷键仍在注册，正在自动释放这些快捷键");
            let app_handle = app.clone();
            defer_shortcut_main_thread_action(app_handle, move |app_handle| {
                Self::unregister_picker_session_shortcuts(&app_handle);
            });
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

fn should_toggle_active_search_window(app: &AppHandle, state: &AppState) -> bool {
    if !state.is_search_active() {
        return false;
    }

    let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) else {
        return false;
    };

    let is_visible = window.is_visible().unwrap_or(false);
    #[cfg(target_os = "windows")]
    let is_minimized =
        crate::platform::windows::window_utils::is_window_minimized(&window).unwrap_or(false);

    #[cfg(not(target_os = "windows"))]
    let is_minimized = window.is_minimized().unwrap_or(false);

    is_visible && !is_minimized
}
fn picker_is_active(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.is_picker_active())
        .unwrap_or(false)
}

#[cfg(test)]
fn is_search_session_shortcut(shortcut: &str) -> bool {
    let _ = shortcut;
    false
}

fn has_registered_picker_session_shortcut(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.picker_session_shortcuts_registered())
        .unwrap_or(false)
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
            | "ctrl+space"
            | "control+space"
            | "digit1"
            | "digit2"
            | "digit3"
            | "digit4"
            | "digit5"
            | "digit6"
            | "digit7"
            | "digit8"
            | "digit9"
            | "ctrl+enter"
            | "control+enter"
    )
}

fn should_release_stale_picker_shortcuts(
    picker_active: bool,
    shortcuts_registered: bool,
    shortcut: &str,
) -> bool {
    !picker_active && shortcuts_registered && is_picker_session_shortcut(shortcut)
}

fn defer_shortcut_main_thread_action<F>(app: AppHandle, action: F)
where
    F: FnOnce(AppHandle) + Send + 'static,
{
    thread::spawn(move || {
        thread::sleep(SHORTCUT_CALLBACK_DEFER_DELAY);
        let app_clone = app.clone();
        let _ = app.run_on_main_thread(move || action(app_clone));
    });
}

fn set_picker_shortcuts_registered(app: &AppHandle, registered: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        state.set_picker_session_shortcuts_registered(registered);
    }
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

    let registerable = normalize_shortcut_for_registration(trimmed);

    Shortcut::from_str(registerable.as_str())
        .map(|value| value.into_string().to_lowercase())
        .map_err(|error| AppError::Message(format!("无效快捷键格式: {error}")))
}

fn normalize_enabled_search_shortcut(enabled: bool, shortcut: &str) -> Option<String> {
    if !enabled {
        return None;
    }

    match normalize_shortcut(shortcut) {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) => None,
        Err(error) => {
            warn!("搜索快捷键 '{shortcut}' 解析失败，跳过匹配: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_picker_session_shortcut, is_search_session_shortcut,
        normalize_enabled_search_shortcut, normalize_shortcut,
        should_release_stale_picker_shortcuts,
    };

    #[test]
    fn search_session_shortcuts_should_not_register_navigation_or_confirm_keys() {
        assert!(!is_search_session_shortcut("up"));
        assert!(!is_search_session_shortcut("arrowdown"));
        assert!(!is_search_session_shortcut("enter"));
        assert!(!is_search_session_shortcut("escape"));
        assert!(!is_search_session_shortcut("control+enter"));
    }

    #[test]
    fn picker_hidden_without_registered_shortcuts_should_not_attempt_cleanup() {
        assert!(!should_release_stale_picker_shortcuts(
            false, false, "digit1"
        ));
    }

    #[test]
    fn picker_hidden_with_registered_shortcuts_should_trigger_cleanup() {
        assert!(should_release_stale_picker_shortcuts(false, true, "digit1"));
    }

    #[test]
    fn picker_session_shortcuts_should_not_contain_search_jump_keys_after_editor_split() {
        assert!(!is_picker_session_shortcut("ctrl+f"));
        assert!(!is_picker_session_shortcut("control+keyf"));
        assert!(!is_picker_session_shortcut("control+keye"));
        assert!(is_picker_session_shortcut("control+enter"));
        assert!(is_picker_session_shortcut("control+space"));
        assert!(!is_picker_session_shortcut("space"));
    }

    #[test]
    fn normalize_shortcut_accepts_win_modifier_alias() {
        assert_eq!(
            normalize_shortcut("Win+F").unwrap(),
            normalize_shortcut("Super+F").unwrap()
        );
    }

    #[test]
    fn normalize_enabled_search_shortcut_skips_invalid_value() {
        assert_eq!(normalize_enabled_search_shortcut(true, "not-a-shortcut"), None);
    }

    #[test]
    fn normalize_enabled_search_shortcut_returns_normalized_value() {
        assert_eq!(
            normalize_enabled_search_shortcut(true, "Win+F"),
            Some(normalize_shortcut("Super+F").unwrap())
        );
    }
}
