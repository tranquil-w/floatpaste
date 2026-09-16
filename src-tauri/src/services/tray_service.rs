use tauri::{
    menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};
use tracing::{debug, warn};

use crate::{
    app_bootstrap::AppState,
    domain::error::AppError,
    services::{settings_service::SettingsService, window_coordinator::WindowCoordinator},
};

pub struct TrayService;

/// 托盘「切换监听」菜单项句柄。
///
/// 菜单文案更新必须走 `set_text` 原地改写：`tray.set_menu` 整体重建会让
/// muda 在托盘窗口上重新挂载/卸载子类，实测重建后的菜单第一次点击事件
/// 被吞掉（第二次才生效）。菜单在 setup 时一次性建好，生命周期内不再替换。
pub struct TrayMenuHandles {
    toggle_monitoring: MenuItem<tauri::Wry>,
}

/// 监听菜单文案跟随当前状态，避免用户误判监听是否已暂停
fn monitoring_menu_label(paused: bool) -> &'static str {
    if paused {
        "恢复监听（当前已暂停）"
    } else {
        "暂停监听"
    }
}

fn build_menu(
    app: &AppHandle,
    monitoring_paused: bool,
) -> Result<(Menu<tauri::Wry>, MenuItem<tauri::Wry>), AppError> {
    let open_settings = MenuItemBuilder::with_id("open-settings", "打开设置").build(app)?;
    let open_picker = MenuItemBuilder::with_id("open-picker", "打开速贴面板").build(app)?;
    let open_search = MenuItemBuilder::with_id("open-search", "打开搜索").build(app)?;
    let toggle_monitoring = MenuItemBuilder::with_id(
        "toggle-monitoring",
        monitoring_menu_label(monitoring_paused),
    )
    .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &open_picker,
            &open_search,
            &open_settings,
            &toggle_monitoring,
            &quit,
        ])
        .build()?;

    Ok((menu, toggle_monitoring))
}

/// 托盘固定 id：托盘图标的稳定标识
const TRAY_ID: &str = "floatpaste-tray";

impl TrayService {
    pub fn setup(app: &AppHandle) -> Result<(), AppError> {
        let monitoring_paused = app
            .try_state::<AppState>()
            .and_then(|state| state.current_settings().ok())
            .map(|settings| settings.pause_monitoring)
            .unwrap_or(false);

        let (menu, toggle_monitoring) = build_menu(app, monitoring_paused)?;
        app.manage(TrayMenuHandles { toggle_monitoring });

        // 按 DPI 精确尺寸加载托盘图标，避免单一 RGBA 位图被系统拉伸导致模糊；
        // 加载失败时回退 Tauri 默认窗口图标
        let icon = match crate::platform::windows::app_icon::tray_icon_image() {
            Ok(icon) => icon,
            Err(error) => {
                warn!("按 DPI 加载托盘图标失败，回退默认窗口图标: {error}");
                app.default_window_icon()
                    .cloned()
                    .ok_or_else(|| AppError::Message("缺少默认窗口图标".to_string()))?
            }
        };

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
                    // 菜单语义为"打开"：activate_picker 内部含标志位自愈与窗口
                    // 可见性校验，已正常显示时仅重定位，并统一注册会话快捷键，
                    // 与主快捷键路径行为一致。
                    if let Err(error) = WindowCoordinator::activate_picker(app, &state) {
                        warn!("托盘显示 Picker 失败: {error}");
                    }
                }
                "toggle-monitoring" => {
                    let Some(state) = app.try_state::<AppState>() else {
                        warn!("托盘切换监听时应用状态未就绪");
                        return;
                    };

                    // 一次菜单点击可能触发两次事件（Windows 上游缺陷），第二次会立即
                    // 把状态切回去，表现为"点了没反应"，用时间窗去抖过滤。
                    if !state.should_accept_monitoring_toggle() {
                        debug!("忽略短时间内的重复监听切换事件");
                        return;
                    }

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

    /// 设置变更后同步托盘菜单文案（如监听状态切换）。
    ///
    /// 原地 `set_text` 更新监听项文案，**不得** `set_menu` 整体重建：
    /// 重建后菜单的第一次点击事件会被 muda 子类重挂过程吞掉，
    /// 表现为「暂停/恢复监听要点两次才生效」（见 TrayMenuHandles）。
    pub fn refresh_menu(app: &AppHandle) {
        let monitoring_paused = app
            .try_state::<AppState>()
            .and_then(|state| state.current_settings().ok())
            .map(|settings| settings.pause_monitoring)
            .unwrap_or(false);

        let Some(handles) = app.try_state::<TrayMenuHandles>() else {
            warn!("托盘菜单句柄未就绪，跳过监听文案同步");
            return;
        };
        if let Err(error) = handles
            .toggle_monitoring
            .set_text(monitoring_menu_label(monitoring_paused))
        {
            warn!("更新托盘监听文案失败: {error}");
        }
    }
}
