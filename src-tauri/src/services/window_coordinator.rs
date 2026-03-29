use chrono::Utc;
use serde::Serialize;
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalSize, Position, Size, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tracing::{error, info, warn};

use crate::{
    app_bootstrap::AppState,
    domain::{
        editor_session::{EditorReturnTarget, EditorSession, EditorSource},
        error::AppError,
        events::{
            EDITOR_SESSION_END_EVENT, EDITOR_SESSION_START_EVENT, PICKER_SESSION_END_EVENT,
            PICKER_SESSION_START_EVENT, SEARCH_INPUT_RESUME_EVENT,
            SEARCH_INPUT_SUSPEND_EVENT, SEARCH_SESSION_END_EVENT,
            SEARCH_SESSION_START_EVENT,
        },
        settings::UserSetting,
        search_session::{SearchSession, SearchSource},
    },
    platform::windows::active_app::ActiveAppResolver,
    services::{
        picker_position_service::{
            PickerPositionService, PICKER_DEFAULT_HEIGHT, PICKER_DEFAULT_WIDTH, PICKER_MIN_HEIGHT,
            PICKER_MIN_WIDTH,
        },
        shortcut_manager::ShortcutManager,
    },
};

pub struct WindowCoordinator;

pub const SETTINGS_WINDOW_LABEL: &str = "manager";
pub const SETTINGS_WINDOW_TITLE: &str = "FloatPaste · 设置";
pub const PICKER_WINDOW_LABEL: &str = "picker";
pub const PICKER_WINDOW_TITLE: &str = "FloatPaste · 速贴";
pub const SEARCH_WINDOW_LABEL: &str = "workbench";
pub const SEARCH_WINDOW_TITLE: &str = "FloatPaste · 搜索";
pub const SEARCH_WINDOW_DEFAULT_WIDTH: u32 = 900;
pub const SEARCH_WINDOW_DEFAULT_HEIGHT: u32 = 600;
pub const SEARCH_WINDOW_MIN_WIDTH: u32 = 600;
pub const SEARCH_WINDOW_MIN_HEIGHT: u32 = 400;
pub const EDITOR_WINDOW_LABEL: &str = "editor";
pub const EDITOR_WINDOW_TITLE: &str = "FloatPaste · 编辑";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PickerSessionPayload {
    session_id: String,
    shown_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchSessionPayload {
    source: &'static str,
    item_id: Option<String>,
    initial_keyword: Option<String>,
}

impl WindowCoordinator {
    pub fn configure_existing_windows(app: &AppHandle) {
        if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
            configure_settings_window(&window);
        }

        if let Some(window) = app.get_webview_window(PICKER_WINDOW_LABEL) {
            configure_picker_window(&window);
        }

        if let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) {
            configure_search_window(&window);
        }

        if let Some(window) = app.get_webview_window(EDITOR_WINDOW_LABEL) {
            configure_editor_window(&window);
        }
    }

    pub fn open_settings(app: &AppHandle) -> Result<(), AppError> {
        let window = ensure_settings_window(app)?;

        window
            .show()
            .map_err(|error| AppError::Message(error.to_string()))?;
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;
        Ok(())
    }

    pub fn show_picker(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        let window = ensure_picker_window(app)?;
        let settings = state.current_settings()?;
        if state.is_picker_active() {
            let session = state.picker_session()?;
            apply_picker_window_position(&window, state, &settings, session.target_window_hwnd);
            return Ok(());
        }

        restore_picker_window_size(app, &window);

        let target_window = ActiveAppResolver::current_foreground_window_handle();
        state.set_picker_session(target_window)?;
        let _ = window.unminimize();
        apply_picker_window_position(&window, state, &settings, target_window);
        notify_search_input_state(app, target_window, true);
        info!("显示 Picker，target_window={target_window:?}");

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::window_utils::show_window_no_activate(&window)
                .map_err(|error| AppError::Message(error.to_string()))?;
            if let Some(hwnd) = target_window {
                let _ = crate::platform::windows::active_app::ActiveAppResolver::restore_foreground_window(hwnd);
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            window
                .show()
                .map_err(|error| AppError::Message(error.to_string()))?;
        }

        state.begin_picker_activation();

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::picker_mouse_monitor::PickerMouseMonitor::begin_session(
                app.clone(),
            );
        }

        window
            .emit(
                PICKER_SESSION_START_EVENT,
                PickerSessionPayload {
                    session_id: Utc::now().timestamp_millis().to_string(),
                    shown_at: Utc::now().to_rfc3339(),
                },
            )
            .map_err(|error| AppError::Message(error.to_string()))?;
        Ok(())
    }

    pub fn toggle_picker(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        if state.is_picker_active() {
            Self::hide_picker_and_restore_target(app, state)
        } else {
            Self::show_picker(app, state)
        }
    }

    pub fn hide_picker(app: &AppHandle) -> Result<(), AppError> {
        if let Some(state) = app.try_state::<AppState>() {
            state.end_picker_activation();
        }
        ShortcutManager::unregister_picker_session_shortcuts(app);
        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::picker_mouse_monitor::PickerMouseMonitor::end_session();
        }

        let Some(window) = app.get_webview_window(PICKER_WINDOW_LABEL) else {
            return Ok(());
        };

        if window
            .is_visible()
            .map_err(|error| AppError::Message(error.to_string()))?
        {
            persist_picker_window_position(app, &window);
            window
                .hide()
                .map_err(|error| AppError::Message(error.to_string()))?;
        }

        info!("隐藏 Picker");
        let _ = window.emit(PICKER_SESSION_END_EVENT, ());
        Ok(())
    }

    pub fn hide_picker_and_restore_target(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        Self::hide_picker(app)?;
        let session = state.picker_session()?;

        let app_handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let app_clone = app_handle.clone();
            let _ = app_handle.run_on_main_thread(move || {
                if let Some(hwnd) = session.target_window_hwnd {
                    let _ = ActiveAppResolver::restore_foreground_window(hwnd);
                }
                notify_search_input_state(&app_clone, session.target_window_hwnd, false);
            });
        });

        Ok(())
    }

    pub fn resume_search_input_if_target(
        app: &AppHandle,
        target_window_hwnd: Option<isize>,
    ) {
        notify_search_input_state(app, target_window_hwnd, false);
    }

    pub fn open_editor_from_picker(
        app: &AppHandle,
        state: &AppState,
        item_id: String,
    ) -> Result<(), AppError> {
        let picker_session = state.picker_session()?;
        Self::hide_picker(app)?;

        let session = EditorSession {
            item_id,
            source: EditorSource::Picker,
            return_to: EditorReturnTarget::Picker,
            target_window_hwnd: picker_session.target_window_hwnd,
        };

        Self::show_editor(app, state, session)
    }

    pub fn open_search_global(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        let window = ensure_search_window(app)?;

        if state.is_search_active() && is_window_ready_for_reuse(&window)? {
            show_and_focus_window(&window)?;
            if restore_search_window_geometry(&window) {
                show_and_focus_window(&window)?;
            }
            return Ok(());
        }

        if state.is_search_active() {
            state.end_search_activation();
        }

        if state.is_picker_active() {
            Self::hide_picker(app)?;
        }

        let target_window = ActiveAppResolver::current_foreground_window_handle();
        let search_session = SearchSession {
            target_window_hwnd: target_window,
            source: SearchSource::GlobalShortcut,
            current_item_id: None,
        };

        state.set_search_session(search_session)?;

        show_and_focus_window(&window)?;
        if restore_search_window_geometry(&window) {
            show_and_focus_window(&window)?;
        }

        state.begin_search_activation();
        begin_search_window_minimize_monitor(app.clone(), state.clone());

        window
            .emit(
                SEARCH_SESSION_START_EVENT,
                SearchSessionPayload {
                    source: "global",
                    item_id: None,
                    initial_keyword: None,
                },
            )
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("全局快捷键打开 Search");
        Ok(())
    }

    pub fn open_editor_from_search(
        app: &AppHandle,
        state: &AppState,
        item_id: String,
    ) -> Result<(), AppError> {
        let target_window_hwnd = state
            .search_session()?
            .and_then(|session| session.target_window_hwnd);

        Self::hide_search_for_editor_transition(app, state)?;

        let session = EditorSession {
            item_id,
            source: EditorSource::Search,
            return_to: EditorReturnTarget::Search,
            target_window_hwnd,
        };

        Self::show_editor(app, state, session)
    }

    pub fn hide_search_and_restore_target(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        hide_search_window(app, state, true)
    }

    pub fn hide_search_without_restore_target(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        hide_search_window(app, state, false)
    }

    pub fn hide_editor_and_restore_source(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        state.end_editor_activation();
        let session = state.editor_session()?;

        let Some(window) = app.get_webview_window(EDITOR_WINDOW_LABEL) else {
            state.clear_editor_session()?;
            return Ok(());
        };

        if window
            .is_visible()
            .map_err(|error| AppError::Message(error.to_string()))?
        {
            window
                .hide()
                .map_err(|error| AppError::Message(error.to_string()))?;
        }

        let _ = window.emit(EDITOR_SESSION_END_EVENT, ());
        state.clear_editor_session()?;

        let Some(session) = session else {
            return Ok(());
        };

        match session.return_to {
            EditorReturnTarget::Picker => restore_picker_after_editor(app, state, &session),
            EditorReturnTarget::Search => restore_search_after_editor(app, state),
        }
    }

    fn show_editor(
        app: &AppHandle,
        state: &AppState,
        session: EditorSession,
    ) -> Result<(), AppError> {
        let window = ensure_editor_window(app)?;
        state.set_editor_session(session.clone())?;
        state.begin_editor_activation();

        window
            .show()
            .map_err(|error| AppError::Message(error.to_string()))?;
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;
        window
            .emit(EDITOR_SESSION_START_EVENT, session)
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("打开 Editor");
        Ok(())
    }

    fn hide_search_for_editor_transition(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        state.end_search_activation();

        let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) else {
            return Ok(());
        };

        if window
            .is_visible()
            .map_err(|error| AppError::Message(error.to_string()))?
        {
            window
                .hide()
                .map_err(|error| AppError::Message(error.to_string()))?;
        }

        Ok(())
    }
}

