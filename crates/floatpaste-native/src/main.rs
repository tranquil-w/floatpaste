// release 不带控制台窗口（日志落文件，debug 保留终端便于排查）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! FloatPaste 原生壳（Slint + 软件渲染）。
//!
//! 与老 Tauri 壳共用 floatpaste-core 与同一数据目录。本阶段完成速贴面板、
//! 搜索窗口、编辑窗口、设置窗口与系统托盘的复刻（无焦点会话模型 / 长按
//! 导航 / 外击关闭 / 悬停预览 / 三种定位 / 尺寸记忆 / 主题 / 搜索会话 /
//! 两段式删除 / 文本编辑与标签管理 / 设置防抖自动保存与运行时联动）。

mod app_icon;
mod app_state;
mod editor;
mod overlay;
mod paste_flow;
mod picker;
mod search;
mod settings;
mod system;
mod theme_bridge;
mod thumbnails;
mod tooltip;
mod tray;
mod win32_ext;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use slint::ComponentHandle;

use floatpaste_core::launch_mode::LaunchMode;
use floatpaste_core::platform::windows::clipboard_monitor::ClipboardMonitor;
use floatpaste_core::platform::windows::hotkey;
use floatpaste_core::platform::windows::mouse_monitor;
use floatpaste_core::platform::windows::session_keyboard;
use floatpaste_core::platform::windows::winv_takeover;
use floatpaste_core::theme;

use app_state::SharedState;
use picker::App;

slint::include_modules!();

/// ERROR_HOTKEY_ALREADY_REGISTERED：组合键已被其他程序（或系统）注册
const ERROR_HOTKEY_OCCUPIED: u32 = 1409;

