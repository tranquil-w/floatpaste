//! FloatPaste 原生壳（Slint + 软件渲染）。
//!
//! 与老 Tauri 壳共用 floatpaste-core 与同一数据目录。本阶段完成速贴面板与
//! 搜索窗口的完整复刻（无焦点会话模型 / 长按导航 / 外击关闭 / 悬停预览 /
//! 三种定位 / 尺寸记忆 / 主题 / 搜索会话 / 两段式删除），editor/settings/
//! tray 按窗口逐个迁移。

mod app_state;
mod overlay;
mod paste_flow;
mod picker;
mod search;
mod system;
mod theme_bridge;
mod thumbnails;
mod tooltip;
mod win32_ext;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use slint::ComponentHandle;

use floatpaste_core::launch_mode::LaunchMode;
use floatpaste_core::platform::windows::clipboard_monitor::ClipboardMonitor;
use floatpaste_core::platform::windows::hotkey;
use floatpaste_core::platform::windows::mouse_monitor;
use floatpaste_core::platform::windows::session_keyboard;
use floatpaste_core::platform::windows::window_control::{self, GestureMode, ResizeDirection};
use floatpaste_core::services::picker_position_service::{PICKER_MIN_HEIGHT, PICKER_MIN_WIDTH};
use floatpaste_core::theme;

use app_state::SharedState;
use picker::App;

slint::include_modules!();

fn main() {
    let _log_guard = system::init_logging();
    let launch_mode = LaunchMode::from_env();

    // 单实例：已有实例时通过命名事件唤醒其速贴会话并退出当前进程
    let _single_instance =
        match floatpaste_core::platform::windows::single_instance::acquire_or_focus_existing(
            launch_mode,
            || floatpaste_core::platform::windows::single_instance::signal_wake_event(),
        ) {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => return,
            Err(error) => {
                tracing::error!("单实例检查失败，退出当前实例: {error}");
                return;
            }
        };

    let core = match system::init_core() {
        Ok(core) => core,
        Err(error) => {
            tracing::error!("初始化核心状态失败: {error}");
            return;
        }
    };

    if let Err(error) =
        floatpaste_core::services::history_service::HistoryService::ensure_welcome_item(
            &core.repository,
        )
    {
        tracing::warn!("初始化欢迎记录失败: {error}");
    }

    // ── 窗口创建 ──
    let picker_win = match QuickPasteWindow::new() {
        Ok(win) => win,
        Err(error) => {
            tracing::error!("创建速贴窗口失败: {error}");
            return;
        }
    };
    let tooltip_win = match TooltipWindow::new() {
        Ok(win) => win,
        Err(error) => {
            tracing::error!("创建预览窗口失败: {error}");
            return;
        }
    };
    let search_win = match SearchWindow::new() {
        Ok(win) => win,
        Err(error) => {
            tracing::error!("创建搜索窗口失败: {error}");
            return;
        }
    };

    let state = Arc::new(SharedState::new(core));

    let app = App {
        state: state.clone(),
        picker: picker_win.as_weak(),
        tooltip: tooltip_win.as_weak(),
        search: search_win.as_weak(),
    };

    // ── 二次启动唤醒：打开速贴会话（等价于按下主快捷键）──
    {
        let app_for_wake = app.clone();
        if let Err(error) =
            floatpaste_core::platform::windows::single_instance::listen_wake(move || {
                let app = app_for_wake.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    picker::toggle(&app);
                });
            })
        {
            tracing::warn!("装载唤醒事件失败，二次启动将无法唤起面板: {error}");
        }
    }

    // ── 主题初值（首帧即正确，无需等会话）──
    {
        let settings = state.current_settings();
        let resolved =
            theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
        let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
        theme_bridge::apply_theme(&picker_win, Some(&tooltip_win), Some(&search_win), &tokens);
    }

    wire_picker_callbacks(&app);
    wire_search_callbacks(&app);

    // ── 窗口句柄与浮层样式：事件循环首轮装配（winit 惰性建窗，
    // show/hide 舞蹈统一在 overlay::silent_assemble）──
    let silent_startup = launch_mode.is_silent();
    let app_for_init = app.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(picker_win) = app_for_init.picker.upgrade() else {
            return;
        };
        let Some(tooltip_win) = app_for_init.tooltip.upgrade() else {
            return;
        };
        let Some(search_win) = app_for_init.search.upgrade() else {
            return;
        };

        match overlay::silent_assemble(&picker_win, true) {
            Some(hwnd) => {
                app_for_init.state.picker_hwnd.store(hwnd, Ordering::SeqCst);
            }
            None => tracing::error!("获取速贴窗口句柄失败，会话功能不可用"),
        }
        if let Some(hwnd) = overlay::silent_assemble(&tooltip_win, false) {
            app_for_init
                .state
                .tooltip_hwnd
                .store(hwnd, Ordering::SeqCst);
        }
        // 搜索窗口可聚焦（需要真实键盘焦点），装配变体不带 NOACTIVATE
        match overlay::silent_assemble_focusable(&search_win) {
            Some(hwnd) => {
                app_for_init.state.search_hwnd.store(hwnd, Ordering::SeqCst);
            }
            None => tracing::error!("获取搜索窗口句柄失败，搜索会话不可用"),
        }

        if !silent_startup {
            picker::activate(&app_for_init);
        }
    });

    // ── 剪贴板监听：录入成功后同步刷新速贴列表与搜索结果 ──
    {
        let app_for_sink = app.clone();
        let on_upsert: floatpaste_core::platform::windows::clipboard_monitor::ClipUpsertSink =
            Arc::new(move |_detail| {
                let app = app_for_sink.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    picker::refresh_list_changed(&app);
                    search::notify_clips_changed(&app);
                });
            });
        if let Err(error) = ClipboardMonitor::start(state.core.clone(), on_upsert) {
            tracing::error!("启动剪贴板监听失败: {error}");
        }
    }

    // ── 全局快捷键：主快捷键切换速贴，搜索快捷键切换搜索窗口 ──
    {
        let app_for_hotkey = app.clone();
        let settings = state.current_settings();
        let shortcut_text = {
            let configured = settings.shortcut;
            if configured.trim().is_empty() {
                "Alt+Q".to_string()
            } else {
                configured
            }
        };
        let search_shortcut_text = settings.search_shortcut;
        let search_shortcut_enabled = settings.search_shortcut_enabled;
        // 快捷键无法解析时只跳过注册，不退出进程：剪贴板监听与
        // 二次启动唤起仍可用
        let main_spec =
            hotkey::parse_hotkey(&shortcut_text).or_else(|| hotkey::parse_hotkey("Alt+Q"));
        if main_spec.is_none() {
            tracing::error!("主快捷键无法解析（配置值与默认值均失败），跳过注册");
        }
        let mut hotkeys = Vec::new();
        if let Some(main_spec) = main_spec {
            hotkeys.push((1, main_spec, false));
            // 搜索快捷键与主快捷键相同时跳过（同组合键 RegisterHotKey 必失败）
            if search_shortcut_enabled {
                match hotkey::parse_hotkey(&search_shortcut_text) {
                    Some(search_spec) if search_spec != main_spec => {
                        hotkeys.push((2, search_spec, true));
                    }
                    Some(_) => tracing::warn!("搜索快捷键与主快捷键相同，跳过注册"),
                    None => tracing::warn!("搜索快捷键无法解析，跳过注册"),
                }
            }
        }
        if let Err(error) = hotkey::register_hotkeys(
            hotkeys
                .into_iter()
                .map(|(id, spec, _)| (id, spec))
                .collect(),
            move |id| {
                tracing::info!("命中全局快捷键 id={id}");
                let app = app_for_hotkey.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if id == 2 {
                        search::toggle_from_shortcut(&app);
                    } else {
                        picker::toggle(&app);
                    }
                });
            },
        ) {
            tracing::error!("注册全局快捷键失败: {error}");
        }
    }

    // 必须用 until_quit 变体：Slint 默认在最后一个窗口关闭/隐藏时退出事件循环，
    // 而"隐藏窗口"是速贴应用的常态操作（Esc/粘贴/热键），会让整个进程静默退出
    if let Err(error) = slint::run_event_loop_until_quit() {
        tracing::error!("事件循环异常退出: {error}");
    }

    // 退出收尾：先停输入拦截与监听，再置退出标志
    session_keyboard::end_session();
    mouse_monitor::end_session();
    ClipboardMonitor::stop();
    hotkey::stop_hotkeys();
    state.core.begin_quit();
}