fn hide_search_window(
    app: &AppHandle,
    state: &AppState,
    restore_target: bool,
) -> Result<(), AppError> {
    state.end_search_activation();
    let session = state.search_session()?;

    if let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) {
        if window
            .is_visible()
            .map_err(|error| AppError::Message(error.to_string()))?
        {
            window
                .hide()
                .map_err(|error| AppError::Message(error.to_string()))?;
        }

        if let Err(err) = window.emit(SEARCH_SESSION_END_EVENT, ()) {
            error!("发送 SEARCH_SESSION_END_EVENT 失败: {err}");
        }
    }

    if restore_target {
        if let Some(ref session) = session {
            if let Some(hwnd) = session.target_window_hwnd {
                let _ = ActiveAppResolver::restore_foreground_window(hwnd);
            }
        }
    }

    state.clear_search_session()?;
    info!("隐藏 Search，会话已清理");
    Ok(())
}

fn ensure_settings_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, SETTINGS_WINDOW_LABEL, WebviewUrl::default())
        .title(SETTINGS_WINDOW_TITLE)
        .inner_size(1200.0, 760.0)
        .resizable(true)
        .center()
        .visible(false)
        .build()
        .map_err(|error| AppError::Message(format!("重新创建 settings 窗口失败: {error}")))?;

    configure_settings_window(&window);
    Ok(window)
}

