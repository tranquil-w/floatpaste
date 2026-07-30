use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};
use tracing::warn;

use crate::{
    app_bootstrap::AppState,
    domain::error::AppError,
    services::{
        settings_service::SettingsService, shortcut_manager::ShortcutManager,
        window_coordinator::WindowCoordinator,
    },
};

pub struct TrayService;

impl TrayService {
    pub fn setup(app: &AppHandle) -> Result<(), AppError> {
        let open_settings = MenuItemBuilder::with_id("open-settings", "打开设置")
            .build(app)?;
        let open_picker = MenuItemBuilder::with_id("open-picker", "打开速贴面板")
            .build(app)?;
        let toggle_monitoring = MenuItemBuilder::with_id("toggle-monitoring", "暂停 / 恢复监听")
            .build(app)?;
        let quit = MenuItemBuilder::with_id("quit", "退出")
            .build(app)?;

        let menu = MenuBuilder::new(app)
            .items(&[
                &open_settings,
                &open_picker,
                &toggle_monitoring,
                &quit,
            ])
            .build()?;

        let icon = app
            .default_window_icon()
            .cloned()
            .ok_or_else(|| AppError::Message("缺少默认窗口图标".to_string()))?;

        TrayIconBuilder::new()
            .icon(icon)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open-settings" => {
                    if let Err(error) = WindowCoordinator::open_settings(app) {
                        warn!("托盘打开设置失败: {error}");
                    }
                }
                "open-picker" => {
                    let Some(state) = app.try_state::<AppState>() else {
                        warn!("托盘打开 Picker 时应用状态未就绪");
                        return;
                    };
                    // 菜单语义为"打开"：直接走显示路径。show_picker 已能自愈标志位与
                    // 窗口可见性脱节的情形，并在已正常显示时仅重定位。显示成功后注册
                    // 会话快捷键（方向键/回车/Esc 等），否则从托盘打开的速贴窗口无法
                    // 用键盘操作，与主快捷键路径行为一致。
                    if let Err(error) = WindowCoordinator::show_picker(app, &state) {
                        warn!("托盘显示 Picker 失败: {error}");
                    } else if let Err(error) =
                        ShortcutManager::register_picker_session_shortcuts(app)
                    {
                        warn!("托盘打开 Picker 后注册会话快捷键失败: {error}");
                    }
                }
                "toggle-monitoring" => {
                    let Some(state) = app.try_state::<AppState>() else {
                        warn!("托盘切换监听时应用状态未就绪");
                        return;
                    };

                    match state.current_settings() {
                        Ok(previous_settings) => {
                            let mut settings = previous_settings.clone();
                            settings.pause_monitoring = !settings.pause_monitoring;
                            if let Err(error) = state.update_settings(settings) {
                                warn!("托盘更新监听状态失败: {error}");
                                return;
                            }
                            if let Err(error) =
                                SettingsService::apply_runtime_side_effects(app, &state)
                            {
                                let _ = state.update_settings(previous_settings);
                                let _ = SettingsService::apply_runtime_side_effects(app, &state);
                                warn!("托盘同步运行设置失败: {error}");
                            }
                        }
                        Err(error) => warn!("托盘读取设置失败: {error}"),
                    }
                }
                "quit" => {
                    // 退出前同步销毁所有窗口 + 卸载鼠标钩子 + 停止长按导航。
                    // 必须销毁（而非隐藏）窗口，否则 Chromium 注销窗口类时仍会因
                    // ERROR_CLASS_HAS_WINDOWS (1412) 失败。RunEvent::ExitRequested 兜底再调一次（幂等）。
                    WindowCoordinator::prepare_for_exit(app);
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
                    if let Err(error) = WindowCoordinator::open_settings(&app) {
                        warn!("托盘左键打开设置失败: {error}");
                    }
                }
            })
            .build(app)?;

        Ok(())
    }
}
