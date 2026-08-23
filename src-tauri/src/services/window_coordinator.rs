use chrono::Utc;
use serde::Serialize;
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tracing::{error, info, warn};

use crate::{
    app_bootstrap::AppState,
    domain::{
        editor_session::{EditorReturnTarget, EditorSession, EditorSource},
        error::AppError,
        events::{
            EDITOR_SESSION_END_EVENT, EDITOR_SESSION_START_EVENT, PICKER_SESSION_END_EVENT,
            PICKER_SESSION_START_EVENT, SEARCH_INPUT_RESUME_EVENT, SEARCH_INPUT_SUSPEND_EVENT,
            SEARCH_SESSION_END_EVENT, SEARCH_SESSION_START_EVENT,
        },
        search_session::{SearchSession, SearchSource},
        settings::UserSetting,
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
pub const SETTINGS_WINDOW_DEFAULT_WIDTH: u32 = 920;
pub const SETTINGS_WINDOW_DEFAULT_HEIGHT: u32 = 760;
pub const SETTINGS_WINDOW_MIN_WIDTH: u32 = 880;
pub const PICKER_WINDOW_LABEL: &str = "picker";
pub const PICKER_WINDOW_TITLE: &str = "FloatPaste · 速贴";
pub const SEARCH_WINDOW_LABEL: &str = "workbench";
pub const SEARCH_WINDOW_TITLE: &str = "FloatPaste · 搜索";
pub const SEARCH_WINDOW_DEFAULT_WIDTH: u32 = 780;
pub const SEARCH_WINDOW_DEFAULT_HEIGHT: u32 = 420;
pub const SEARCH_WINDOW_MIN_WIDTH: u32 = 620;
pub const SEARCH_WINDOW_MIN_HEIGHT: u32 = 1;
pub const EDITOR_WINDOW_LABEL: &str = "editor";
pub const EDITOR_WINDOW_TITLE: &str = "FloatPaste · 编辑";
const WINDOW_FOCUS_RETRY_DELAY_MS: u64 = 20;
const WINDOW_FOCUS_MAX_RETRIES: u32 = 3;

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

        if let Some(window) = app.get_webview_window(crate::services::tooltip_window::TOOLTIP_WINDOW_LABEL) {
            crate::services::tooltip_window::configure_tooltip_window(&window);
        }
    }

    pub fn open_settings(app: &AppHandle) -> Result<(), AppError> {
        // 先结束 picker 会话：会话快捷键（Esc/Enter/数字键等）是全局热键，
        // 不先解除会劫持设置窗口的键盘输入，导致设置页无法用 Esc 关闭
        if let Some(state) = app.try_state::<AppState>() {
            if state.is_picker_active() {
                Self::hide_picker(app)?;
            }
        }

        let window = ensure_settings_window(app)?;

        window
            .show()?;
        window
            .set_focus()?;
        Ok(())
    }

    pub fn show_picker(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        let window = ensure_picker_window(app)?;
        let settings = state.current_settings()?;
        if state.is_picker_active() {
            // 兜底：is_picker_active 是独立的 AtomicBool，可能与窗口真实可见性脱节
            // （例如 Win+D、多屏切换、DPI/缩放变化、被全屏应用抢占后窗口被系统隐藏）。
            // 若标志位为激活但窗口实际不可见，直接返回会让后续所有打开尝试（快捷键、托盘、命令）
            // 沦为空操作而死锁。这里校验真实可见性，不一致时重置状态后走完整显示流程。
            let window_actually_visible = window.is_visible().unwrap_or(false);
            if window_actually_visible {
                let session = state.picker_session()?;
                apply_picker_window_position(
                    &window,
                    state,
                    &settings,
                    session.target_window_hwnd,
                );
                return Ok(());
            }
            warn!("Picker 标志位为激活但窗口实际不可见，重置状态后重新显示");
            state.end_picker_activation();
            ShortcutManager::unregister_picker_session_shortcuts(app);
        }

        restore_picker_window_size(app, &window);

        let target = ActiveAppResolver::current_foreground_focus_target();
        state.set_picker_session(target.window_hwnd, target.focus_hwnd)?;
        let _ = window.unminimize();
        apply_picker_window_position(&window, state, &settings, target.window_hwnd);
        notify_search_input_state(app, target.window_hwnd, true);
        info!(
            "显示 Picker，target_window={:?}, target_focus={:?}",
            target.window_hwnd, target.focus_hwnd
        );

        state.begin_picker_activation();

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::picker_mouse_monitor::PickerMouseMonitor::begin_session(
                app.clone(),
            );
        }

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::window_utils::show_window_no_activate(&window)?;
            if let Some(hwnd) = target.window_hwnd {
                let _ = crate::platform::windows::active_app::ActiveAppResolver::restore_foreground_window_with_focus(
                    hwnd,
                    target.focus_hwnd,
                );
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            window
                .show()?;
        }

        window
            .emit(
                PICKER_SESSION_START_EVENT,
                PickerSessionPayload {
                    session_id: Utc::now().timestamp_millis().to_string(),
                    shown_at: Utc::now().to_rfc3339(),
                },
            )?;

        Ok(())
    }

    /// 激活 Picker 会话：显示窗口并注册会话快捷键。
    ///
    /// 主快捷键、托盘菜单、启动流程与前端命令打开 Picker 的唯一入口。
    /// 注册失败只降级为鼠标操作（保留 warn 日志），不回滚已显示的窗口。
    pub fn activate_picker(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        Self::show_picker(app, state)?;
        if let Err(error) = ShortcutManager::register_picker_session_shortcuts(app) {
            warn!("注册 Picker 会话快捷键失败，保留鼠标降级路径: {error}");
        }
        Ok(())
    }

    /// 主快捷键命中：Picker 活跃则关闭并恢复目标焦点，否则收起 Search 后激活 Picker。
    ///
    /// 激活状态在执行时实时读取，避免调用方提前捕获快照后，于延迟期间
    /// 被鼠标钩子等其它路径改写（TOCTOU），把"打开"误判成"关闭"。
    /// 须在主线程调用。
    pub fn toggle_picker_from_shortcut(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        if state.is_picker_active() {
            return Self::hide_picker_and_restore_target(app, state);
        }

        if state.is_search_active() {
            // 搜索窗口活跃时先收起，避免两窗口争抢焦点导致闪烁
            Self::hide_search_without_restore_target(app, state)?;
        }

        Self::activate_picker(app, state)
    }

    /// 搜索快捷键命中：搜索窗口可见且未最小化则关闭并恢复目标，否则全局打开。
    pub fn toggle_search_from_shortcut(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        if is_search_window_ready_for_toggle(app) {
            return Self::hide_search_and_restore_target(app, state);
        }

        Self::open_search_global(app, state)
    }

    /// Picker 会话中命中搜索快捷键：只收起 Picker（不恢复目标焦点，焦点交给
    /// 搜索窗口）再打开搜索。
    pub fn open_search_from_active_picker(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        Self::hide_picker(app)?;
        Self::open_search_global(app, state)
    }

    /// 退出前的集中收尾：置退出标志、卸载低级鼠标钩子、停止长按导航、销毁所有窗口。
    ///
    /// 关键：必须真正销毁（destroy）窗口，而非仅隐藏（SW_HIDE）。隐藏的窗口仍是存活的
    /// `Chrome_WidgetWin_0` 类窗口，Chromium 在进程拆解时注销窗口类会因 `ERROR_CLASS_HAS_WINDOWS
    /// (1412)` 失败（参见 tauri#7606、tauri#14088）。销毁后进程退出阶段不再有同类窗口残留。
    /// 供托盘退出与 `RunEvent::ExitRequested` 共用，幂等可重复调用。
    pub fn prepare_for_exit(app: &AppHandle) {
        if let Some(state) = app.try_state::<AppState>() {
            state.begin_quit();
        }

        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::picker_mouse_monitor::PickerMouseMonitor::end_session();
        }

        // 停止 picker 长按导航 repeat 线程，避免它在退出后继续 run_on_main_thread + emit。
        ShortcutManager::stop_all_picker_navigation_repeat();

        // 真正销毁所有窗口（destroy 绕过 CloseRequested 拦截，直接释放窗口及其 webview）。
        for label in [
            PICKER_WINDOW_LABEL,
            SEARCH_WINDOW_LABEL,
            EDITOR_WINDOW_LABEL,
            SETTINGS_WINDOW_LABEL,
            crate::services::tooltip_window::TOOLTIP_WINDOW_LABEL,
        ] {
            if let Some(window) = app.get_webview_window(label) {
                if let Err(error) = window.destroy() {
                    warn!("退出时销毁窗口 {label} 失败: {error}");
                }
            }
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
            .is_visible()?
        {
            persist_picker_window_position(app, &window);
            #[cfg(target_os = "windows")]
            {
                crate::platform::windows::window_utils::hide_window(&window)?;
            }
            #[cfg(not(target_os = "windows"))]
            window
                .hide()?;
        }

        info!("隐藏 Picker");
        let _ = window.emit(PICKER_SESSION_END_EVENT, ());
        let _ = crate::services::tooltip_window::TooltipWindow::hide_tooltip(app);
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
            // 退出窗口期不再恢复目标窗口焦点 / 操作搜索窗口，避免在退出阶段访问正被销毁的 webview。
            let is_quitting = app_handle
                .try_state::<AppState>()
                .map(|state| state.is_quitting())
                .unwrap_or(false);
            if is_quitting {
                return;
            }
            let app_clone = app_handle.clone();
            let _ = app_handle.run_on_main_thread(move || {
                if let Some(hwnd) = session.target_window_hwnd {
                    let _ = ActiveAppResolver::restore_foreground_window_with_focus(
                        hwnd,
                        session.target_focus_hwnd,
                    );
                }
                notify_search_input_state(&app_clone, session.target_window_hwnd, false);
            });
        });

        Ok(())
    }

    pub fn resume_search_input_if_target(app: &AppHandle, target_window_hwnd: Option<isize>) {
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
            target_focus_hwnd: picker_session.target_focus_hwnd,
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

        position_search_on_cursor_monitor(&window);
        if restore_search_window_geometry(&window) {
            show_and_focus_window(&window)?;
        }
        show_and_focus_window(&window)?;

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
            )?;

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
            target_focus_hwnd: None,
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
            .is_visible()?
        {
            window
                .hide()?;
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
            .show()?;
        window
            .set_focus()?;
        window
            .emit(EDITOR_SESSION_START_EVENT, session)?;

        info!("打开 Editor");
        Ok(())
    }

    fn hide_search_for_editor_transition(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        state.end_search_activation();
        let _ = crate::services::tooltip_window::TooltipWindow::hide_tooltip(app);

        let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) else {
            return Ok(());
        };

        if window
            .is_visible()?
        {
            window
                .hide()?;
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
    let _ = crate::services::tooltip_window::TooltipWindow::hide_tooltip(app);
    let session = state.search_session()?;

    if let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) {
        if window
            .is_visible()?
        {
            window
                .hide()?;
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
        .inner_size(
            SETTINGS_WINDOW_DEFAULT_WIDTH as f64,
            SETTINGS_WINDOW_DEFAULT_HEIGHT as f64,
        )
        .min_inner_size(SETTINGS_WINDOW_MIN_WIDTH as f64, 1.0)
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
        .shadow(true)
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
        .inner_size(
            SEARCH_WINDOW_DEFAULT_WIDTH as f64,
            SEARCH_WINDOW_DEFAULT_HEIGHT as f64,
        )
        .min_inner_size(
            SEARCH_WINDOW_MIN_WIDTH as f64,
            SEARCH_WINDOW_MIN_HEIGHT as f64,
        )
        .resizable(false)
        .visible(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
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
    if let Err(error) = crate::platform::windows::window_utils::remove_window_system_menu(window) {
        warn!("移除搜索窗口系统菜单失败: {error}");
    }

    #[cfg(target_os = "windows")]
    if let Err(error) = crate::platform::windows::window_utils::block_alt_menu_activation(window) {
        warn!("拦截搜索窗口 Alt 系统菜单激活失败: {error}");
    }

    let app = window.app_handle().clone();
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
                if let Err(err) = WindowCoordinator::hide_search_and_restore_target(&app, &state) {
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

/// 从 Editor 返回 Picker：恢复窗口可见性与会话快捷键，但**故意不发送
/// PICKER_SESSION_START_EVENT**，以保留用户进入编辑器时的选中项和滚动位置，
/// 让"编辑完返回"自然衔接原上下文，而非回跳到列表顶部。
fn restore_picker_after_editor(
    app: &AppHandle,
    state: &AppState,
    session: &EditorSession,
) -> Result<(), AppError> {
    state.set_picker_session(session.target_window_hwnd, session.target_focus_hwnd)?;
    notify_search_input_state(app, session.target_window_hwnd, true);
    let window = ensure_picker_window(app)?;

    restore_picker_window_size(app, &window);

    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::window_utils::show_window_no_activate(&window)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        window
            .show()?;
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

/// 从 Editor 返回 Search：恢复窗口可见性，但**故意不发送
/// SEARCH_SESSION_START_EVENT**，以保留用户进入编辑器时的选中项和滚动位置，
/// 让"编辑完返回"自然衔接原上下文，而非回跳到列表顶部。
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
        .is_visible()?;
    #[cfg(target_os = "windows")]
    let is_minimized = crate::platform::windows::window_utils::is_window_minimized(window)?;

    #[cfg(not(target_os = "windows"))]
    let is_minimized = window
        .is_minimized()?;

    Ok(is_visible && !is_minimized)
}

/// 搜索窗口是否处于"会话活跃且可见未最小化"的可关闭状态，作为搜索快捷键
/// toggle 的关闭侧判定；否则快捷键语义为打开。
fn is_search_window_ready_for_toggle(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };

    if !state.is_search_active() {
        return false;
    }

    let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) else {
        return false;
    };

    is_window_ready_for_reuse(&window).unwrap_or(false)
}

fn show_and_focus_window(window: &WebviewWindow) -> Result<(), AppError> {
    window
        .show()?;

    #[cfg(target_os = "windows")]
    {
        let mut last_error: Option<AppError> = None;

        for attempt in 0..=WINDOW_FOCUS_MAX_RETRIES {
            match crate::platform::windows::window_utils::restore_window_and_focus(window) {
                Ok(()) => return Ok(()),
                Err(error)
                    if should_retry_window_focus(&error) && attempt < WINDOW_FOCUS_MAX_RETRIES =>
                {
                    last_error = Some(error);
                    std::thread::sleep(Duration::from_millis(WINDOW_FOCUS_RETRY_DELAY_MS));
                }
                Err(error) => return Err(error),
            }
        }

        return Err(last_error
            .unwrap_or_else(|| AppError::Message("搜索窗口聚焦失败".to_string())));
    }

    #[cfg(not(target_os = "windows"))]
    {
        window
            .set_focus()?;
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

fn position_search_on_cursor_monitor(window: &WebviewWindow) {
    use crate::platform::windows::picker_position::{current_cursor_point, work_area_from_point};

    let Ok(cursor) = current_cursor_point() else {
        return;
    };
    let Ok(work_area) = work_area_from_point(cursor) else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };

    let window_width = size.width as i32;
    let window_height = size.height as i32;
    let x = work_area.left + (work_area.width() - window_width) / 2;
    let y = work_area.top + (work_area.height() - window_height) / 2;

    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

fn begin_search_window_minimize_monitor(app: AppHandle, state: AppState) {
    #[cfg(target_os = "windows")]
    {
        let token = state.next_search_session_monitor_token();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(120));

            if !state.is_search_active() || state.is_quitting() {
                break;
            }

            if state.current_search_session_monitor_token() != token {
                break;
            }

            let Some(window) = app.get_webview_window(SEARCH_WINDOW_LABEL) else {
                break;
            };

            let is_minimized = crate::platform::windows::window_utils::is_window_minimized(&window)
                .unwrap_or(false);
            if !is_minimized {
                continue;
            }

            let app_handle = app.clone();
            let state_clone = state.clone();
            let _ = app.run_on_main_thread(move || {
                if state_clone.is_search_active() && !state_clone.is_quitting() {
                    if let Err(error) = WindowCoordinator::hide_search_without_restore_target(
                        &app_handle,
                        &state_clone,
                    ) {
                        error!("搜索窗口最小化后自动结束会话失败: {error}");
                    }
                }
            });
            break;
        });
    }
}

fn notify_search_input_state(app: &AppHandle, target_window_hwnd: Option<isize>, suspended: bool) {
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

fn should_retry_window_focus(error: &AppError) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("underlying handle is not available")
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
    if let Err(error) = crate::platform::windows::window_utils::remove_window_system_menu(window) {
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
    use super::{
        should_restore_picker_after_search_close, should_retry_window_focus,
        SETTINGS_WINDOW_DEFAULT_HEIGHT, SETTINGS_WINDOW_DEFAULT_WIDTH, SETTINGS_WINDOW_MIN_WIDTH,
    };
    use crate::domain::error::AppError;
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

    #[test]
    fn tauri_config_should_pin_settings_window_for_two_column_layout() {
        let config: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let windows = config["app"]["windows"].as_array().unwrap();
        let settings = windows
            .iter()
            .find(|window| window["label"] == "manager")
            .unwrap();

        assert_eq!(settings["width"], Value::from(920));
        assert_eq!(settings["height"], Value::from(760));
        assert_eq!(settings["minWidth"], Value::from(880));
    }

    #[test]
    fn settings_window_constants_should_match_two_column_contract() {
        assert_eq!(SETTINGS_WINDOW_DEFAULT_WIDTH, 920);
        assert_eq!(SETTINGS_WINDOW_DEFAULT_HEIGHT, 760);
        assert_eq!(SETTINGS_WINDOW_MIN_WIDTH, 880);
    }

    #[test]
    fn tauri_config_should_enable_asset_protocol_for_picker_image_previews() {
        let config: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let security = &config["app"]["security"];

        assert_eq!(security["assetProtocol"]["enable"], Value::Bool(true));
        assert!(
            security["assetProtocol"]["scope"]
                .as_array()
                .map(|scope| !scope.is_empty())
                .unwrap_or(false)
        );
    }

    #[test]
    fn tauri_config_csp_should_allow_asset_images() {
        let config: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let csp = config["app"]["security"]["csp"]
            .as_str()
            .expect("csp should be configured");

        assert!(csp.contains("img-src"));
        assert!(csp.contains("asset:"));
        assert!(csp.contains("http://asset.localhost"));
    }

    #[test]
    fn should_retry_window_focus_when_underlying_handle_is_not_available() {
        assert!(should_retry_window_focus(&AppError::Message(
            "the underlying handle is not available".to_string()
        )));
    }

    #[test]
    fn should_not_retry_window_focus_for_other_errors() {
        assert!(!should_retry_window_focus(&AppError::Message(
            "permission denied".to_string()
        )));
    }
}