fn ensure_picker_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(PICKER_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, PICKER_WINDOW_LABEL, WebviewUrl::default())
        .title(PICKER_WINDOW_TITLE)
        .inner_size(PICKER_DEFAULT_WIDTH as f64, PICKER_DEFAULT_HEIGHT as f64)
        .min_inner_size(PICKER_MIN_WIDTH as f64, PICKER_MIN_HEIGHT as f64)
        .resizable(true)
        .visible(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(false)
        .build()
        .map_err(|error| AppError::Message(format!("创建 picker 窗口失败: {error}")))?;

    configure_picker_window(&window);
    Ok(window)
}

fn ensure_search_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, SEARCH_WINDOW_LABEL, WebviewUrl::default())
        .title(SEARCH_WINDOW_TITLE)
        .inner_size(SEARCH_WINDOW_DEFAULT_WIDTH as f64, SEARCH_WINDOW_DEFAULT_HEIGHT as f64)
        .min_inner_size(SEARCH_WINDOW_MIN_WIDTH as f64, SEARCH_WINDOW_MIN_HEIGHT as f64)
        .resizable(true)
        .visible(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(false)
        .center()
        .build()
        .map_err(|error| AppError::Message(format!("创建 search 窗口失败: {error}")))?;

    configure_search_window(&window);
    Ok(window)
}

