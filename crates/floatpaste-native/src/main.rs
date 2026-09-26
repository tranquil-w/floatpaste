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

use floatpaste_core::domain::error::AppError;
use floatpaste_core::launch_mode::{self, LaunchMode};
use floatpaste_core::platform::windows::clipboard_monitor::ClipboardMonitor;
use floatpaste_core::platform::windows::elevated_task;
use floatpaste_core::platform::windows::elevation;
use floatpaste_core::platform::windows::hotkey;
use floatpaste_core::platform::windows::mouse_monitor;
use floatpaste_core::platform::windows::session_keyboard;
use floatpaste_core::platform::windows::single_instance;
use floatpaste_core::platform::windows::winv_takeover;
use floatpaste_core::theme;

use app_state::SharedState;
use picker::App;

slint::include_modules!();

/// ERROR_HOTKEY_ALREADY_REGISTERED：组合键已被其他程序（或系统）注册
pub(crate) const ERROR_HOTKEY_OCCUPIED: u32 = 1409;
/// 哨兵码：Win+V 与本应用主/搜索快捷键组合相同而跳过注册（非系统占用，
/// 与 1409 区分开供设置页给出不同指引）
pub(crate) const HOTKEY_ERROR_WINV_SELF_CONFLICT: u32 = u32::MAX;
/// 全局快捷键 id：速贴唤起 / 搜索窗口 / Win+V 接管（均唤起速贴面板）
pub(crate) const HOTKEY_ID_MAIN: u32 = 1;
pub(crate) const HOTKEY_ID_SEARCH: u32 = 2;
pub(crate) const HOTKEY_ID_WINV: u32 = 3;

