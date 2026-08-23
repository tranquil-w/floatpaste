use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder},
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

/// 监听菜单文案跟随当前状态，避免用户误判监听是否已暂停
fn monitoring_menu_label(paused: bool) -> &'static str {
    if paused {
        "恢复监听（当前已暂停）"
    } else {
        "暂停监听"
    }
}

fn build_menu(app: &AppHandle, monitoring_paused: bool) -> Result<Menu<tauri::Wry>, AppError> {
    let open_settings = MenuItemBuilder::with_id("open-settings", "打开设置").build(app)?;
    let open_picker = MenuItemBuilder::with_id("open-picker", "打开速贴面板").build(app)?;
    let open_search = MenuItemBuilder::with_id("open-search", "打开搜索").build(app)?;
    let toggle_monitoring =
        MenuItemBuilder::with_id("toggle-monitoring", monitoring_menu_label(monitoring_paused))
            .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    Ok(MenuBuilder::new(app)
        .items(&[&open_picker, &open_search, &open_settings, &toggle_monitoring, &quit])
        .build()?)
}

/// 托盘固定 id：refresh_menu 通过 tray_by_id 定位重建菜单
const TRAY_ID: &str = "floatpaste-tray";

impl TrayService {
    pub fn setup(app: &AppHandle) -> Result<(), AppError> {
        let monitoring_paused = app
            .try_state::<AppState>()
            .and_then(|state| state.current_settings().ok())
            .map(|settings| settings.pause_monitoring)
            .unwrap_or(false);

        let menu = build_menu(app, monitoring_paused)?;

        let icon = app
            .default_window_icon()
            .cloned()
            .ok_or_else(|| AppError::Message("缺少默认窗口图标".to_string()))?;

        TrayIconBuilder::with_id(TRAY_ID)
            .icon(icon)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open-settings" => {
                    if let Err(error) = WindowCoordinator::open_settings(app) {
                        warn!("托盘打开设置失败: {error}");
                    }
                }
                "open-search" => {
                    let Some(state) = app.try_state::<AppState>() else {
                        warn!("托盘打开搜索时应用状态未就绪");
                        return;
                    };
                    if let Err(error) = WindowCoordinator::open_search_global(app, &state) {
                        warn!("托盘打开搜索失败: {error}");
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

    /// 设置变更后刷新托盘菜单文案（如监听状态切换）。
    pub fn refresh_menu(app: &AppHandle) {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return;
        };

        let monitoring_paused = app
            .try_state::<AppState>()
            .and_then(|state| state.current_settings().ok())
            .map(|settings| settings.pause_monitoring)
            .unwrap_or(false);

        match build_menu(app, monitoring_paused) {
            Ok(menu) => {
                if let Err(error) = tray.set_menu(Some(menu)) {
                    warn!("刷新托盘菜单失败: {error}");
                }
            }
            Err(error) => warn!("重建托盘菜单失败: {error}"),
        }
    }
}