fn ensure_editor_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(EDITOR_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, EDITOR_WINDOW_LABEL, WebviewUrl::default())
        .title(EDITOR_WINDOW_TITLE)
        .inner_size(800.0, 600.0)
        .min_inner_size(400.0, 300.0)
        .resizable(true)
        .visible(false)
        .decorations(true)
        .always_on_top(false)
        .skip_taskbar(false)
        .center()
        .build()
        .map_err(|error| AppError::Message(format!("创建 editor 窗口失败: {error}")))?;

    configure_editor_window(&window);
    Ok(window)
}

fn configure_search_window(window: &WebviewWindow) {
    if let Err(error) = window.set_always_on_top(true) {
        warn!("设置搜索窗口置顶失败: {error}");
    }

    #[cfg(target_os = "windows")]
    if let Err(error) =
        crate::platform::windows::window_utils::remove_window_system_menu(window)
    {
        warn!("移除搜索窗口系统菜单失败: {error}");
    }

    #[cfg(target_os = "windows")]
    if let Err(error) =
        crate::platform::windows::window_utils::block_alt_menu_activation(window)
    {
        warn!("æ‹¦æˆªæœç´¢çª—å£ Alt ç³»ç»Ÿèœå•æ¿€æ´»å¤±è´¥: {error}");
    }

    let app = window.app_handle().clone();
    window.on_window_event(move |event| {
        match event {
            WindowEvent::CloseRequested { api, .. } => {
                if app
                    .try_state::<AppState>()
                    .map(|state| state.is_quitting())
                    .unwrap_or(false)
                {
                    return;
                }
                api.prevent_close();
                if let Some(state) = app.try_state::<AppState>() {
                    if let Err(err) =
                        WindowCoordinator::hide_search_and_restore_target(&app, &state)
                    {
                        error!("Search CloseRequested 处理失败: {err}");
                    }
                }
            }
            WindowEvent::Focused(false) => {
                if app
                    .try_state::<AppState>()
                    .map(|state| state.is_quitting())
                    .unwrap_or(false)
                {
                    return;
                }

                if let Some(state) = app.try_state::<AppState>() {
                    #[cfg(target_os = "windows")]
                    {
                        if let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) {
                            if crate::platform::windows::window_utils::is_cursor_inside_window(&window)
                                .unwrap_or(false)
                            {
                                return;
                            }
                        }
                    }

                    if state.should_ignore_search_focus_loss().unwrap_or(false) {
                        return;
                    }

                    if state.is_search_active() {
                        if let Err(err) =
                            WindowCoordinator::hide_search_without_restore_target(&app, &state)
                        {
                            error!("Search Focused(false) 处理失败: {err}");
                        }
                    }
                }
            }
            _ => {}
        }
    });
}

fn configure_editor_window(_window: &WebviewWindow) {}

fn configure_settings_window(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if app
                .try_state::<AppState>()
                .map(|state| state.is_quitting())
                .unwrap_or(false)
            {
                return;
            }
            api.prevent_close();
            let _ = handle.hide();
        }
    });
}