fn main() {
    // Slint 内置开关：Windows 上隐藏窗口即销毁 winit 窗口与软渲染帧缓冲，
    // 下次显示自动重建。编辑/设置窗口关闭即回收内存；overlay 装配窗口
    // （速贴/搜索/tooltip）显隐走停屏与 Win32 路径，不触发此行为
    std::env::set_var("SLINT_DESTROY_WINDOW_ON_HIDE", "1");
    let _log_guard = system::init_logging();
    install_panic_hook();

    // 提权辅助路径（UAC 重入自身）：完成管理员自启任务的注册/卸载后以
    // 退出码报告结果。必须先于单实例检查——旧实例正持锁等待本进程结果
    let args: Vec<String> = std::env::args().collect();
    if let Some(exit_code) = run_elevated_sidecar(&args) {
        std::process::exit(exit_code);
    }

    // 提权重启：新实例（经 UAC 启动）先等旧实例释放单实例互斥量再正常
    // 接管；等待超时则照常继续，由单实例检查兜底（唤醒旧实例）
    if args
        .iter()
        .any(|arg| arg == launch_mode::ELEVATED_RELAUNCH_ARG)
    {
        single_instance::wait_mutex_release(std::time::Duration::from_secs(5));
    }

    let launch_mode = LaunchMode::from_env();

    // 单实例：已有实例时通过命名事件唤醒其速贴会话并退出当前进程
    let _single_instance = match single_instance::acquire_or_focus_existing(launch_mode, || {
        single_instance::signal_wake_event()
    }) {
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
        let editor_win = app_for_init.editor.upgrade();
        let settings_win = app_for_init.settings.upgrade();

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
            // 穿透位守护：winit 异步样式重排会剥掉 TRANSPARENT/LAYERED，
            // 剥掉后 tooltip 盖在条目上吞点击（点铅笔无响应的根因）
            win32_ext::install_click_through_subclass(hwnd);
        }
        // 搜索窗口可聚焦（需要真实键盘焦点），装配变体不带 NOACTIVATE
        match overlay::silent_assemble_focusable(&search_win) {
            Some(hwnd) => {
                app_for_init.state.search_hwnd.store(hwnd, Ordering::SeqCst);
            }
            None => tracing::error!("获取搜索窗口句柄失败，搜索会话不可用"),
        }
        // 编辑/设置窗：内容窗停屏装配（带系统标题栏；屏外保持 Slint 可见
        // 与表面有效，显隐走平移不销毁重建）
        if let Some(win) = editor_win.as_ref() {
            match overlay::silent_assemble_content(win) {
                Some(hwnd) => {
                    app_for_init.state.editor_hwnd.store(hwnd, Ordering::SeqCst);
                    app_icon::apply_window_icon(hwnd);
                }
                None => tracing::error!("获取编辑窗口句柄失败，编辑会话不可用"),
            }
        }
        if let Some(win) = settings_win.as_ref() {
            match overlay::silent_assemble_content(win) {
                Some(hwnd) => {
                    app_for_init
                        .state
                        .settings_hwnd
                        .store(hwnd, Ordering::SeqCst);
                    app_icon::apply_window_icon(hwnd);
                }
                None => tracing::error!("获取设置窗口句柄失败，设置会话不可用"),
            }
        }

        // 最小化恢复 → 重挂材质：DWM 偶发不重新应用 SystemBackdrop（用户
        // 实测编辑窗最小化再恢复后整窗失材质）。两个内容窗共用一个分派
        // 闭包，按 hwnd 路由
        let app_for_restore = app_for_init.clone();
        let restore_material = move |hwnd: isize| {
            let app = app_for_restore.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if hwnd == app.state.editor_hwnd.load(Ordering::SeqCst) {
                    if let Some(win) = app.editor.upgrade() {
                        win.set_material_active(overlay::apply_material(&app, hwnd, overlay::MaterialSurface::Content));
                    }
                } else if hwnd == app.state.settings_hwnd.load(Ordering::SeqCst) {
                    if let Some(win) = app.settings.upgrade() {
                        win.set_material_active(overlay::apply_material(&app, hwnd, overlay::MaterialSurface::Content));
                    }
                }
            });
        };
        // hwnd 非零即装配成功（装配失败时保持 0），可直接据此安装
        let editor_hwnd = app_for_init.state.editor_hwnd.load(Ordering::SeqCst);
        if editor_hwnd != 0 {
            win32_ext::install_material_restore_subclass(editor_hwnd, restore_material.clone());
        }
        let settings_hwnd = app_for_init.state.settings_hwnd.load(Ordering::SeqCst);
        if settings_hwnd != 0 {
            win32_ext::install_material_restore_subclass(settings_hwnd, restore_material.clone());
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
    // 启动时同步一次自启任务（Run 键存量迁移 + 任务缺失自愈；不弹 UAC，
    // 缺提权任务时留待用户改动设置时确认）
    settings::spawn_autostart_sync(&app, false);
    tray::start(app.clone());

    // 「始终以管理员身份运行」启动期自检：任务计划只覆盖登录自启，手动
    // 启动（图标/命令行）经 asInvoker manifest 拿不到提权。预期提权而
    // 实际未提权时经 UAC 重入自身（复用 --elevated-relaunch 闭环：本
    // 进程退出释放单实例互斥量，提权新实例接管）；UAC 取消则照常以
    // 普通权限运行，托盘气泡说明一次
    if launch_mode::needs_elevated_relaunch(
        app.state.current_settings().always_run_elevated,
        elevation::is_current_process_elevated(),
        &args,
    ) {
        // 透传原启动参数（--silent 等），提权接管后保持同一启动模式
        let mut relaunch_args: Vec<String> = args.iter().skip(1).cloned().collect();
        relaunch_args.push(launch_mode::ELEVATED_RELAUNCH_ARG.to_string());
        match elevation::relaunch_elevated(&relaunch_args.join(" ")) {
            Ok(()) => {
                tracing::info!("提权重启已获确认，退出当前实例交由提权实例接管");
                session_keyboard::end_session();
                mouse_monitor::end_session();
                ClipboardMonitor::stop();
                hotkey::stop_hotkeys();
                app.state.core.begin_quit();
                return;
            }
            Err(error) => {
                tracing::warn!("提权重启未获确认，继续以普通权限运行: {error}");
                tray::notify_elevation_declined();
            }
        }
    }

    // 必须用 until_quit 变体：Slint 默认在最后一个窗口关闭/隐藏时退出事件循环，
    // 而"隐藏窗口"是速贴应用的常态操作（Esc/粘贴/热键），会让整个进程静默退出。
    // 正常返回也要留痕：托盘常驻场景下"事件循环悄悄返回"与"进程崩溃"在
    // 用户侧都表现为快捷键全灭，日志必须能区分两者
    match slint::run_event_loop_until_quit() {
        Ok(()) => tracing::info!("事件循环已返回（收到 quit 或窗口 keepalive 归零），进入退出收尾"),
        Err(error) => tracing::error!("事件循环异常退出: {error}"),
    }

    // 退出收尾：先停输入拦截与监听，再置退出标志
    session_keyboard::end_session();
    mouse_monitor::end_session();
    ClipboardMonitor::stop();
    hotkey::stop_hotkeys();
    state.core.begin_quit();
}

/// panic 默认钩子只写 stderr：release 是 windows_subsystem 应用无控制台，
/// panic 痕迹随 stderr 丢失，进程死得无声（日志戛然而止无法与崩溃区分）。
/// 这里转写 tracing 落日志文件；stderr 照写一份保住 debug 终端输出
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|location| format!("{}:{}:{}", location.file(), location.line(), location.column()))
            .unwrap_or_else(|| "未知位置".to_string());
        let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "非字符串载荷".to_string()
        };
        tracing::error!("panic 于 {location}: {payload}");
        eprintln!("panic 于 {location}: {payload}");
    }));
}

