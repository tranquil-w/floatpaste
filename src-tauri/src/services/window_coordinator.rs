use chrono::Utc;
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalSize, Position, Size, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tracing::{error, info, warn};

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::{
            PICKER_SESSION_END_EVENT, PICKER_SESSION_START_EVENT, WORKBENCH_SESSION_END_EVENT,
            WORKBENCH_SESSION_START_EVENT,
        },
        settings::UserSetting,
    },
    platform::windows::active_app::ActiveAppResolver,
    services::picker_position_service::{
        PickerPositionService, PICKER_DEFAULT_HEIGHT, PICKER_DEFAULT_WIDTH, PICKER_MIN_HEIGHT,
        PICKER_MIN_WIDTH,
    },
};

pub struct WindowCoordinator;
pub const MANAGER_WINDOW_LABEL: &str = "manager";
pub const MANAGER_WINDOW_TITLE: &str = "FloatPaste / 浮贴";
pub const PICKER_WINDOW_LABEL: &str = "picker";
pub const PICKER_WINDOW_TITLE: &str = "FloatPaste Picker";
pub const WORKBENCH_WINDOW_LABEL: &str = "workbench";
pub const WORKBENCH_WINDOW_TITLE: &str = "FloatPaste Workbench";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PickerSessionPayload {
    session_id: String,
    shown_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchSessionPayload {
    source: &'static str,
    item_id: Option<String>,
    initial_keyword: Option<String>,
}

impl WindowCoordinator {
    pub fn configure_existing_windows(app: &AppHandle) {
        if let Some(window) = app.get_webview_window(MANAGER_WINDOW_LABEL) {
            configure_manager_window(&window);
        }

        if let Some(window) = app.get_webview_window(PICKER_WINDOW_LABEL) {
            configure_picker_window(&window);
        }

        if let Some(window) = app.get_webview_window(WORKBENCH_WINDOW_LABEL) {
            configure_workbench_window(&window);
        }
    }

    pub fn open_manager(app: &AppHandle) -> Result<(), AppError> {
        let window = ensure_manager_window(app)?;

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

        let manager_visible = app
            .get_webview_window(MANAGER_WINDOW_LABEL)
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        let target_window = if manager_visible {
            None
        } else {
            ActiveAppResolver::current_foreground_window_handle()
        };
        state.set_picker_session(target_window, manager_visible)?;
        let _ = window.unminimize();
        apply_picker_window_position(&window, state, &settings, target_window);
        info!("显示 Picker，manager_visible={manager_visible}, target_window={target_window:?}");

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::window_utils::show_window_no_activate(&window)
                .map_err(|error| AppError::Message(error.to_string()))?;
            // 作为保险，如果焦点仍然被抢占，快速恢复原目标窗口焦点
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

        if manager_visible {
            if let Some(manager) = app.get_webview_window(MANAGER_WINDOW_LABEL) {
                let _ = manager.hide();
            }
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
        crate::services::shortcut_manager::ShortcutManager::unregister_picker_session_shortcuts(
            app,
        );
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
                } else if session.reopen_manager_on_close {
                    let _ = Self::open_manager(&app_clone);
                }
            });
        });

        Ok(())
    }

    pub fn hide_picker_and_open_manager(
        app: &AppHandle,
        _state: &AppState,
    ) -> Result<(), AppError> {
        Self::hide_picker(app)?;

        let app_handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let app_clone = app_handle.clone();
            let _ = app_handle.run_on_main_thread(move || {
                let _ = Self::open_manager(&app_clone);
            });
        });

        Ok(())
    }

    /// 从 Picker 编辑进入 Workbench
    pub fn open_workbench_from_picker_edit(
        app: &AppHandle,
        state: &AppState,
        item_id: String,
    ) -> Result<(), AppError> {
        let window = ensure_workbench_window(app)?;
        let picker_session = state.picker_session()?;

        let workbench_session = crate::domain::workbench_session::WorkbenchSession {
            target_window_hwnd: picker_session.target_window_hwnd,
            source: crate::domain::workbench_session::WorkbenchSource::PickerEdit,
            current_item_id: Some(item_id.clone()),
            from_picker: true,
            picker_selected_index: None,
        };

        state.set_workbench_session(workbench_session)?;
        Self::hide_picker(app)?;

        if let Err(error) = window.show() {
            error!("Workbench 显示失败，尝试恢复 Picker: {error}");
            let _ = state.clear_workbench_session();
            let _ = Self::show_picker(app, state);
            return Err(AppError::Message(error.to_string()));
        }
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;

        state.begin_workbench_activation();

        window
            .emit(
                WORKBENCH_SESSION_START_EVENT,
                WorkbenchSessionPayload {
                    source: "picker_edit",
                    item_id: Some(item_id.clone()),
                    initial_keyword: None,
                },
            )
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("从 Picker 编辑进入 Workbench, item_id={item_id}");
        Ok(())
    }

    /// 从 Picker 搜索进入 Workbench
    pub fn open_workbench_from_picker_search(
        app: &AppHandle,
        state: &AppState,
        initial_keyword: Option<String>,
    ) -> Result<(), AppError> {
        let window = ensure_workbench_window(app)?;
        let picker_session = state.picker_session()?;

        let workbench_session = crate::domain::workbench_session::WorkbenchSession {
            target_window_hwnd: picker_session.target_window_hwnd,
            source: crate::domain::workbench_session::WorkbenchSource::PickerSearch,
            current_item_id: None,
            from_picker: true,
            picker_selected_index: None,
        };

        state.set_workbench_session(workbench_session)?;
        Self::hide_picker(app)?;

        if let Err(error) = window.show() {
            error!("Workbench 显示失败，尝试恢复 Picker: {error}");
            let _ = state.clear_workbench_session();
            let _ = Self::show_picker(app, state);
            return Err(AppError::Message(error.to_string()));
        }
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;

        state.begin_workbench_activation();

        window
            .emit(
                WORKBENCH_SESSION_START_EVENT,
                WorkbenchSessionPayload {
                    source: "picker_search",
                    item_id: None,
                    initial_keyword: initial_keyword.clone(),
                },
            )
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("从 Picker 搜索进入 Workbench, keyword={initial_keyword:?}");
        Ok(())
    }

    /// 全局快捷键直接打开 Workbench
    pub fn open_workbench_global(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        let window = ensure_workbench_window(app)?;

        if state.is_workbench_active() {
            window
                .set_focus()
                .map_err(|error| AppError::Message(error.to_string()))?;
            return Ok(());
        }

        let target_window = ActiveAppResolver::current_foreground_window_handle();

        let workbench_session = crate::domain::workbench_session::WorkbenchSession {
            target_window_hwnd: target_window,
            source: crate::domain::workbench_session::WorkbenchSource::GlobalShortcut,
            current_item_id: None,
            from_picker: false,
            picker_selected_index: None,
        };

        state.set_workbench_session(workbench_session)?;

        window
            .show()
            .map_err(|error| AppError::Message(error.to_string()))?;
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;

        state.begin_workbench_activation();

        window
            .emit(
                WORKBENCH_SESSION_START_EVENT,
                WorkbenchSessionPayload {
                    source: "global",
                    item_id: None,
                    initial_keyword: None,
                },
            )
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("全局快捷键打开 Workbench");
        Ok(())
    }

    /// 隐藏 Workbench 并恢复目标窗口
    pub fn hide_workbench_and_restore_target(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        state.end_workbench_activation();
        crate::services::shortcut_manager::ShortcutManager::unregister_workbench_session_shortcuts(
            app,
        );
        let session = state.workbench_session()?;

        let Some(window) = app.get_webview_window(WORKBENCH_WINDOW_LABEL) else {
            return Ok(());
        };

        window
            .hide()
            .map_err(|error| AppError::Message(error.to_string()))?;

        if let Err(err) = window.emit(WORKBENCH_SESSION_END_EVENT, ()) {
            error!("发送 WORKBENCH_SESSION_END_EVENT 失败: {err}");
        }
        state.clear_workbench_session()?;

        if let Some(ref sess) = session {
            // Workbench 是普通装饰窗口（非 show_window_no_activate），隐藏后可立即还焦，无需 50ms 延时
            if let Some(hwnd) = sess.target_window_hwnd {
                let _ = ActiveAppResolver::restore_foreground_window(hwnd);
            }
        }

        info!("隐藏 Workbench");
        Ok(())
    }
}