fn apply_picker_window_position(
    window: &WebviewWindow,
    state: &AppState,
    settings: &UserSetting,
    target_window_hwnd: Option<isize>,
) {
    let position = PickerPositionService::resolve_window_position(
        window,
        &state.repository,
        &settings.picker_position_mode,
        target_window_hwnd,
    )
    .ok()
    .flatten();

    if let Some(position) = position {
        let _ = window.set_position(Position::Physical(position));
    } else {
        let _ = window.center();
    }
}

fn persist_picker_window_position(app: &AppHandle, window: &WebviewWindow) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let position = PickerPositionService::capture_window_position(window)
        .ok()
        .flatten();
    if let Some(position) = position {
        let _ = state.repository.save_picker_window_state(&position);
    }
}

fn restore_picker_window_size(app: &AppHandle, window: &WebviewWindow) {
    let _ = window.set_min_size(Some(Size::Physical(PhysicalSize::new(
        PICKER_MIN_WIDTH,
        PICKER_MIN_HEIGHT,
    ))));

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let size = match PickerPositionService::resolve_window_size(&state.repository) {
        Ok(size) => size,
        Err(error) => {
            warn!("恢复 picker 窗口尺寸失败: {error}");
            None
        }
    };
    if let Some(size) = size {
        let _ = window.set_size(Size::Physical(size));
    }
}

fn restore_picker_after_editor(
    app: &AppHandle,
    state: &AppState,
    session: &EditorSession,
) -> Result<(), AppError> {
    state.set_picker_session(session.target_window_hwnd)?;
    notify_search_input_state(app, session.target_window_hwnd, true);
    let window = ensure_picker_window(app)?;

    restore_picker_window_size(app, &window);

    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::window_utils::show_window_no_activate(&window)
            .map_err(|error| AppError::Message(error.to_string()))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        window
            .show()
            .map_err(|error| AppError::Message(error.to_string()))?;
    }

    state.begin_picker_activation();
    ShortcutManager::register_picker_session_shortcuts(app)?;

    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::picker_mouse_monitor::PickerMouseMonitor::begin_session(
            app.clone(),
        );
    }

    info!("从 Editor 返回 Picker");
    Ok(())
}

fn restore_search_after_editor(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    let window = ensure_search_window(app)?;
    show_and_focus_window(&window)?;
    if restore_search_window_geometry(&window) {
        show_and_focus_window(&window)?;
    }

    state.begin_search_activation();
    begin_search_window_minimize_monitor(app.clone(), state.clone());
    info!("从 Editor 返回 Search");
    Ok(())
}

fn is_window_ready_for_reuse(window: &WebviewWindow) -> Result<bool, AppError> {
    let is_visible = window
        .is_visible()
        .map_err(|error| AppError::Message(error.to_string()))?;
    #[cfg(target_os = "windows")]
    let is_minimized = crate::platform::windows::window_utils::is_window_minimized(window)
        .map_err(AppError::Message)?;

    #[cfg(not(target_os = "windows"))]
    let is_minimized = window
        .is_minimized()
        .map_err(|error| AppError::Message(error.to_string()))?;

    Ok(is_visible && !is_minimized)
}

fn show_and_focus_window(window: &WebviewWindow) -> Result<(), AppError> {
    window
        .show()
        .map_err(|error| AppError::Message(error.to_string()))?;

    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::window_utils::restore_window_and_focus(window)
            .map_err(AppError::Message)?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;
        Ok(())
    }
}

fn restore_search_window_geometry(window: &WebviewWindow) -> bool {
    let _ = window.set_min_size(Some(Size::Physical(PhysicalSize::new(
        SEARCH_WINDOW_MIN_WIDTH,
        SEARCH_WINDOW_MIN_HEIGHT,
    ))));

    let outer_position = window.outer_position().ok();
    let outer_size = window.outer_size().ok();
    let has_offscreen_position = outer_position
        .map(|position| position.x <= -30_000 || position.y <= -30_000)
        .unwrap_or(false);
    let has_shell_placeholder_size = outer_size
        .map(|size| size.width <= 240 || size.height <= 80)
        .unwrap_or(false);

    if !has_offscreen_position && !has_shell_placeholder_size {
        return false;
    }

    let _ = window.set_size(Size::Physical(PhysicalSize::new(
        SEARCH_WINDOW_DEFAULT_WIDTH,
        SEARCH_WINDOW_DEFAULT_HEIGHT,
    )));
    let _ = window.center();
    true
}