fn wire_picker_callbacks(app: &App) {
    let win = match app.picker.upgrade() {
        Some(win) => win,
        None => return,
    };

    // 单击选中（不上屏）
    {
        let app_cb = app.clone();
        win.on_row_clicked(move |index| {
            picker::set_selected(&app_cb, index.max(0) as usize);
        });
    }

    // 双击上屏
    {
        let app_cb = app.clone();
        win.on_row_double_clicked(move |index| {
            let index = index.max(0) as usize;
            picker::set_selected(&app_cb, index);
            picker::confirm(&app_cb, index, false);
        });
    }

    // 悬停（移动即重置 400ms 计时）
    {
        let app_cb = app.clone();
        win.on_row_hover(move |index, x, y| {
            tooltip::schedule(&app_cb, index.max(0) as usize, x, y);
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_hover_left(move || {
            tooltip::cancel(&app_cb);
        });
    }

    // 列表实测预览可用宽（含窗口缩放）→ 按新宽度重裁预览
    {
        let app_cb = app.clone();
        win.on_preview_widths_changed(move || {
            picker::schedule_preview_reclamp(&app_cb);
        });
    }

    // 头部拖拽移动（非模态手势：down/moved/up 全程由 Slint 事件驱动；
    // 系统模态循环会吞掉指针抬起事件，面板此后收不到任何条目点击）
    {
        let state_cb = app.state.clone();
        win.on_header_drag_started(move || {
            let hwnd = state_cb.picker_hwnd.load(Ordering::SeqCst);
            if hwnd != 0 {
                window_control::begin_window_gesture(hwnd, GestureMode::Move, 0, 0);
            }
        });
    }
    {
        let state_cb = app.state.clone();
        win.on_header_drag_moved(move || {
            let hwnd = state_cb.picker_hwnd.load(Ordering::SeqCst);
            if hwnd != 0 {
                window_control::update_window_gesture(hwnd);
            }
        });
    }
    {
        let state_cb = app.state.clone();
        win.on_header_drag_finished(move || {
            let hwnd = state_cb.picker_hwnd.load(Ordering::SeqCst);
            if hwnd != 0 {
                window_control::end_window_gesture(hwnd);
            }
        });
    }

    // 八方向拉伸（同一非模态手势，带最小尺寸约束）
    {
        let app_cb = app.clone();
        win.on_resize_started(move |direction| {
            let hwnd = app_cb.state.picker_hwnd.load(Ordering::SeqCst);
            if hwnd == 0 {
                return;
            }
            let direction = match direction {
                0 => ResizeDirection::North,
                1 => ResizeDirection::South,
                2 => ResizeDirection::West,
                3 => ResizeDirection::East,
                4 => ResizeDirection::NorthWest,
                5 => ResizeDirection::NorthEast,
                6 => ResizeDirection::SouthWest,
                _ => ResizeDirection::SouthEast,
            };
            let scale = app_cb
                .picker
                .upgrade()
                .map(|win| win.window().scale_factor())
                .unwrap_or(1.0);
            window_control::begin_window_gesture(
                hwnd,
                GestureMode::Resize(direction),
                (PICKER_MIN_WIDTH as f32 * scale) as i32,
                (PICKER_MIN_HEIGHT as f32 * scale) as i32,
            );
        });
    }
    {
        let state_cb = app.state.clone();
        win.on_resize_moved(move || {
            let hwnd = state_cb.picker_hwnd.load(Ordering::SeqCst);
            if hwnd != 0 {
                window_control::update_window_gesture(hwnd);
            }
        });
    }
    {
        let state_cb = app.state.clone();
        win.on_resize_finished(move || {
            let hwnd = state_cb.picker_hwnd.load(Ordering::SeqCst);
            if hwnd != 0 {
                window_control::end_window_gesture(hwnd);
            }
        });
    }

    // 加载失败重试
    {
        let app_cb = app.clone();
        win.on_retry_clicked(move || {
            picker::refresh_list_changed(&app_cb);
        });
    }
}

fn wire_search_callbacks(app: &App) {
    let win = match app.search.upgrade() {
        Some(win) => win,
        None => return,
    };

    // 搜索框
    {
        let app_cb = app.clone();
        win.on_keyword_edited(move |_text| {
            search::keyword_edited(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_clear_keyword(move || {
            search::clear_keyword(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_filter_selected(move |filter| {
            search::filter_selected(&app_cb, filter);
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_toggled(move |index| {
            search::tag_toggled(&app_cb, index.max(0) as usize);
        });
    }
    {
        let app_cb = app.clone();
        win.on_clear_filters(move || {
            search::clear_filters(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_retry_clicked(move || {
            search::retry(&app_cb);
        });
    }

    // 行交互
    {
        let app_cb = app.clone();
        win.on_row_clicked(move |index| {
            search::set_selected(&app_cb, index.max(0) as usize);
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_double_clicked(move |index| {
            search::paste_index(&app_cb, index.max(0) as usize, false);
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_hover(move |index, x, y| {
            search::row_hover(&app_cb, index.max(0) as usize, x, y);
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_hover_left(move || {
            search::hover_left(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_paste(move |index| {
            search::paste_index(&app_cb, index.max(0) as usize, false);
        });
    }
    // 图片按文件粘贴的按钮只在选中行上出现，作用于当前选中
    {
        let app_cb = app.clone();
        win.on_action_paste_as_file(move || {
            search::paste_selected(&app_cb, true);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_edit(move || {
            search::edit_requested(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_toggle_favorite(move || {
            search::toggle_favorite(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_delete(move || {
            search::request_delete(&app_cb);
        });
    }

    // 键盘会话（capture 阶段拦截）
    {
        let app_cb = app.clone();
        win.on_key_navigate_up(move || {
            search::navigate(&app_cb, true);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_navigate_down(move || {
            search::navigate(&app_cb, false);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_paste(move |as_file| {
            search::paste_selected(&app_cb, as_file);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_edit(move || {
            search::edit_requested(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_toggle_favorite(move || {
            search::toggle_favorite(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_delete(move || {
            search::request_delete(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_close(move || {
            search::hide(&app_cb, false);
        });
    }

    // 触底预载 / 高度棘轮 / 头部拖拽
    {
        let app_cb = app.clone();
        win.on_near_bottom(move || {
            search::fetch_next(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_heights_changed(move || {
            search::heights_changed(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_drag_started(move || {
            search::drag_started(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_drag_moved(move || {
            search::drag_moved(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_drag_finished(move || {
            search::drag_finished(&app_cb);
        });
    }
}