fn ensure_manager_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(MANAGER_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, MANAGER_WINDOW_LABEL, WebviewUrl::default())
        .title(MANAGER_WINDOW_TITLE)
        .inner_size(1200.0, 760.0)
        .resizable(true)
        .center()
        .visible(false)
        .build()
        .map_err(|error| AppError::Message(format!("重新创建 manager 窗口失败: {error}")))?;

    configure_manager_window(&window);
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

fn ensure_workbench_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(WORKBENCH_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, WORKBENCH_WINDOW_LABEL, WebviewUrl::default())
        .title(WORKBENCH_WINDOW_TITLE)
        .inner_size(900.0, 600.0)
        .min_inner_size(600.0, 400.0)
        .resizable(true)
        .visible(false)
        .decorations(true)
        .always_on_top(false)
        .skip_taskbar(false)
        .center()
        .build()
        .map_err(|error| AppError::Message(format!("创建 workbench 窗口失败: {error}")))?;

    configure_workbench_window(&window);
    Ok(window)
}

fn configure_workbench_window(window: &WebviewWindow) {
    let app = window.app_handle().clone();
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
            if let Some(state) = app.try_state::<AppState>() {
                if let Err(err) = WindowCoordinator::hide_workbench_and_restore_target(&app, &state)
                {
                    error!("Workbench CloseRequested 处理失败: {err}");
                }
            }
        }
    });
}

fn configure_manager_window(window: &WebviewWindow) {
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

fn configure_picker_window(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    if let Err(error) = crate::platform::windows::window_utils::apply_picker_window_shape(window) {
        warn!("初始化 picker 圆角窗口失败: {error}");
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