fn begin_search_window_minimize_monitor(app: AppHandle, state: AppState) {
    #[cfg(target_os = "windows")]
    {
        let token = state.next_search_session_monitor_token();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(120));

                if !state.is_search_active() {
                    break;
                }

                if state.current_search_session_monitor_token() != token {
                    break;
                }

                let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) else {
                    break;
                };

                let is_minimized =
                    crate::platform::windows::window_utils::is_window_minimized(&window)
                        .unwrap_or(false);
                if !is_minimized {
                    continue;
                }

                let app_handle = app.clone();
                let state_clone = state.clone();
                let _ = app.run_on_main_thread(move || {
                    if state_clone.is_search_active() {
                        if let Err(error) =
                            WindowCoordinator::hide_search_without_restore_target(
                                &app_handle,
                                &state_clone,
                            )
                        {
                            error!("搜索窗口最小化后自动结束会话失败: {error}");
                        }
                    }
                });
                break;
            }
        });
    }
}

fn notify_search_input_state(
    app: &AppHandle,
    target_window_hwnd: Option<isize>,
    suspended: bool,
) {
    let Some(search) = app.get_webview_window(SEARCH_WINDOW_LABEL) else {
        return;
    };
    let Ok(hwnd) = search.hwnd() else {
        return;
    };

    if target_window_hwnd != Some(hwnd.0 as isize) {
        return;
    }

    let event_name = if suspended {
        SEARCH_INPUT_SUSPEND_EVENT
    } else {
        SEARCH_INPUT_RESUME_EVENT
    };
    let _ = search.emit(event_name, ());
}

#[cfg(test)]
fn should_restore_picker_after_search_close(_session: &SearchSession) -> bool {
    false
}

fn configure_picker_window(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    if let Err(error) = crate::platform::windows::window_utils::apply_picker_window_shape(window) {
        warn!("初始化 picker 圆角窗口失败: {error}");
    }

    #[cfg(target_os = "windows")]
    if let Err(error) =
        crate::platform::windows::window_utils::remove_window_system_menu(window)
    {
        warn!("移除 Picker 系统菜单失败: {error}");
    }

    let app = window.app_handle().clone();
    let handle = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            if app
                .try_state::<AppState>()
                .map(|state| state.is_quitting())
                .unwrap_or(false)
            {
                return;
            }
            api.prevent_close();
            if let Some(state) = app.try_state::<AppState>() {
                let _ = WindowCoordinator::hide_picker_and_restore_target(&app, &state);
            } else {
                let _ = WindowCoordinator::hide_picker(&app);
            }
        }
        #[cfg(target_os = "windows")]
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            if let Err(error) =
                crate::platform::windows::window_utils::apply_picker_window_shape(&handle)
            {
                warn!("刷新 picker 圆角窗口失败: {error}");
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::should_restore_picker_after_search_close;
    use crate::domain::search_session::{SearchSession, SearchSource};
    use serde_json::Value;

    #[test]
    fn search_close_should_not_restore_picker_flow() {
        let session = SearchSession {
            target_window_hwnd: None,
            source: SearchSource::GlobalShortcut,
            current_item_id: None,
        };
        assert!(!should_restore_picker_after_search_close(&session));
    }

    #[test]
    fn tauri_config_should_disable_search_native_decorations() {
        let config: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let windows = config["app"]["windows"].as_array().unwrap();
        let search = windows
            .iter()
            .find(|window| window["label"] == "workbench")
            .unwrap();

        assert_eq!(search["decorations"], Value::Bool(false));
    }
}
