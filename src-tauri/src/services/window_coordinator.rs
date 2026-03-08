use chrono::Utc;
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tracing::info;

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::{PICKER_SESSION_END_EVENT, PICKER_SESSION_START_EVENT},
    },
    platform::windows::active_app::ActiveAppResolver,
};

pub struct WindowCoordinator;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PickerSessionPayload {
    session_id: String,
    shown_at: String,
}

impl WindowCoordinator {
    pub fn configure_existing_windows(app: &AppHandle) {
        if let Some(window) = app.get_webview_window("manager") {
            configure_manager_window(&window);
        }

        if let Some(window) = app.get_webview_window("picker") {
            configure_picker_window(&window);
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
        if state.is_picker_active() {
            let _ = window.center();
            return Ok(());
        }

        let manager_visible = app
            .get_webview_window("manager")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        let target_window = if manager_visible {
            None
        } else {
            ActiveAppResolver::current_foreground_window_handle()
        };
        state.set_picker_session(target_window, manager_visible)?;
        let _ = window.unminimize();
        let _ = window.center();
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

        if manager_visible {
            if let Some(manager) = app.get_webview_window("manager") {
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
        let Some(window) = app.get_webview_window("picker") else {
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
}

fn ensure_manager_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window("manager") {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, "manager", WebviewUrl::default())
        .title("FloatPaste / 浮贴")
        .inner_size(1480.0, 920.0)
        .resizable(true)
        .center()
        .visible(true)
        .build()
        .map_err(|error| AppError::Message(format!("重新创建 manager 窗口失败: {error}")))?;

    configure_manager_window(&window);
    Ok(window)
}

fn ensure_picker_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window("picker") {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, "picker", WebviewUrl::default())
        .title("FloatPaste Picker")
        .inner_size(360.0, 420.0)
        .resizable(false)
        .visible(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(false)
        .build()
        .map_err(|error| AppError::Message(format!("创建 picker 窗口失败: {error}")))?;

    configure_picker_window(&window);
    let _ = window.center();
    Ok(window)
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

fn configure_picker_window(window: &WebviewWindow) {
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
                let _ = WindowCoordinator::hide_picker_and_restore_target(&app, &state);
            } else {
                let _ = WindowCoordinator::hide_picker(&app);
            }
        }
    });
}
