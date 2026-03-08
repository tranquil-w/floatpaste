use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tracing::warn;

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::MANAGER_OPEN_SETTINGS_EVENT,
    },
    services::{shortcut_manager::ShortcutManager, window_coordinator::WindowCoordinator},
};

pub struct TrayService;

impl TrayService {
    pub fn setup(app: &AppHandle) -> Result<(), AppError> {
        let open_manager = MenuItemBuilder::with_id("open-manager", "打开资料库")
            .build(app)
            .map_err(|error| AppError::Message(error.to_string()))?;
        let open_picker = MenuItemBuilder::with_id("open-picker", "打开速贴面板")
            .build(app)
            .map_err(|error| AppError::Message(error.to_string()))?;
        let toggle_monitoring = MenuItemBuilder::with_id("toggle-monitoring", "暂停 / 恢复监听")
            .build(app)
            .map_err(|error| AppError::Message(error.to_string()))?;
        let open_settings = MenuItemBuilder::with_id("open-settings", "打开设置")
            .build(app)
            .map_err(|error| AppError::Message(error.to_string()))?;
        let quit = MenuItemBuilder::with_id("quit", "退出")
            .build(app)
            .map_err(|error| AppError::Message(error.to_string()))?;

        let menu = MenuBuilder::new(app)
            .items(&[
                &open_manager,
                &open_picker,
                &toggle_monitoring,
                &open_settings,
                &quit,
            ])
            .build()
            .map_err(|error| AppError::Message(error.to_string()))?;

        let icon = app
            .default_window_icon()
            .cloned()
            .ok_or_else(|| AppError::Message("缺少默认窗口图标".to_string()))?;

        TrayIconBuilder::new()
            .icon(icon)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open-manager" => {
                    if let Err(error) = WindowCoordinator::open_manager(app) {
                        warn!("托盘打开资料库失败: {error}");
                    }
                }
                "open-picker" => {
                    let Some(state) = app.try_state::<AppState>() else {
                        warn!("托盘打开 Picker 时应用状态未就绪");
                        return;
                    };
                    if let Err(error) = WindowCoordinator::toggle_picker(app, &state) {
                        warn!("托盘切换 Picker 失败: {error}");
                    }
                }
                "toggle-monitoring" => {
                    let Some(state) = app.try_state::<AppState>() else {
                        warn!("托盘切换监听时应用状态未就绪");
                        return;
                    };

                    match state.current_settings() {
                        Ok(mut settings) => {
                            settings.pause_monitoring = !settings.pause_monitoring;
                            if let Err(error) = state.update_settings(settings) {
                                warn!("托盘更新监听状态失败: {error}");
                                return;
                            }
                            if let Err(error) = ShortcutManager::update_from_settings(app, &state) {
                                warn!("托盘同步快捷键失败: {error}");
                            }
                        }
                        Err(error) => warn!("托盘读取设置失败: {error}"),
                    }
                }
                "open-settings" => {
                    if let Err(error) = WindowCoordinator::open_manager(app) {
                        warn!("托盘打开设置失败: {error}");
                        return;
                    }
                    let _ = app.emit(MANAGER_OPEN_SETTINGS_EVENT, ());
                }
                "quit" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        state.begin_quit();
                    }
                    app.exit(0);
                }
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let app = tray.app_handle();
                    if let Err(error) = WindowCoordinator::open_manager(&app) {
                        warn!("托盘左键打开资料库失败: {error}");
                    }
                }
            })
            .build(app)
            .map_err(|error| AppError::Message(error.to_string()))?;

        Ok(())
    }
}