fn main() {
    // Slint 内置开关：Windows 上隐藏窗口即销毁 winit 窗口与软渲染帧缓冲，
    // 下次显示自动重建。编辑/设置窗口关闭即回收内存；overlay 装配窗口
    // （速贴/搜索/tooltip）显隐走停屏与 Win32 路径，不触发此行为
    std::env::set_var("SLINT_DESTROY_WINDOW_ON_HIDE", "1");
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
    // 编辑窗口：普通带框窗（任务栏可见、可缩放），启动即建、按需显示
    let editor_win = match EditorWindow::new() {
        Ok(win) => win,
        Err(error) => {
            tracing::error!("创建编辑窗口失败: {error}");
            return;
        }
    };
    // 设置窗口：普通带框窗（同旧版 label=manager），启动即建、按需显示
    let settings_win = match SettingsWindow::new() {
        Ok(win) => win,
        Err(error) => {
            tracing::error!("创建设置窗口失败: {error}");
            return;
        }
    };

    let state = Arc::new(SharedState::new(core));

    let app = App {
        state: state.clone(),
        picker: picker_win.as_weak(),
        tooltip: tooltip_win.as_weak(),
        search: search_win.as_weak(),
        editor: editor_win.as_weak(),
        settings: settings_win.as_weak(),
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

    picker::wire(&app);
    search::wire(&app);
    editor::wire(&app);
    settings::wire(&app);

    // ── 窗口句柄与浮层样式：事件循环首轮装配（winit 惰性建窗，
    // show/停屏舞蹈统一在 overlay::silent_assemble）──
    // 主题写入放在停屏之后：属性变化使窗口变脏，Slint 在屏外完成首帧
    // 渲染与呈现——表面内容就绪后，任何上屏移动都立即有内容，开窗不闪
    // 透明（写在建窗之前不会触发屏外渲染，表面是空的）
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
        if let Some(hwnd) = overlay::silent_assemble_parked(&tooltip_win, false) {
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

        let settings = app_for_init.state.current_settings();
        let resolved =
            theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
        let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
        let editor_for_theme = app_for_init.editor.upgrade();
        let settings_for_theme = app_for_init.settings.upgrade();
        theme_bridge::apply_theme(
            Some(&picker_win),
            Some(&tooltip_win),
            Some(&search_win),
            editor_for_theme.as_ref(),
            settings_for_theme.as_ref(),
            &tokens,
        );

        // 主题写入后同步暖一次表面：停屏窗口收不到自发 WM_PAINT，
        // 不主动泵一次呈现，上屏首帧会是透明的
        win32_ext::warm_surface(app_for_init.state.picker_hwnd.load(Ordering::SeqCst));
        win32_ext::warm_surface(app_for_init.state.search_hwnd.load(Ordering::SeqCst));

        if !silent_startup {
            // 延一轮事件循环再打开：先让停屏窗口在屏外完成首帧渲染，
            // 移回屏上时表面已有有效内容，首开不闪透明
            let app_for_activate = app_for_init.clone();
            slint::Timer::single_shot(std::time::Duration::from_millis(0), move || {
                picker::activate(&app_for_activate);
            });
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

    // ── 全局快捷键：主快捷键切换速贴，搜索快捷键切换搜索窗口；
    //    托盘常驻（设置窗口「打开设置」左键单击与右键菜单）──
    sync_global_hotkeys(&app);
    tray::start(app.clone());

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

/// 注册全局快捷键（主快捷键=速贴开关，搜索快捷键=搜索开关，Win+V 接管
/// 开启时追加速贴唤起入口）。设置保存后的重注册复用此函数：旧热键线程
/// 的注销是异步的（stop 后须等其消息循环退出才释放组合键），注册失败按
/// 150ms 重试三次；末次仍有失败的 id 记入 state 供设置页展示
/// （1409=组合被其他程序占用，用户可见反馈）。
pub(crate) fn sync_global_hotkeys(app: &App) {
    let settings = app.state.current_settings();
    let shortcut_text = {
        let configured = settings.shortcut;
        if configured.trim().is_empty() {
            "Ctrl+Q".to_string()
        } else {
            configured
        }
    };
    let search_shortcut_text = settings.search_shortcut;
    let search_shortcut_enabled = settings.search_shortcut_enabled;
    // 快捷键无法解析时只跳过注册，不退出进程：剪贴板监听与
    // 二次启动唤起仍可用
    let main_spec =
        hotkey::parse_hotkey(&shortcut_text).or_else(|| hotkey::parse_hotkey("Ctrl+Q"));
    let Some(main_spec) = main_spec else {
        tracing::error!("主快捷键无法解析（配置值与默认值均失败），跳过注册");
        return;
    };
    let mut specs: Vec<(u32, hotkey::HotkeySpec)> = vec![(1, main_spec)];
    // 搜索快捷键与主快捷键相同时跳过（同组合键 RegisterHotKey 必失败）
    if search_shortcut_enabled {
        match hotkey::parse_hotkey(&search_shortcut_text) {
            Some(search_spec) if search_spec != main_spec => {
                specs.push((2, search_spec));
            }
            Some(_) => tracing::warn!("搜索快捷键与主快捷键相同，跳过注册"),
            None => tracing::warn!("搜索快捷键无法解析，跳过注册"),
        }
    }
    // Win+V 接管（ADR-0001）：先自愈注册表（设置开启但值被外部清除时
    // 补写）；与主/搜索快捷键相同则不注册（失败记录会让设置页给出提示）
    let winv_spec = hotkey::parse_hotkey("Win+V");
    if settings.takeover_winv {
        match winv_takeover::is_enabled_in_registry() {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = winv_takeover::enable_in_registry() {
                    tracing::warn!("补写 Win+V 接管注册表失败: {error}");
                }
            }
            Err(error) => tracing::warn!("读取 Win+V 接管注册表状态失败: {error}"),
        }
        let occupied = |spec: &hotkey::HotkeySpec| {
            specs.iter().any(|(_, existing)| existing == spec)
        };
        match winv_spec {
            Some(spec) if !occupied(&spec) => specs.push((3, spec)),
            Some(_) => {
                tracing::warn!("Win+V 与已有全局快捷键相同，跳过接管注册");
                app.state
                    .set_hotkey_failures(vec![(3, ERROR_HOTKEY_OCCUPIED)]);
            }
            None => tracing::warn!("Win+V 组合无法解析，跳过接管注册"),
        }
    }

    let mut last_failures: Vec<(u32, u32)> = Vec::new();
    let mut last_hard_error = false;
    for attempt in 0..3 {
        hotkey::stop_hotkeys();
        let app_for_hotkey = app.clone();
        let on_trigger = move |id: u32| {
            tracing::info!("命中全局快捷键 id={id}");
            let app = app_for_hotkey.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if id == 2 {
                    search::toggle_from_shortcut(&app);
                } else {
                    // 主快捷键与 Win+V 接管均打开速贴面板
                    picker::toggle(&app);
                }
            });
        };
        match hotkey::register_hotkeys(specs.clone(), on_trigger) {
            Ok(outcome) if outcome.failed.is_empty() => {
                app.state.set_hotkey_failures(Vec::new());
                return;
            }
            Ok(outcome) => {
                // 部分失败：可用热键照常工作；重试等旧线程退出释放
                // 组合键（新一轮 stop 已在循环头执行），末次结果记录
                last_failures = outcome.failed;
                last_hard_error = false;
            }
            // 线程启动失败等硬错误：保留鼠标路径
            Err(error) if attempt < 2 => {
                tracing::warn!("注册全局快捷键失败（第 {} 次）：{error}", attempt + 1);
                last_hard_error = true;
                std::thread::sleep(std::time::Duration::from_millis(150));
                continue;
            }
            Err(error) => {
                tracing::error!("注册全局快捷键失败: {error}");
                last_hard_error = true;
            }
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }
    if last_hard_error {
        last_failures = specs.iter().map(|(id, _)| (*id, 0)).collect();
    }
    app.state.set_hotkey_failures(last_failures);
}