/// UAC 重入自身的辅助路径：按参数执行自启任务的注册/删除，退出码 0/1
/// 报告结果（等待方 run_elevated_and_wait 读取）。返回 Some(码) 表示本次
/// 进程就是辅助进程，不进 GUI。
fn run_elevated_sidecar(args: &[String]) -> Option<i32> {
    // 删除兜底：本地删除失败（任务 ACL 异常等）时经 UAC 重入删除
    if args
        .iter()
        .any(|arg| arg == launch_mode::REMOVE_ELEVATED_AUTOSTART_ARG)
    {
        let result = elevated_task::uninstall();
        if let Err(error) = &result {
            tracing::error!("自启任务删除失败: {error}");
        }
        return Some(if result.is_ok() { 0 } else { 1 });
    }
    if !args
        .iter()
        .any(|arg| arg == launch_mode::SETUP_ELEVATED_AUTOSTART_ARG)
    {
        return None;
    }
    // 注册 HIGHEST 任务（需提权）；任务动作参数 = SETUP 标记之外的剩余
    // 参数（当前只有 --silent，对应「开机时静默启动」设置）
    let task_arguments = args
        .iter()
        .filter(|arg| arg.as_str() != launch_mode::SETUP_ELEVATED_AUTOSTART_ARG)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let result = std::env::current_exe()
        .map_err(AppError::from)
        .and_then(|exe| elevated_task::install(&exe.to_string_lossy(), &task_arguments, true));
    if let Err(error) = &result {
        tracing::error!("自启任务注册失败: {error}");
    }
    Some(if result.is_ok() { 0 } else { 1 })
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
    let main_spec = hotkey::parse_hotkey(&shortcut_text).or_else(|| hotkey::parse_hotkey("Ctrl+Q"));
    let Some(main_spec) = main_spec else {
        tracing::error!("主快捷键无法解析（配置值与默认值均失败），跳过注册");
        return;
    };
    let mut specs: Vec<(u32, hotkey::HotkeySpec)> = vec![(HOTKEY_ID_MAIN, main_spec)];
    // 搜索快捷键与主快捷键相同时跳过（同组合键 RegisterHotKey 必失败）
    if search_shortcut_enabled {
        match hotkey::parse_hotkey(&search_shortcut_text) {
            Some(search_spec) if search_spec != main_spec => {
                specs.push((HOTKEY_ID_SEARCH, search_spec));
            }
            Some(_) => tracing::warn!("搜索快捷键与主快捷键相同，跳过注册"),
            None => tracing::warn!("搜索快捷键无法解析，跳过注册"),
        }
    }
    // Win+V 接管（ADR-0001）：先自愈注册表（设置开启但值被外部清除时
    // 补写）；与主/搜索快捷键相同则不注册（失败记录让设置页给出
    // 「与自身快捷键冲突」指引，区别于系统占用）
    let mut winv_self_conflict = false;
    if settings.takeover_winv {
        let winv_spec = hotkey::parse_hotkey("Win+V");
        match winv_takeover::is_enabled_in_registry() {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = winv_takeover::enable_in_registry() {
                    tracing::warn!("补写 Win+V 接管注册表失败: {error}");
                }
            }
            Err(error) => tracing::warn!("读取 Win+V 接管注册表状态失败: {error}"),
        }
        let occupied =
            |spec: &hotkey::HotkeySpec| specs.iter().any(|(_, existing)| existing == spec);
        match winv_spec {
            Some(spec) if !occupied(&spec) => specs.push((HOTKEY_ID_WINV, spec)),
            Some(_) => {
                tracing::warn!("Win+V 与已有全局快捷键相同，跳过接管注册");
                winv_self_conflict = true;
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
                // 不做命中防抖：MOD_NOREPEAT 已在注册层过滤按住连发，
                // 人工连按每击必响应（开/关由击数次序决定）
                if id == HOTKEY_ID_SEARCH {
                    search::toggle_from_shortcut(&app);
                } else {
                    // 主快捷键与 Win+V 接管均打开速贴面板
                    picker::toggle(&app);
                }
            });
        };
        match hotkey::register_hotkeys(specs.clone(), on_trigger) {
            Ok(outcome) if outcome.failed.is_empty() => {
                last_failures.clear();
                last_hard_error = false;
                break;
            }
            Ok(outcome) => {
                // 部分失败：可用热键照常工作；重试等旧线程退出释放
                // 组合键（新一轮 stop 已在循环头执行），末次结果记录。
                // 连续两轮失败集相同 = 非瞬态（如系统未释放 Win+V），
                // 不再空转第三轮
                if outcome.failed == last_failures {
                    break;
                }
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
    // 自身冲突的跳过项不进 specs，注册循环覆盖不到，统一在出口并入
    if winv_self_conflict {
        last_failures.push((HOTKEY_ID_WINV, HOTKEY_ERROR_WINV_SELF_CONFLICT));
    }
    app.state.set_hotkey_failures(last_failures);
}
