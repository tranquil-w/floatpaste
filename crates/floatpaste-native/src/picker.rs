//! 速贴面板会话逻辑：显示/隐藏/切换、列表刷新、确认上屏、收藏、删除、导航。
//!
//! 行为逐项对齐原版 WindowCoordinator / ShortcutManager / PickerShell：
//! - 显隐不抢焦点（WS_EX_NOACTIVATE，hide/show 一律以屏幕外停屏复现），
//!   并把前台还给目标窗口；
//! - 会话期键盘由 core::session_keyboard 经 RegisterHotKey 专用线程接管
//!   （LL 钩子方案已被静默摘钩问题淘汰，勿回退）；
//! - 外击关闭经 WH_MOUSE_LL 钩子；
//! - 尺寸/位置持久化与三种定位模式在 core::PickerPositionService。

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
#[cfg(test)]
use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tracing::{info, warn};

use floatpaste_core::backdrop;
use floatpaste_core::domain::clip_item::{
    available_paste_modes, resolve_secondary_mode, ClipItemSummary, PasteMode, PasteOption,
    TextKind,
};
use floatpaste_core::domain::settings::{
    PasteTrigger, PickerPositionMode, UserSetting, WindowMaterial,
};
use floatpaste_core::platform::windows::active_app::{ActiveAppResolver, WindowFocusTarget};
use floatpaste_core::platform::windows::picker_position::ScreenPoint;
use floatpaste_core::platform::windows::screen_capture;
use floatpaste_core::platform::windows::start_menu;
use floatpaste_core::platform::windows::window_control::{self, GestureMode, ResizeDirection};
use floatpaste_core::platform::windows::{mouse_monitor, session_keyboard};
use floatpaste_core::services::clip_display::{
    clip_type_label, paste_action_summaries, PasteActionSummary,
};
use floatpaste_core::services::clip_service::ClipService;
use floatpaste_core::services::list_navigation::next_navigation_index;
use floatpaste_core::services::picker_position_service::{
    default_window_size, resolve_near_cursor, PickerPositionService, WindowGeometry,
    PICKER_MIN_HEIGHT, PICKER_MIN_WIDTH,
};
use floatpaste_core::services::time_format::format_relative_time_or_unused;
#[cfg(test)]
use floatpaste_core::state::CoreState;
use floatpaste_core::theme;

use crate::app::App;
#[cfg(test)]
use crate::app_state::SharedState;
use crate::app_state::TargetSession;
use crate::overlay::{self, ForegroundPolicy};
use crate::paste_actions::action_items;
use crate::paste_flow;
use crate::search;
use crate::theme_bridge;
use crate::thumbnails;
use crate::tooltip;
use crate::win32_ext;
#[cfg(test)]
use crate::SearchWindow;
use crate::{ClipRow, PanelGeometry, QuickPasteWindow};

// debug 软件渲染冷帧较慢；发布构建保留短暂的屏外布局与呈现准备期。
const REVEAL_DELAY_MS: u64 = if cfg!(debug_assertions) { 200 } else { 50 };
// 定位和抓屏共用预算；冷 provider / GDI 不能把第一次呼出拖到秒级。
const PREPARATION_BUDGET: Duration = Duration::from_millis(150);

/// 入库预览的字符截断上限（normalize_service 同值）：达到即认为原文更长
const PREVIEW_SOURCE_LIMIT: usize = 120;

/* ───────────────── 显示 / 隐藏 / 切换 ───────────────── */

// 会话期置顶位守护：速贴靠 WS_EX_TOPMOST 压住普通窗口。两种失效都要
// 兜住——① winit 异步样式重排/系统操作把置顶位剥掉；② 另一个置顶
// 窗口在速贴之后抬升（topmost 带内后来者在上有权盖住速贴）。会话期间
// 每 500ms 检查一次，失效即重抬；会话结束停表。仅在事件循环线程触达。
thread_local! {
    static TOPMOST_GUARD: std::cell::RefCell<Option<slint::Timer>> =
        const { std::cell::RefCell::new(None) };
    static REVEAL_TIMER: std::cell::RefCell<slint::Timer> = slint::Timer::default().into();
    static LAST_HIDDEN_AT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

fn cancel_reveal() {
    // stop() 保留回调及其中的接收端；丢弃定时器才能立即释放迟到结果。
    REVEAL_TIMER.with(|timer| {
        timer.replace(slint::Timer::default());
    });
}

fn start_topmost_guard(app: &App) {
    stop_topmost_guard();
    let app_cb = app.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(500),
        move || {
            let app = app_cb.clone();
            if !app.state.is_picker_active() {
                stop_topmost_guard();
                return;
            }
            let hwnd = app.state.picker_hwnd.load(Ordering::SeqCst);
            if hwnd == 0 {
                return;
            }
            // 自家 tooltip 悬浮在速贴之上属正常预览，不算被覆盖
            let tooltip_hwnd = app.state.tooltip_hwnd.load(Ordering::SeqCst);
            let covered = window_control::is_covered_by_visible_window(hwnd, &[tooltip_hwnd]);
            if !window_control::is_topmost(hwnd) || covered {
                warn!("速贴置顶失效（位丢失或被覆盖），重抬 TOPMOST");
                window_control::set_window_topmost_no_activate(hwnd);
            }
        },
    );
    TOPMOST_GUARD.with(|slot| *slot.borrow_mut() = Some(timer));
}

fn stop_topmost_guard() {
    TOPMOST_GUARD.with(|slot| {
        if let Some(timer) = slot.borrow_mut().as_ref() {
            timer.stop();
        }
        *slot.borrow_mut() = None;
    });
}

/// 打开速贴面板（对齐 WindowCoordinator::activate_picker）
pub fn activate(app: &App) {
    let started = Instant::now();
    let Some(win) = app.picker.upgrade() else {
        return;
    };

    let settings = app.state.current_settings();
    let hwnd = app
        .state
        .picker_hwnd
        .load(std::sync::atomic::Ordering::SeqCst);
    if hwnd == 0 {
        warn!("速贴窗口 HWND 尚未就绪，无法显示");
        return;
    }

    if app.state.is_picker_active() {
        // 停屏窗口的 IsWindowVisible 仍为真；准备期重复打开不能提前落位。
        if REVEAL_TIMER.with(|timer| timer.borrow().running()) {
            return;
        }
        // 兜底：标志位与真实可见性脱节时（Win+D/多屏切换/被全屏应用抢占）
        // 重置状态后走完整显示流程，避免后续打开沦为空操作
        if window_control::is_window_visible(hwnd) {
            // 尺寸不动：用窗口当前实测尺寸（Slint 的 size() 即物理像素，
            // 不要再乘缩放因子，乘了约束用的尺寸会虚大）
            let size = win.window().size();
            if let Some(point) = resolve_window_position(app, &settings, size, None) {
                window_control::set_window_bounds(
                    hwnd,
                    point.x,
                    point.y,
                    size.width as i32,
                    size.height as i32,
                );
            }
            // 该路径绕过 after_show：置顶与守护需自行保证
            window_control::set_window_topmost_no_activate(hwnd);
            start_topmost_guard(app);
            // close 会结束全局键鼠会话；重新装配以覆盖托盘菜单让位路径。
            crate::tray_menu::close(app);
            begin_input_session(app, hwnd, &settings);
            return;
        }
        warn!("Picker 标志位为激活但窗口实际不可见，重置状态后重新显示");
        app.state.end_picker_activation();
        session_keyboard::end_session();
        mouse_monitor::end_session();
    }

    // 托盘菜单与速贴共用键鼠会话；完整打开前先让位。
    crate::tray_menu::close(app);

    // 恢复记忆尺寸（无记忆则按设计尺寸与当前缩放折算为物理像素）。
    // 这一组尺寸必须原样带到定位环节——首开时窗口刚请求过 resize，
    // 此时回读窗口尺寸拿到的是装配期旧值，用它算约束必然错位
    let size = resolve_physical_size(app, win.window().scale_factor());
    // 停屏态定尺寸走裸 SetWindowPos（单一确定的「只动几何、绝不激活」，
    // 不经 Slint/winit 的窗口 API 附带语义），见 window_control 注释
    window_control::set_window_size_no_activate(hwnd, size.width as i32, size.height as i32);
    window_control::set_window_min_size(
        hwnd,
        (PICKER_MIN_WIDTH as f32 * win.window().scale_factor()) as i32,
        (PICKER_MIN_HEIGHT as f32 * win.window().scale_factor()) as i32,
    );

    // 开始菜单开着时速贴压不过它（shell 表面在更高 z-order 带内），定位
    // 服务会把落位改到开始菜单旁边（开始菜单保持打开）；这里跳过把
    // shell 窗口记为回贴目标——插入符定位会锚到 shell 自己的焦点元素，
    // 还原/回贴动作对 shell 窗口全部无效
    let focus_target = if start_menu::foreground_is_start_menu() {
        WindowFocusTarget::default()
    } else {
        ActiveAppResolver::current_foreground_focus_target()
    };
    let session = TargetSession {
        session_id: crate::app_state::next_session_id(),
        target_window_hwnd: focus_target.window_hwnd,
        target_focus_hwnd: focus_target.focus_hwnd,
    };
    app.state.set_picker_session(session);
    info!(
        session_id = session.session_id,
        hwnd,
        idle_ms = ?LAST_HIDDEN_AT.with(|at| at.get().map(|at| at.elapsed().as_millis())),
        target_window = ?session.target_window_hwnd,
        target_focus = ?session.target_focus_hwnd,
        "显示 Picker"
    );

    // 空态快捷键提示跟随当前设置（用户改过热键后提示不失效）
    win.set_monitoring_paused(settings.pause_monitoring);

    // 主题随设置刷新（设置可能在后台被改变）
    let theme_started = Instant::now();
    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
    let theme_resolve_ms = theme_started.elapsed().as_millis();
    let theme_apply_started = Instant::now();
    theme_bridge::apply_theme(
        Some(&win),
        app.tooltip.upgrade().as_ref(),
        app.tray_menu.upgrade().as_ref(),
        app.search.upgrade().as_ref(),
        app.editor.upgrade().as_ref(),
        app.settings.upgrade().as_ref(),
        &tokens,
    );
    tracing::debug!(
        session_id = session.session_id,
        resolve_ms = theme_resolve_ms,
        apply_ms = theme_apply_started.elapsed().as_millis(),
        "速贴主题准备完成"
    );

    // 材质底：唤起时按设置「背景材质」现读分派（Mica/Acrylic 均自绘
    // 烘焙、tint 分档 / 无实底，见 prepare_picker_backdrop）

    let epoch = app.state.begin_picker_activation();

    // 重现（无激活 + 置顶，对齐原版 always_on_top）。窗口自启动起保持
    // Slint 可见（停屏态），这里只做最小化兜位恢复，不走 Slint show：
    // hide→show 周期中 winit 清空表面且脏区跟踪失效，是开窗透明闪烁的
    // 根源。样式重挂与前台策略仍交 overlay 公共层处理
    let show_started = Instant::now();
    let _ = window_control::show_window_no_activate(hwnd);
    tracing::debug!(
        session_id = session.session_id,
        show_ms = show_started.elapsed().as_millis(),
        "速贴屏外窗口恢复完成"
    );

    // 会话开始：刷新列表、选中归零、滚回顶部、清空消息。放在上屏之前：
    // 列表重活（查询 + 逐行裁排）跑在停屏期间，上屏即最新内容
    refresh_list_reset(app, &settings);
    tracing::debug!(
        session_id = session.session_id,
        elapsed_ms = started.elapsed().as_millis(),
        "速贴列表与主题准备完成"
    );

    prepare_reveal(
        app,
        &win,
        size,
        epoch,
        &settings.picker_position_mode,
        tokens,
        started,
    );
}

/// 两条显示路径共用准备期与收尾；等待时间只给事件循环渲染机会，不保证帧已完成。
fn schedule_reveal(
    app: &App,
    win: &QuickPasteWindow,
    hwnd: isize,
    point: ScreenPoint,
    size: slint::PhysicalSize,
    epoch: u64,
    started: Instant,
) {
    win32_ext::force_full_repaint(win);
    let app = app.clone();
    let scheduled = Instant::now();
    REVEAL_TIMER.with(|timer| {
        timer.borrow().start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(REVEAL_DELAY_MS),
            move || {
                if !app.state.is_current_picker_activation(epoch) {
                    return;
                }
                let Some(win) = app.picker.upgrade() else {
                    return;
                };
                let timer_wait_ms = scheduled.elapsed().as_millis();
                let warm_started = Instant::now();
                win32_ext::warm_surface(hwnd);
                let warm_ms = warm_started.elapsed().as_millis();
                let place_started = Instant::now();
                window_control::set_window_bounds(
                    hwnd,
                    point.x,
                    point.y,
                    size.width as i32,
                    size.height as i32,
                );
                win32_ext::force_full_repaint(&win);
                let place_ms = place_started.elapsed().as_millis();
                let overlay_started = Instant::now();
                let target = app.state.picker_session().target_window_hwnd;
                // 用户可在准备期切换前台；只纠正速贴自己偷走的焦点。
                let restore =
                    target.map_or(ForegroundPolicy::Keep, ForegroundPolicy::RestoreIfStolen);
                let state = app.state.clone();
                overlay::after_show(hwnd, false, restore, restore, move || {
                    state.is_current_picker_activation(epoch)
                });
                let overlay_ms = overlay_started.elapsed().as_millis();
                let input_started = Instant::now();
                start_topmost_guard(&app);
                if target == Some(app.state.search_hwnd.load(Ordering::SeqCst)) {
                    search::suspend_input(&app);
                }
                // LL 鼠标钩子由事件循环线程泵出，必须在重活完成且真实显示后安装。
                begin_input_session(&app, hwnd, &app.state.current_settings());
                tracing::debug!(
                    session_id = app.state.picker_session().session_id,
                    configured_delay_ms = REVEAL_DELAY_MS,
                    timer_wait_ms,
                    warm_ms,
                    place_ms,
                    overlay_ms,
                    input_setup_ms = input_started.elapsed().as_millis(),
                    "速贴显屏阶段完成"
                );
                // 到此仅表示已移入屏内并完成输入装配，不保证 DWM 已合成首帧
                info!(
                    session_id = app.state.picker_session().session_id,
                    elapsed_ms = started.elapsed().as_millis(),
                    "速贴实际显示并接管输入"
                );
            },
        );
    });
}

fn save_window_geometry(app: &App, hwnd: isize) {
    // 准备期尚未落位，不能用 -32000 停屏坐标覆盖用户的几何记忆。
    if !app.state.is_picker_active() || REVEAL_TIMER.with(|timer| timer.borrow().running()) {
        return;
    }
    if let Some(rect) = (hwnd != 0)
        .then(|| win32_ext::physical_rect(hwnd))
        .flatten()
        .filter(|rect| (rect.left, rect.top) != (-32000, -32000))
    {
        let geometry = WindowGeometry {
            x: rect.left,
            y: rect.top,
            width: (rect.right - rect.left).max(0) as u32,
            height: (rect.bottom - rect.top).max(0) as u32,
        };
        if let Some(stored) = PickerPositionService::capture_window_position(geometry) {
            let _ = app.core().repository.save_picker_window_state(&stored);
        }
    }
}

/// 隐藏（对齐 WindowCoordinator::hide_picker + hide_picker_and_restore_target）
pub fn hide(app: &App, restore_target: bool) {
    reset_delete_arm(app);
    stop_topmost_guard();
    let Some(win) = app.picker.upgrade() else {
        return;
    };
    let was_active = app.state.is_picker_active();
    let hwnd = app
        .state
        .picker_hwnd
        .load(std::sync::atomic::Ordering::SeqCst);

    session_keyboard::end_session();
    mouse_monitor::end_session();

    save_window_geometry(app, hwnd);
    cancel_reveal();
    win.set_actions_row(-1);

    // 停屏到屏幕外而非 Slint hide（同 hide_for_editor）：hide→show 周期中
    // winit 清空窗口表面且 Slint 脏区跟踪失效，下次显示会出现透明空壳；
    // 停屏保持表面内容有效，重现即原样恢复
    win.window()
        .set_position(slint::PhysicalPosition::new(-32000, -32000));

    app.state.end_picker_activation();
    if was_active {
        LAST_HIDDEN_AT.with(|at| at.set(Some(Instant::now())));
    }
    tooltip::cancel(app);
    info!(
        session_id = app.state.picker_session().session_id,
        "隐藏 Picker"
    );

    if restore_target {
        let session = app.state.picker_session();
        let epoch = app.state.picker_session_epoch();
        let app_for_resume = app.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if app_for_resume.core().is_quitting()
                || app_for_resume.state.is_picker_active()
                || app_for_resume.state.picker_session_epoch() != epoch
            {
                return;
            }
            if let Some(target_hwnd) = session.target_window_hwnd {
                let current = ActiveAppResolver::current_foreground_hwnd();
                // 新窗口已取得前台时，不让关闭前的延迟恢复覆盖它。
                if current.is_none() || current == Some(hwnd) || current == Some(target_hwnd) {
                    let _ = ActiveAppResolver::restore_foreground_window(target_hwnd);
                    if target_hwnd == app_for_resume.state.search_hwnd.load(Ordering::SeqCst) {
                        search::resume_input(&app_for_resume);
                    }
                }
            }
        });
    }
}

/// 进入编辑器前复用收起流程，保留目标会话并让编辑器接管前台。
pub fn hide_for_editor(app: &App) {
    hide(app, false);
}

/// 主快捷键命中：活跃则关闭并恢复目标，否则打开。
/// 搜索窗口活跃时先无还原收起（对齐旧版 toggle_picker_from_shortcut，
/// 避免两窗口争抢焦点导致闪烁）。每击必响应，不做防抖（系统重复由
/// 注册层 MOD_NOREPEAT 过滤）
pub fn toggle(app: &App) {
    if app.state.is_picker_active() {
        hide(app, true);
        return;
    }
    if app.state.is_search_active() {
        search::hide(app, false);
    }
    activate(app);
}

/// 从编辑器返回速贴：恢复会话快捷键与外击关闭，窗口以无激活方式在
/// 停屏前的原位重现，**故意不重置列表与选中**（对齐旧版
/// restore_picker_after_editor 不发 SESSION_START，保留进入编辑器时的
/// 上下文与滚动位置）。先归还目标前台，再延迟显示；会话键经
/// RegisterHotKey 接回，无需窗口持有焦点
pub fn restore_after_editor(app: &App, target: TargetSession) {
    let started = Instant::now();
    let Some(win) = app.picker.upgrade() else {
        return;
    };
    let hwnd = app
        .state
        .picker_hwnd
        .load(std::sync::atomic::Ordering::SeqCst);
    if hwnd == 0 {
        warn!("速贴窗口 HWND 尚未就绪，无法从编辑器返回");
        return;
    }
    // 编辑期间也可能被主快捷键再次唤起；先停屏，避免抓到面板自己。
    if app.state.is_picker_active() {
        hide(app, false);
    }
    let settings = app.state.current_settings();
    crate::tray_menu::close(app);

    // 恢复记忆尺寸（无记忆则按设计尺寸与当前缩放折算），同 activate：
    // 尺寸与定位取自同一组数，不回读窗口
    let size = resolve_physical_size(app, win.window().scale_factor());
    window_control::set_window_size_no_activate(hwnd, size.width as i32, size.height as i32);
    window_control::set_window_min_size(
        hwnd,
        (PICKER_MIN_WIDTH as f32 * win.window().scale_factor()) as i32,
        (PICKER_MIN_HEIGHT as f32 * win.window().scale_factor()) as i32,
    );

    app.state.set_picker_session(target);
    let epoch = app.state.begin_picker_activation();
    info!(session_id = target.session_id, "从编辑器恢复 Picker");

    // 编辑窗口刚关闭，先归还来源目标；等待期内的用户切换由显示收尾保留。
    if let Some(hwnd) = target.target_window_hwnd {
        if !ActiveAppResolver::restore_foreground_window_with_focus(hwnd, target.target_focus_hwnd)
            && ActiveAppResolver::force_foreground_window(hwnd)
        {
            let _ = ActiveAppResolver::restore_foreground_window_with_focus(
                hwnd,
                target.target_focus_hwnd,
            );
        }
    }
    let _ = window_control::show_window_no_activate(hwnd);

    // 主题与底图必须在准备首帧之前更新。
    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
    theme_bridge::apply_theme(
        Some(&win),
        app.tooltip.upgrade().as_ref(),
        app.tray_menu.upgrade().as_ref(),
        app.search.upgrade().as_ref(),
        app.editor.upgrade().as_ref(),
        app.settings.upgrade().as_ref(),
        &tokens,
    );

    prepare_reveal(
        app,
        &win,
        size,
        epoch,
        &PickerPositionMode::LastPosition,
        tokens,
        started,
    );
}

fn begin_input_session(app: &App, hwnd: isize, settings: &UserSetting) {
    let epoch = app.state.picker_session_epoch();
    let app_for_mouse = app.clone();
    // 外击关闭不还原前台：用户点击的位置已取得焦点，此时再把原目标
    // 拉回前台会被前台所有权检查拒绝，Windows 拒绝时闪烁目标窗口的
    // 任务栏按钮（Esc/主快捷键关闭仍走还原，见 hide）。命中矩形按短影
    // 留边内缩，只把可见卡片视为窗口内
    let inset = app.picker.upgrade().map_or(0, |win| shadow_margin_px(&win));
    mouse_monitor::begin_session(
        hwnd,
        inset,
        Box::new(move || {
            let app = app_for_mouse.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if app.state.is_current_picker_activation(epoch) {
                    hide(&app, false);
                }
            });
        }),
    );

    let app_for_keys = app.clone();
    session_keyboard::begin_session(
        session_keyboard::SessionKeyConfig::from_settings(settings),
        Box::new(move |action| {
            let app = app_for_keys.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if app.state.is_current_picker_activation(epoch) {
                    handle_session_action(&app, action);
                }
            });
        }),
    );
}

pub(crate) fn handle_session_action(app: &App, action: session_keyboard::SessionAction) {
    use session_keyboard::SessionAction;
    if !app.state.is_picker_active() {
        // 会话残留动作（如窗口消失瞬间的按键）在此过滤；
        // 留痕用于诊断「面板已显但按键无响应」类问题
        info!("会话动作忽略（面板非活跃）: {action:?}");
        return;
    }
    if action == SessionAction::Dismiss
        && app
            .with_picker(|win| win.get_delete_armed_row() >= 0)
            .unwrap_or(false)
    {
        reset_delete_arm(app);
        return;
    }
    if !matches!(action, SessionAction::DeleteEntry | SessionAction::Confirm) {
        reset_delete_arm(app);
    }
    // 菜单展开：会话键先服务菜单（↑↓/Enter/Esc），见专属处理
    let actions_row = app.with_picker(|win| win.get_actions_row()).unwrap_or(-1);
    if actions_row >= 0 {
        handle_session_action_with_actions(app, actions_row, action);
        return;
    }
    match action {
        SessionAction::NavigateUp => navigate(app, true),
        SessionAction::NavigateDown => navigate(app, false),
        SessionAction::Confirm => confirm(app, current_index(app), false),
        SessionAction::ConfirmAsPath => confirm(app, current_index(app), true),
        SessionAction::Dismiss => hide(app, true),
        SessionAction::ToggleFavorite => toggle_favorite(app),
        SessionAction::DeleteEntry => request_delete(app, current_index(app)),
        SessionAction::OpenEditor => {
            // 打开编辑器（面板隐藏、会话让位，关闭后回到本面板；
            // 对齐旧版 PICKER_OPEN_EDITOR_EVENT）
            let index = current_index(app);
            let Some(item) = app.state.item_at(index) else {
                warn!("打开编辑器失败: 选中索引 {index} 无对应条目（列表缓存与选中态脱节）");
                return;
            };
            crate::editor::open_from_picker(app, item.id);
        }
        SessionAction::SelectIndex(digit) => {
            let count = app.state.items().len();
            if count == 0 {
                return;
            }
            let index = (digit as usize - 1).min(count - 1);
            set_selected(app, index);
            confirm(app, index, false);
        }
    }
}

fn current_index(app: &App) -> usize {
    app.with_picker(|win| win.get_selected().max(0) as usize)
        .unwrap_or(0)
}

pub fn set_selected(app: &App, index: usize) {
    let id = app.state.item_at(index).map(|item| item.id);
    if id != app.state.selected_id() {
        reset_delete_arm(app);
    }
    app.state.set_selected_id(id);
    // 只写选中索引：预览统一按重字重裁排（见 build_rows），不随选中变化，
    // 行模型无需重建（重建还会打断行内悬停态）
    app.with_picker(|win| win.set_selected(index as i32));
}

/* ───────────────── 列表刷新 ───────────────── */

/// 会话开始：重置到顶部（对齐 PICKER_SESSION_START_EVENT 处理）
fn refresh_list_reset(app: &App, settings: &floatpaste_core::domain::settings::UserSetting) {
    app.state.set_selected_id(None);
    if let Some(win) = app.picker.upgrade() {
        win.set_monitoring_paused(settings.pause_monitoring);
        win.set_digit_shortcuts_enabled(settings.picker_digit_shortcuts_enabled);
        win.set_loading(false);
    }
    let keep_anchor = false;
    refresh_list(app, keep_anchor);
    app.with_picker(|win| {
        win.set_selected(0);
        win.set_scroll_generation(win.get_scroll_generation() + 1);
    });
}

/// 剪贴板变化：按 id 锚点恢复选中（对齐 PickerShell 的 items 变更 effect）
pub fn refresh_list_changed(app: &App) {
    if !app.state.is_picker_active() {
        return;
    }
    refresh_list(app, true);
}

/// 设置保存后刷新提示和已缓存的动作目录，不重新查询或改变选中条目。
pub(crate) fn refresh_copy(app: &App) {
    close_actions(app);
    let settings = app.state.current_settings();
    app.with_picker(|win| {
        win.set_monitoring_paused(settings.pause_monitoring);
        win.set_digit_shortcuts_enabled(settings.picker_digit_shortcuts_enabled);
        apply_card_height(win, settings.picker_card_height);
        build_rows(app, win, &app.state.items());
    });
}

/// 编辑器内保存/删除/改标签后的刷新：速贴此时处于失活态，
/// 跳过活跃判定强制重排，保证返回时列表已反映变更
pub fn refresh_after_editor(app: &App) {
    refresh_list(app, true);
}

/// 重新查询列表并重建行模型。
/// keep_anchor=true 时保持当前选中（按 id 恢复）；false 时选中归零
fn refresh_list(app: &App, keep_anchor: bool) {
    reset_delete_arm(app);
    let settings = app.state.current_settings();
    let limit = settings.picker_record_limit.clamp(9, 1000);

    let query_started = Instant::now();
    let result = app.core().repository.list_recent(limit);
    tracing::debug!(
        session_id = app.state.picker_session().session_id,
        query_ms = query_started.elapsed().as_millis(),
        row_count = result.as_ref().map_or(0, Vec::len),
        success = result.is_ok(),
        keep_anchor,
        "速贴列表查询完成"
    );
    match result {
        Ok(items) => {
            let anchor_id = if keep_anchor {
                app.state.selected_id()
            } else {
                None
            };
            app.state.set_items(items.clone());

            let win = match app.picker.upgrade() {
                Some(win) => win,
                None => return,
            };
            win.set_load_failed(false);

            let selected_index = match &anchor_id {
                Some(id) => items
                    .iter()
                    .position(|item| &item.id == id)
                    .unwrap_or_else(|| {
                        // 锚点条目已被删除：落到当前 index 指向的条目上
                        let current = win.get_selected().max(0) as usize;
                        current.min(items.len().saturating_sub(1))
                    }),
                None => 0,
            };
            app.state
                .set_selected_id(items.get(selected_index).map(|item| item.id.clone()));
            win.set_selected(selected_index as i32);

            // 行集整体重建（剪贴板变化/收藏刷新/缩略图回填）：浮层锚定的
            // 行号可能已漂移，收起菜单，避免菜单作用到换位后的条目
            win.set_actions_row(-1);

            build_rows(app, &win, &items);
            let thumbnails_started = Instant::now();
            ensure_thumbnails(app, &items);
            // dispatch_ms 只覆盖缓存检查与任务派发，不含后台线程的解码耗时
            tracing::debug!(
                session_id = app.state.picker_session().session_id,
                dispatch_ms = thumbnails_started.elapsed().as_millis(),
                "速贴缩略图任务派发完成"
            );
        }
        Err(error) => {
            warn!("加载速贴列表失败: {error}");
            app.with_picker(|win| {
                win.set_load_failed(true);
                win.set_load_error_text("数据读取失败，请重试".into());
            });
        }
    }
}

thread_local! {
    /// 列表版本号：异步缩略图回来时校验列表是否已变
    /// （缓存本体在 thumbnails 模块，速贴与搜索共用）
    static LIST_VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

// 已知规模边界：行模型全量重建，且预览逐行裁排；当前
// picker_record_limit（9..=1000）内够用，若要放开上限需先做裁排结果
// 缓存与模型 diff 更新，否则软件渲染与逐行度量会成为刷新瓶颈

/// 预览单行排版高度（逻辑像素）：同一文本一行与两行的度量之差，
/// 不依赖具体字体行框数值。度量走 measure-preview 标尺，与裁排同字重
fn preview_line_height(win: &QuickPasteWindow, is_code: bool) -> f32 {
    let one = win.invoke_measure_preview("文本Ag".into(), 1000.0, is_code);
    let two = win.invoke_measure_preview("文本Ag\n文本Ag".into(), 1000.0, is_code);
    (two - one).max(1.0)
}

/// 应用「速贴卡片高度」设置：写入 PanelGeometry 后读回派生的预览高度，
/// 按实测正文行高派生预览行数预算——布局与裁排共用 PanelGeometry，
/// 卡片铬件公式只在 slint 一侧维护。行数取 round：默认档（预览 60px）
/// 在正文行高 19.4–20.7px 区间恒得三行，与既有固定三行行为一致；
/// floor 会在行高略超 20px 时无谓退到两行
fn apply_card_height(win: &QuickPasteWindow, card_height: u32) {
    let geo = win.global::<PanelGeometry>();
    geo.set_card_height(card_height as f32);
    let preview_height = geo.get_preview_height().max(0.0);
    let line_height = preview_line_height(win, false);
    geo.set_preview_max_lines((preview_height / line_height).round().max(1.0) as i32);
}

/// 用 Slint 排版引擎把预览裁到 max_lines 行的纯裁排核心。
/// measure 返回候选串在给定宽度下的排版高度（逻辑像素），与窗口解耦
/// 以便单元测试固定裁切语义：
/// - 文本放得下且非截断源 → 原样返回（对齐 line-clamp 不溢出不加省略号）
/// - 否则二分最大前缀使「前缀 + …」仍放得下预算行；截断源（原文更长）
///   即便恰好排满预算行也会补出省略号，截断无省略号会被感知为显示不全
pub fn clamp_preview_with(
    measure: &mut dyn FnMut(&str, f32) -> f32,
    text: &str,
    avail_logical: f32,
    source_truncated: bool,
    line_height: f32,
    max_lines: usize,
) -> String {
    if text.is_empty() || avail_logical <= 0.0 {
        return text.to_string();
    }
    let budget = max_lines as f32 * line_height;

    fn fits_with_ellipsis(
        measure: &mut dyn FnMut(&str, f32) -> f32,
        text: &str,
        count: usize,
        avail_logical: f32,
        budget: f32,
    ) -> bool {
        let mut candidate: String = text.chars().take(count).collect();
        let trimmed = candidate.trim_end().len();
        candidate.truncate(trimmed);
        candidate.push('…');
        measure(candidate.as_str(), avail_logical) <= budget + 0.5
    }

    let full_height = measure(text, avail_logical);
    if full_height <= budget + 0.5 && !source_truncated {
        return text.to_string();
    }
    if fits_with_ellipsis(measure, text, text.chars().count(), avail_logical, budget) {
        let mut trimmed = text.trim_end().to_string();
        trimmed.push('…');
        return trimmed;
    }
    if !fits_with_ellipsis(measure, text, 0, avail_logical, budget) {
        return "…".into();
    }
    let mut lo = 0usize;
    let mut hi = text.chars().count();
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if fits_with_ellipsis(measure, text, mid, avail_logical, budget) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mut candidate: String = text.chars().take(lo).collect();
    let trimmed = candidate.trim_end().len();
    candidate.truncate(trimmed);
    candidate.push('…');
    candidate
}

/// 行内裁排：度量走 measure-preview 标尺（Slint 排版引擎，字重固定重字重
/// ——见 picker.slint，选中两态共用同一串与同一行高）
fn clamp_preview(
    win: &QuickPasteWindow,
    text: &str,
    avail_logical: f32,
    source_truncated: bool,
    line_height: f32,
    is_code: bool,
) -> String {
    clamp_preview_with(
        &mut |candidate, width| win.invoke_measure_preview(candidate.into(), width, is_code),
        text,
        avail_logical,
        source_truncated,
        line_height,
        win.global::<PanelGeometry>().get_preview_max_lines() as usize,
    )
}

/// 预览可用宽：以 slint 列表布局实测上报为准（单一事实来源）。
/// 列表未实例化（首帧，preview-width-* 为 0）时按窗口宽兜底：窗口宽 −
/// 卡片边框/滚动槽/列表与行内边距（常量读 PanelGeometry，与 slint 布局
/// 同式）− 缩略图列。实测值到位后 preview-widths-changed 会触发重裁修正
fn preview_avail_widths(win: &QuickPasteWindow) -> (f32, f32) {
    let reported_no_thumb = win.get_preview_width_no_thumb();
    if reported_no_thumb > 0.0 {
        return (reported_no_thumb, win.get_preview_width_thumb());
    }
    let geo = win.global::<PanelGeometry>();
    let chrome = 2.0 * (geo.get_card_border() + geo.get_shadow_margin())
        + geo.get_scroll_slot()
        + geo.get_preview_pad_h();
    let thumb_column = geo.get_thumb_size() + geo.get_thumb_gap();
    // window().size() 为物理像素；度量标尺与布局都以逻辑像素运作
    let scale = win.window().scale_factor();
    let panel_logical = win.window().size().width as f32 / scale;
    let no_thumb = (panel_logical - chrome).max(40.0);
    (no_thumb, (no_thumb - thumb_column).max(40.0))
}

fn build_rows(app: &App, win: &QuickPasteWindow, items: &[ClipItemSummary]) {
    let geometry_started = Instant::now();
    let digit_enabled = win.get_digit_shortcuts_enabled();
    let settings = app.state.current_settings();
    let keys = &settings.session_keys;
    win.set_delete_shortcut(keys.delete_entry.clone().into());
    win.set_dismiss_shortcut(keys.dismiss.clone().into());
    let (avail_no_thumb, avail_with_thumb) = preview_avail_widths(win);
    let line_height = preview_line_height(win, false);
    let code_line_height = preview_line_height(win, true);
    let geometry_ms = geometry_started.elapsed().as_millis();
    let rows_started = Instant::now();
    let mut preview_time = Duration::ZERO;

    let rows: Vec<ClipRow> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let thumb = thumbnails::cached(&item.id);
            let has_thumb = thumb.is_some();
            // 预览达到入库截断上限即认为原文更长，行末始终要有省略号
            let source_truncated = item.content_preview.chars().count() >= PREVIEW_SOURCE_LIMIT;
            let action_reserve = win.global::<PanelGeometry>().get_row_action_width();
            let preview_started = Instant::now();
            let is_code = item.r#type == "text" && item.text_kind == TextKind::Code;
            let preview = clamp_preview(
                win,
                &item.content_preview,
                (if has_thumb {
                    avail_with_thumb
                } else {
                    avail_no_thumb
                } - action_reserve)
                    .max(40.0),
                source_truncated,
                if is_code {
                    code_line_height
                } else {
                    line_height
                },
                is_code,
            );
            preview_time += preview_started.elapsed();
            ClipRow {
                id: item.id.clone().into(),
                preview: preview.into(),
                is_code,
                type_label: clip_type_label(item).into(),
                source_app: item
                    .source_app
                    .clone()
                    .unwrap_or_else(|| "未知来源".into())
                    .into(),
                time_text: format_relative_time_or_unused(
                    item.last_used_at
                        .as_deref()
                        .or(Some(item.created_at.as_str())),
                )
                .into(),
                favorited: item.is_favorited,
                digit: if digit_enabled && index < 9 {
                    SharedString::from((index + 1).to_string())
                } else {
                    SharedString::from("")
                },
                has_thumb: thumb.is_some(),
                thumb: thumb.unwrap_or_default(),
                tags: {
                    let shown: Vec<SharedString> = item
                        .tags
                        .iter()
                        .take(1)
                        .map(|tag| SharedString::from(tag.clone()))
                        .collect();
                    ModelRc::new(Rc::new(VecModel::from(shown)))
                },
                tag_more: item.tags.len().saturating_sub(1) as i32,
                actions: action_items(&paste_action_summaries(
                    item,
                    &keys.confirm,
                    &keys.confirm_as_file,
                    &settings.secondary_paste,
                )),
            }
        })
        .collect();

    let rows_time = rows_started.elapsed();
    let publish_started = Instant::now();
    win.set_rows(ModelRc::new(Rc::new(VecModel::from(rows))));
    LIST_VERSION.with(|version| version.set(thumbnails::bump_list_version()));
    tracing::debug!(
        session_id = app.state.picker_session().session_id,
        row_count = items.len(),
        geometry_ms,
        preview_ms = preview_time.as_millis(),
        projection_ms = rows_time.saturating_sub(preview_time).as_millis(),
        publish_ms = publish_started.elapsed().as_millis(),
        "速贴行模型准备完成"
    );
}

thread_local! {
    /// 宽度变化重裁的防抖令牌：新触发使旧回调失效
    static RECLAMP_TOKEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// 面板宽度变化（拖拽缩放）后按新宽度重裁预览（150ms 防抖）
pub fn schedule_preview_reclamp(app: &App) {
    let token = RECLAMP_TOKEN.with(|value| {
        value.set(value.get() + 1);
        value.get()
    });
    let app = app.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
        if RECLAMP_TOKEN.with(|value| value.get()) != token || !app.state.is_picker_active() {
            return;
        }
        let items = app.state.items();
        app.with_picker(|win| build_rows(&app, win, &items));
    });
}

/// 异步补齐缩略图：先渲染已有缓存，后台解码缺失项后回填行模型
/// （对齐原版 img loading=lazy 的非阻塞观感）
fn ensure_thumbnails(app: &App, items: &[ClipItemSummary]) {
    thumbnails::ensure(app, items, |app| {
        let items = app.state.items();
        app.with_picker(|win| build_rows(app, win, &items));
    });
}

/* ───────────────── 会话动作 ───────────────── */

/// 确认上屏：主动作按本体；次级动作用该类别配置的默认变体（普通文本
/// 等同主动作）。窗口不解析模式，目录与配置集中在 domain。
pub fn confirm(app: &App, index: usize, secondary: bool) {
    let Some(item) = app.state.item_at(index) else {
        return;
    };
    let settings = app.state.current_settings();
    let mode = if secondary {
        resolve_secondary_mode(&item, &settings.secondary_paste)
    } else {
        PasteMode::Original
    };
    confirm_mode(app, index, mode);
}

/// 按显式动作上屏：「更多动作」浮层与菜单的选择路径。
fn confirm_mode(app: &App, index: usize, mode: PasteMode) {
    let Some(item) = app.state.item_at(index) else {
        return;
    };

    tooltip::cancel(app);

    let settings = app.state.current_settings();
    let option = PasteOption {
        restore_clipboard_after_paste: settings.restore_clipboard_after_paste,
        paste_to_target: true,
        mode,
    };

    if let Err(error) =
        paste_flow::paste_item(app, &item.id, option, paste_flow::PasteSource::Picker)
    {
        warn!("粘贴失败: {error}");
        crate::tray::notify_failure(&paste_flow::failure_notice(mode, &error));
    }
}

/* ───────────────── 更多操作菜单 ───────────────── */

/// 为某行打开更多操作菜单：选中切到该行，高亮回到目录首项（主动作）。
/// 临时选择不改变默认次级动作（快捷键行为不随上次操作漂移）
fn open_actions(app: &App, index: usize) {
    let Some(item) = app.state.item_at(index) else {
        return;
    };
    reset_delete_arm(app);
    tooltip::cancel(app);
    set_selected(app, index);
    let settings = app.state.current_settings();
    let keys = &settings.session_keys;
    let mut actions = paste_action_summaries(
        &item,
        &keys.confirm,
        &keys.confirm_as_file,
        &settings.secondary_paste,
    );
    for (label, shortcut) in [
        (
            if item.is_favorited {
                "取消收藏"
            } else {
                "收藏"
            },
            &keys.toggle_favorite,
        ),
        ("编辑", &keys.open_editor),
        ("删除", &keys.delete_entry),
    ] {
        actions.push(PasteActionSummary {
            label: label.into(),
            shortcut: shortcut.clone(),
        });
    }
    app.with_picker(|win| {
        win.set_menu_actions(action_items(&actions));
        win.set_actions_row(index as i32);
        win.set_action_selected(0);
    });
}

fn close_actions(app: &App) {
    reset_delete_arm(app);
    app.with_picker(|win| win.set_actions_row(-1));
}

/// 浮层展开期间的会话键：↑↓ 与 Enter 服务浮层，Esc 只收浮层（再按才
/// 关面板）；编辑/收藏收起浮层后执行，删除在菜单内确认；其余上屏类动作（次级键、
/// 数字直达）只收浮层不上屏——浏览菜单时的误触不应立即改变剪贴板。
fn handle_session_action_with_actions(
    app: &App,
    sheet_row: i32,
    action: session_keyboard::SessionAction,
) {
    use session_keyboard::SessionAction;
    let index = sheet_row as usize;
    let Some(item) = app.state.item_at(index) else {
        close_actions(app);
        return;
    };
    let count = available_paste_modes(&item).len() + 3;
    let current = app
        .with_picker(|win| win.get_action_selected().max(0) as usize)
        .unwrap_or(0);
    match action {
        SessionAction::NavigateUp | SessionAction::NavigateDown => {
            let next =
                next_navigation_index(count, current, matches!(action, SessionAction::NavigateUp));
            app.with_picker(|win| win.set_action_selected(next as i32));
        }
        SessionAction::Confirm => activate_menu_item(app, index, current),
        SessionAction::Dismiss => close_actions(app),
        SessionAction::OpenEditor => {
            close_actions(app);
            crate::editor::open_from_picker(app, item.id);
        }
        SessionAction::ToggleFavorite => {
            close_actions(app);
            toggle_favorite(app);
        }
        SessionAction::DeleteEntry => request_delete(app, index),
        SessionAction::ConfirmAsPath | SessionAction::SelectIndex(_) => close_actions(app),
    }
}

/// 菜单上半部沿用上屏目录（文件条目含非上屏的「打开所在文件夹」，
/// 由 `paste_flow` 分流执行），分隔线下固定为收藏、编辑、删除。
fn activate_menu_item(app: &App, index: usize, action_index: usize) {
    let Some(item) = app.state.item_at(index) else {
        close_actions(app);
        return;
    };
    let modes = available_paste_modes(&item);
    if let Some(mode) = modes.get(action_index) {
        reset_delete_arm(app);
        close_actions(app);
        confirm_mode(app, index, *mode);
        return;
    }
    match action_index.checked_sub(modes.len()) {
        Some(0) => {
            reset_delete_arm(app);
            close_actions(app);
            toggle_favorite(app);
        }
        Some(1) => {
            reset_delete_arm(app);
            close_actions(app);
            crate::editor::open_from_picker(app, item.id);
        }
        Some(2) => request_delete(app, index),
        _ => {}
    }
}

/* ───────────────── 两段式删除 ───────────────── */

const DELETE_ARM_TIMEOUT: Duration = Duration::from_secs(3);

thread_local! {
    // 按条目 ID 与时限确认，不能让列表行号漂移或延迟定时器导致错删。
    static ARMED_DELETE: std::cell::RefCell<Option<(String, Instant)>> = const { std::cell::RefCell::new(None) };
    static DELETE_TIMER: slint::Timer = slint::Timer::default();
}

fn reset_delete_arm(app: &App) {
    DELETE_TIMER.with(|timer| timer.stop());
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = None);
    app.with_picker(|win| win.set_delete_armed_row(-1));
}

pub fn request_delete(app: &App, index: usize) {
    if !app.state.is_picker_active() {
        return;
    }
    let Some(item) = app.state.item_at(index) else {
        return;
    };
    tooltip::cancel(app);
    set_selected(app, index);
    let confirmed = ARMED_DELETE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|(id, started)| id == &item.id && started.elapsed() < DELETE_ARM_TIMEOUT)
    });
    reset_delete_arm(app);
    if confirmed {
        close_actions(app);
        if let Err(error) = ClipService::delete(app.core(), &item.id) {
            warn!("删除速贴条目失败: {error}");
            crate::tray::notify_failure("删除条目失败，请稍后重试");
            return;
        }
        thumbnails::evict(&item.id);
        refresh_list(app, true);
        search::notify_clips_changed(app);
        return;
    }
    if app.with_picker(|win| win.get_actions_row()).unwrap_or(-1) != index as i32 {
        open_actions(app, index);
    }
    let delete_index = available_paste_modes(&item).len() as i32 + 2;
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = Some((item.id, Instant::now())));
    app.with_picker(|win| {
        win.set_delete_armed_row(index as i32);
        win.set_action_selected(delete_index);
    });
    let app = app.clone();
    DELETE_TIMER.with(|timer| {
        timer.start(
            slint::TimerMode::SingleShot,
            DELETE_ARM_TIMEOUT,
            move || {
                reset_delete_arm(&app);
            },
        );
    });
}

/// 收藏当前选中（对齐 toggleFavoriteSelection：成功后刷新列表并提示）
pub fn toggle_favorite(app: &App) {
    let index = current_index(app);
    let Some(item) = app.state.item_at(index) else {
        return;
    };

    if app
        .state
        .favorite_pending
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }

    let next_favorited = !item.is_favorited;
    let result = ClipService::set_favorited(app.core(), &item.id, next_favorited);
    app.state
        .favorite_pending
        .store(false, std::sync::atomic::Ordering::SeqCst);

    match result {
        // 成功无提示：列表刷新后行上星标与强调边条即反馈
        Ok(()) => {
            refresh_list(app, true);
        }
        Err(error) => {
            warn!("更新收藏状态失败: {error}");
            crate::tray::notify_failure("更新收藏失败，请稍后重试");
        }
    }
}

/// ↑↓ 导航（循环，对齐 (i ± 1 + n) % n）
pub fn navigate(app: &App, up: bool) {
    let count = app.state.items().len();
    if count == 0 {
        return;
    }
    let current = current_index(app);
    let next = next_navigation_index(count, current, up);
    set_selected(app, next);
}

/* ───────────────── 定位 ───────────────── */

/// 本次上屏的窗口物理尺寸：有记忆用记忆（落盘的已是物理像素），无记忆按
/// 设计尺寸与显示器缩放折算。定位与定尺寸必须取自同一组数——首开（或刚从
/// 编辑器返回）时窗口尺寸是这里算出来、刚请求下去的值，回读窗口拿到的
/// 还可能是装配期旧值
fn resolve_physical_size(app: &App, scale_factor: f32) -> slint::PhysicalSize {
    let (width, height) =
        PickerPositionService::resolve_window_size(&app.core().repository, scale_factor)
            .ok()
            .flatten()
            .unwrap_or_else(|| default_window_size(scale_factor));
    slint::PhysicalSize::new(width, height)
}

/// 按设置模式定位并上屏。窗口尺寸是定位的输入而非输出：速贴的三种模式
/// 都要拿它与工作区边界一起约束，保证无论光标贴在屏幕哪一侧，窗口都
/// 整块落在光标所在显示器的工作区内，且优先贴近光标。`physical_size`
/// 必须是窗口**真实**的物理尺寸（传小了约束即失效）
/// 定位解析（纯计算，不动窗口）：定位模式解析 + 光标兜底。
/// 返回 None 表示模式解析与光标兜底均不可得
fn resolve_window_position(
    app: &App,
    settings: &UserSetting,
    physical_size: slint::PhysicalSize,
    target_window_hwnd: Option<isize>,
) -> Option<ScreenPoint> {
    let width = physical_size.width.max(1) as i32;
    let height = physical_size.height.max(1) as i32;
    let mode = settings.picker_position_mode.clone();
    // 解析失败（仓储报错 / 工作区不可得）回退到贴光标
    PickerPositionService::resolve_window_position(
        &app.core().repository,
        &mode,
        width,
        height,
        target_window_hwnd,
    )
    .ok()
    .flatten()
    .or_else(|| resolve_near_cursor(width, height))
}

fn shadow_margin_px(win: &QuickPasteWindow) -> i32 {
    (win.global::<PanelGeometry>().get_shadow_margin() * win.window().scale_factor()).round() as i32
}

type PreparedReveal = (Option<ScreenPoint>, Option<(Vec<u8>, u32, u32)>);

// 简化: 同时只容许一份准备任务；系统调用未返回时后续呼出直接回退，
// 不为每次按键堆积线程。升级：需要并行准备时再引入有界任务队列。
static PREPARATION_BUSY: AtomicBool = AtomicBool::new(false);

struct PreparationGuard;

impl Drop for PreparationGuard {
    fn drop(&mut self) {
        PREPARATION_BUSY.store(false, Ordering::Release);
    }
}

fn spawn_preparation(
    task: impl FnOnce() -> PreparedReveal + Send + 'static,
) -> Option<mpsc::Receiver<PreparedReveal>> {
    if PREPARATION_BUSY.swap(true, Ordering::AcqRel) {
        tracing::debug!("速贴后台准备任务仍在执行，本次直接回退");
        return None;
    }
    let guard = PreparationGuard;
    let (tx, rx) = mpsc::sync_channel(1);
    if let Err(error) = std::thread::Builder::new()
        .name("picker-prepare".into())
        .spawn(move || {
            let _guard = guard;
            let _ = tx.send(task());
        })
    {
        warn!("启动速贴准备线程失败: {error}");
        return None;
    }
    Some(rx)
}

/// 只把原始像素带回 UI 线程；外部进程、GDI 与 CPU 烘焙共用一个截止时间。
fn prepare_reveal(
    app: &App,
    win: &QuickPasteWindow,
    size: slint::PhysicalSize,
    epoch: u64,
    mode: &PickerPositionMode,
    tokens: theme::ThemeTokens,
    started: Instant,
) {
    let setup_started = Instant::now();
    let settings = app.state.current_settings();
    let material = settings.window_material;
    win.set_material_active(matches!(material, WindowMaterial::Mica));
    // 旧底图属于上一次会话；超时也不能露出旧屏幕内容。
    win.set_blur_active(false);
    let fallback = if matches!(mode, PickerPositionMode::LastPosition) {
        resolve_parked_position(app, size)
    } else {
        resolve_near_cursor(size.width as i32, size.height as i32)
    };
    let repository = app.core().repository.clone();
    let session = app.state.picker_session();
    let state = app.state.clone();
    let mode = mode.clone();
    let scale = win.window().scale_factor();
    let margin = shadow_margin_px(win);
    let corner = (win.global::<PanelGeometry>().get_card_radius() * scale).round() as usize;
    let rx = spawn_preparation(move || {
        let _span =
            tracing::info_span!("picker_preparation", session_id = session.session_id, epoch)
                .entered();
        if !state.is_current_picker_activation(epoch) || started.elapsed() >= PREPARATION_BUDGET {
            return (fallback, None);
        }
        let point = PickerPositionService::resolve_window_position(
            &repository,
            &mode,
            size.width as i32,
            size.height as i32,
            session.target_window_hwnd,
        )
        .ok()
        .flatten()
        .or(fallback);
        tracing::debug!(
            elapsed_ms = started.elapsed().as_millis(),
            "速贴定位准备完成"
        );
        if !state.is_current_picker_activation(epoch) || started.elapsed() >= PREPARATION_BUDGET {
            return (point, None);
        }
        let backdrop = point.and_then(|point| {
            prepare_picker_backdrop(point, size, margin, scale, corner, material, &tokens)
        });
        (point, backdrop)
    });
    tracing::debug!(
        session_id = session.session_id,
        setup_ms = setup_started.elapsed().as_millis(),
        elapsed_ms = started.elapsed().as_millis(),
        "速贴后台准备派发完成"
    );
    wait_for_preparation(app, size, epoch, fallback, started, rx);
}

fn wait_for_preparation(
    app: &App,
    size: slint::PhysicalSize,
    epoch: u64,
    fallback: Option<ScreenPoint>,
    started: Instant,
    rx: Option<mpsc::Receiver<PreparedReveal>>,
) {
    let app = app.clone();
    REVEAL_TIMER.with(|timer| {
        timer.borrow().start(
            slint::TimerMode::Repeated,
            Duration::from_millis(8),
            move || {
                if !app.state.is_current_picker_activation(epoch) {
                    cancel_reveal();
                    return;
                }
                let result = rx.as_ref().map(|rx| rx.try_recv());
                let prepared = match result {
                    Some(Ok(prepared)) if started.elapsed() < PREPARATION_BUDGET => prepared,
                    Some(Err(mpsc::TryRecvError::Empty))
                        if started.elapsed() < PREPARATION_BUDGET =>
                    {
                        return
                    }
                    other => {
                        let reason = match other {
                            None => "worker_unavailable",
                            Some(Err(mpsc::TryRecvError::Disconnected)) => "worker_disconnected",
                            _ => "deadline",
                        };
                        info!(
                            session_id = app.state.picker_session().session_id,
                            reason,
                            budget_ms = PREPARATION_BUDGET.as_millis(),
                            elapsed_ms = started.elapsed().as_millis(),
                            "速贴准备超时或不可用，回退实底与备用位置"
                        );
                        (fallback, None)
                    }
                };
                cancel_reveal();
                let Some(win) = app.picker.upgrade() else {
                    return;
                };
                let Some(point) = prepared.0 else {
                    warn!("速贴定位失败，窗口保持停屏位");
                    hide(&app, false);
                    return;
                };
                if let Some((rgba, width, height)) = prepared.1 {
                    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
                    buf.make_mut_bytes().copy_from_slice(&rgba);
                    win.set_blur_backdrop(slint::Image::from_rgba8(buf));
                    win.set_blur_active(true);
                }
                let hwnd = app.state.picker_hwnd.load(Ordering::SeqCst);
                schedule_reveal(&app, &win, hwnd, point, size, epoch, started);
            },
        );
    });
}

/// 速贴底分档编排：唤起时按设置项「背景材质」现读分派（见
/// [WindowMaterial]）。速贴永不激活，DWM 对从未激活窗不渲染 Mica
/// backdrop（ADR-0005 窗口活性前提，实测确认），Mica 与 Acrylic 两档
/// 均为自绘烘焙毛玻璃、以 tint 深浅分档——Mica 档
/// [`backdrop::MICA_TINT_ALPHA`]=0.92 近不透明（Mica 材质本身不透明，
/// 行卡走不透明实卡），Acrylic 档 [`backdrop::ACRYLIC_TINT_ALPHA`]
/// =0.80（深色 tint 提亮，行卡半透玻璃卡），「无」档实底。烘焙抓屏失败（锁屏/会话
/// 切换）回实底。每次唤起重烘焙，明暗自愈
fn prepare_picker_backdrop(
    point: ScreenPoint,
    size: slint::PhysicalSize,
    margin: i32,
    scale: f32,
    corner: usize,
    material: WindowMaterial,
    tokens: &theme::ThemeTokens,
) -> Option<(Vec<u8>, u32, u32)> {
    let tint_alpha = match material {
        WindowMaterial::Mica => backdrop::MICA_TINT_ALPHA,
        WindowMaterial::Acrylic => backdrop::ACRYLIC_TINT_ALPHA,
        WindowMaterial::None => return None,
    };
    let point = ScreenPoint {
        x: point.x + margin,
        y: point.y + margin,
    };
    let size = slint::PhysicalSize::new(
        size.width.saturating_sub(2 * margin as u32).max(1),
        size.height.saturating_sub(2 * margin as u32).max(1),
    );
    // 抓屏 → 三遍 box 模糊 → 裁回卡片尺寸 → 主题 tint → 圆角 alpha。
    let started = std::time::Instant::now();
    let blur_radius = (backdrop::BLUR_RADIUS_LOGICAL * scale).round().max(1.0) as usize;
    let (w, h) = (size.width.max(1) as usize, size.height.max(1) as usize);
    // 扩边抓屏：四周多抓一个模糊半径，模糊后裁回窗口尺寸——边缘模糊
    // 吃到窗外内容，不因滑窗边界 clamp 比中心更「实」。抓取范围钳到虚
    // 拟桌面：桌面之外 BitBlt 填不透明黑，贴屏边唤起会把黑边糊进底图；
    // clamp 语义随之落在屏幕边界（该侧本无窗外内容可吃）
    let m = blur_radius as i32;
    let (want_x, want_y) = (point.x - m, point.y - m);
    let (vx, vy, vw, vh) = screen_capture::virtual_screen_bounds();
    let cap_x = want_x.max(vx);
    let cap_y = want_y.max(vy);
    let cap_w = ((want_x + w as i32 + 2 * m).min(vx + vw) - cap_x).max(1);
    let cap_h = ((want_y + h as i32 + 2 * m).min(vy + vh) - cap_y).max(1);
    let Some(mut rgba) = screen_capture::capture_region_rgba(cap_x, cap_y, cap_w, cap_h) else {
        tracing::warn!(
            "模糊底图抓屏失败: pos=({},{}) size={}x{}",
            point.x,
            point.y,
            w,
            h
        );
        return None;
    };
    let capture_ms = started.elapsed().as_millis();
    let bake_started = Instant::now();
    if cap_w < w as i32 || cap_h < h as i32 {
        // 钳制后装不下窗口（显示器热插拔竞态）：无完整窗口区域可裁，
        // 不进 crop 的 debug_assert，回实底
        tracing::warn!(
            "模糊底图抓屏范围小于窗口: pos=({},{}) size={}x{}",
            point.x,
            point.y,
            w,
            h
        );
        return None;
    }
    let (full_w, full_h) = (cap_w as usize, cap_h as usize);
    backdrop::blur_rgba(&mut rgba, full_w, full_h, blur_radius);
    let mut rgba = backdrop::crop(
        &rgba,
        full_w,
        full_h,
        (cap_x - want_x) as usize,
        (cap_y - want_y) as usize,
        w,
        h,
    );
    if matches!(material, WindowMaterial::Mica) {
        backdrop::tint_opaque(&mut rgba, tokens.material_layer_rgb, tint_alpha);
    } else {
        backdrop::tint_acrylic(&mut rgba, tokens.material_layer_rgb, tokens.is_dark);
    }
    backdrop::bake_round_corners(&mut rgba, w, h, corner);
    // 烘焙成本监控：唤起关键路径上的新增 CPU 开销，一眼可见
    tracing::info!(
        capture_ms,
        bake_ms = bake_started.elapsed().as_millis(),
        "模糊底图烘焙: {}x{}px 半径{} 用时{}ms",
        w,
        h,
        blur_radius,
        started.elapsed().as_millis()
    );
    Some((rgba, w as u32, h as u32))
}

/// 归位解析（纯计算）：不重跑定位模式——鼠标/插入符锚点在编辑往返后
/// 已漂移，重算必然换位。hide_for_editor 停屏前已把当时的几何落盘，
/// 按「上次位置」读回并钳进工作区即原位重现（尺寸同源：上面的
/// resolve_physical_size 读的也是这份落盘几何）。读不到记忆时兜底贴
/// 光标，不把窗口留在屏外停屏位
fn resolve_parked_position(app: &App, physical_size: slint::PhysicalSize) -> Option<ScreenPoint> {
    let width = physical_size.width.max(1) as i32;
    let height = physical_size.height.max(1) as i32;
    floatpaste_core::services::picker_position_service::resolve_from_last_position(
        &app.core().repository,
        width,
        height,
    )
    .ok()
    .flatten()
    .or_else(|| resolve_near_cursor(width, height))
}

// 类型徽章文案（对齐旧版 getClipTypeLabel）见 core::clip_display::clip_type_label

/// 粘贴完成后由 paste_flow 调用：刷新列表顺序（mark_used 改变活动排序）
pub fn notify_pasted(app: &App) {
    refresh_list_changed(app);
}

/* ───────────────── 回调装配 ───────────────── */

/// 速贴窗口回调绑定（main 装配期调用一次）
pub fn wire(app: &App) {
    let Some(win) = app.picker.upgrade() else {
        return;
    };

    // 启动先按持久化设置定形几何；后续变更经 refresh_copy 重入
    apply_card_height(&win, app.state.current_settings().picker_card_height);

    // 单击：单击触发模式下立即上屏，否则仅选中。菜单展开时先收
    // 浮层并吞掉本次点击——去点菜单外的第一击不应立即上屏
    {
        let app_cb = app.clone();
        win.on_row_clicked(move |index| {
            let index = index.max(0) as usize;
            set_selected(&app_cb, index);
            if app_cb
                .with_picker(|win| win.get_actions_row())
                .unwrap_or(-1)
                >= 0
            {
                close_actions(&app_cb);
                return;
            }
            if app_cb.state.current_settings().paste_trigger == PasteTrigger::Click {
                confirm(&app_cb, index, false);
            }
        });
    }

    // 双击上屏（浮层展开时同样只收浮层）
    {
        let app_cb = app.clone();
        win.on_row_double_clicked(move |index| {
            let index = index.max(0) as usize;
            set_selected(&app_cb, index);
            if app_cb
                .with_picker(|win| win.get_actions_row())
                .unwrap_or(-1)
                >= 0
            {
                close_actions(&app_cb);
                return;
            }
            confirm(&app_cb, index, false);
        });
    }

    // 悬停（移动即重置 400ms 计时）；浮层展开时不排悬浮气泡——
    // 气泡会盖住浮层选项
    {
        let app_cb = app.clone();
        win.on_row_hover(move |index, x, y| {
            if app_cb
                .with_picker(|win| win.get_actions_row())
                .unwrap_or(-1)
                >= 0
            {
                tooltip::cancel(&app_cb);
                return;
            }
            tooltip::schedule(&app_cb, index.max(0) as usize, x, y);
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_hover_left(move || {
            tooltip::cancel(&app_cb);
        });
    }

    // 行内按钮共用悬浮气泡，进入按钮时展示动作提示而非条目预览。
    {
        let app_cb = app.clone();
        win.on_button_hint(move |text, x, y| {
            let hwnd = app_cb.state.picker_hwnd.load(Ordering::SeqCst);
            if hwnd != 0 {
                tooltip::schedule_static(
                    &app_cb,
                    tooltip::HoverHost {
                        hwnd,
                        is_active: |app| app.state.is_picker_active(),
                    },
                    text.to_string(),
                    x,
                    y,
                );
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_button_hint_left(move || tooltip::cancel(&app_cb));
    }

    // 行内更多按钮 → 更多操作菜单
    {
        let app_cb = app.clone();
        win.on_row_actions_requested(move |index| {
            if app_cb.with_picker(|win| win.get_actions_row()) == Some(index) {
                close_actions(&app_cb);
            } else {
                open_actions(&app_cb, index.max(0) as usize);
            }
        });
    }

    // 菜单点击与键盘 Enter 共用路由：上屏方式或收藏、编辑、删除。
    {
        let app_cb = app.clone();
        win.on_action_item_clicked(move |action_index| {
            let row = app_cb
                .with_picker(|win| win.get_actions_row())
                .unwrap_or(-1);
            if row < 0 {
                return;
            }
            let index = row as usize;
            if let Ok(action_index) = usize::try_from(action_index) {
                activate_menu_item(&app_cb, index, action_index);
            }
        });
    }

    // 列表实测预览可用宽（含窗口缩放）→ 按新宽度重裁预览
    {
        let app_cb = app.clone();
        win.on_preview_widths_changed(move || {
            schedule_preview_reclamp(&app_cb);
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

    // 顶栏关闭按钮：同 Esc 会话关闭路径（隐藏并归还目标焦点）
    {
        let app_cb = app.clone();
        win.on_close_clicked(move || {
            hide(&app_cb, true);
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
            refresh_list_changed(&app_cb);
        });
    }
}

#[cfg(test)]
pub(crate) mod reveal_contract_tests {
    use super::*;
    use floatpaste_core::{
        domain::settings::StoredWindowPosition, repository::sqlite_repository::SqliteRepository,
        services::image_storage::ImageStorage, services::normalize_service::NormalizeService,
    };
    use std::{path::Path, time::Duration};

    fn pump(ms: u64) {
        slint::Timer::single_shot(Duration::from_millis(ms), || {
            slint::quit_event_loop().unwrap();
        });
        slint::run_event_loop_until_quit().unwrap();
    }

    fn wait_for_reveal(hwnd: isize, expected: ScreenPoint) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if win32_ext::physical_rect(hwnd)
                .is_some_and(|r| (r.left, r.top) == (expected.x, expected.y))
            {
                return;
            }
            // 暖帧繁忙时准备超时和显示延迟会在不同轮事件循环调度。
            // 正向显屏断言等实际几何，提前显示/取消断言仍按原时序检查。
            pump(16);
        }
    }

    pub(crate) fn run() {
        let dir = std::env::temp_dir().join(format!("floatpaste-reveal-{}", std::process::id()));
        let _cleanup = TempDirGuard(dir.clone());
        let repository = SqliteRepository::new(Path::new(":memory:")).unwrap();
        for text in ["首帧甲", "首帧乙"] {
            let item = NormalizeService::normalize_text(text, None).unwrap();
            repository.save_text_item(&item).unwrap();
        }
        let settings = UserSetting {
            picker_position_mode: PickerPositionMode::LastPosition,
            window_material: WindowMaterial::None,
            ..Default::default()
        };
        let core = CoreState::new(repository, ImageStorage::new(dir).unwrap(), settings);
        let win = QuickPasteWindow::new().unwrap();
        win.window()
            .set_position(slint::PhysicalPosition::new(-32000, -32000));
        win.show().unwrap();
        pump(100);
        let hwnd = overlay::silent_assemble_parked(&win).unwrap();
        let app = App {
            state: Arc::new(SharedState::new(core)),
            picker: win.as_weak(),
            tooltip: Default::default(),
            tray_menu: Default::default(),
            search: Default::default(),
            editor: Default::default(),
            settings: Default::default(),
        };
        app.state.picker_hwnd.store(hwnd, Ordering::SeqCst);
        let stored = StoredWindowPosition {
            x: 120,
            y: 140,
            width: Some(360),
            height: Some(420),
        };
        app.core()
            .repository
            .save_picker_window_state(&stored)
            .unwrap();
        let expected = resolve_parked_position(&app, slint::PhysicalSize::new(360, 420)).unwrap();
        let parked = || {
            let rect = win32_ext::physical_rect(hwnd).unwrap();
            assert_eq!((rect.left, rect.top), (-32000, -32000));
            assert!(!mouse_monitor::is_session_active(), "停屏期间不能接管输入");
        };

        // 系统调用卡住时不得为连续呼出叠加线程；完成后允许下一份任务。
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let pending = spawn_preparation(move || {
            release_rx.recv().unwrap();
            (Some(expected), None)
        })
        .unwrap();
        assert!(spawn_preparation(|| unreachable!("忙时不得启动第二份任务")).is_none());
        release_tx.send(()).unwrap();
        assert_eq!(
            pending.recv_timeout(Duration::from_secs(1)).unwrap().0,
            Some(expected)
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        while PREPARATION_BUSY.load(Ordering::Acquire) && Instant::now() < deadline {
            pump(8);
        }
        assert!(!PREPARATION_BUSY.load(Ordering::Acquire));

        // 慢准备任务不阻塞 UI；到期后显示，迟到结果不得挪动窗口或换底图。
        let (late_tx, late_rx) = mpsc::sync_channel(1);
        let epoch = app.state.begin_picker_activation();
        win.set_blur_active(false);
        wait_for_preparation(
            &app,
            slint::PhysicalSize::new(360, 420),
            epoch,
            Some(expected),
            Instant::now(),
            Some(late_rx),
        );
        let responsive = Rc::new(std::cell::Cell::new(false));
        let responsive_cb = responsive.clone();
        slint::Timer::single_shot(Duration::from_millis(8), move || responsive_cb.set(true));
        pump(32);
        assert!(responsive.get(), "后台准备期间事件循环必须能响应其他事件");
        parked();
        pump(PREPARATION_BUDGET.as_millis() as u64 + REVEAL_DELAY_MS + 80);
        wait_for_reveal(hwnd, expected);
        let rect = win32_ext::physical_rect(hwnd).unwrap();
        assert_eq!((rect.left, rect.top), (expected.x, expected.y));
        assert!(!win.get_blur_active(), "超时必须使用当前主题实底");
        assert!(
            late_tx
                .send((Some(ScreenPoint { x: 900, y: 900 }), None))
                .is_err(),
            "到期后必须丢弃准备结果接收端"
        );
        hide(&app, false);

        // 重复关闭不能把真实闲置的起点刷新成最近一次空关闭。
        let hidden_at = LAST_HIDDEN_AT.with(std::cell::Cell::get);
        assert!(hidden_at.is_some());
        hide(&app, false);
        assert_eq!(LAST_HIDDEN_AT.with(std::cell::Cell::get), hidden_at);

        // 准备期关闭后再收到结果，不得重新显示。
        let (cancelled_tx, cancelled_rx) = mpsc::sync_channel(1);
        let epoch = app.state.begin_picker_activation();
        wait_for_preparation(
            &app,
            slint::PhysicalSize::new(360, 420),
            epoch,
            Some(expected),
            Instant::now(),
            Some(cancelled_rx),
        );
        hide(&app, false);
        assert!(cancelled_tx.send((Some(expected), None)).is_err());
        pump(PREPARATION_BUDGET.as_millis() as u64 + REVEAL_DELAY_MS + 80);
        parked();

        // 同一实际窗口依次走首次呼出和编辑返回；返回沿用原 session_id。
        for returning in [false, true] {
            let target = app.state.picker_session();
            let open = || {
                if returning {
                    restore_after_editor(&app, target);
                } else {
                    activate(&app);
                }
            };
            open();
            let old_epoch = app.state.picker_session_epoch();
            parked();
            activate(&app);
            parked();
            assert_eq!(
                app.state.picker_session_epoch(),
                old_epoch,
                "重复打开不得重启准备期"
            );
            toggle(&app);
            let saved = app
                .core()
                .repository
                .load_picker_window_state()
                .unwrap()
                .unwrap();
            assert_eq!(
                (saved.x, saved.y, saved.width, saved.height),
                (stored.x, stored.y, stored.width, stored.height),
                "取消准备不能污染几何记忆"
            );

            open();
            assert!(!app.state.is_current_picker_activation(old_epoch));
            pump(REVEAL_DELAY_MS / 2);
            parked();
            pump(REVEAL_DELAY_MS + 80);
            wait_for_reveal(hwnd, expected);
            let rect = win32_ext::physical_rect(hwnd).unwrap();
            assert_eq!((rect.left, rect.top), (expected.x, expected.y));
            assert_eq!((rect.right - rect.left, rect.bottom - rect.top), (360, 420));
            assert!(
                mouse_monitor::is_session_active(),
                "真实显示后必须安装鼠标钩子"
            );
            assert!(window_control::is_topmost(hwnd));
            assert!(TOPMOST_GUARD.with(|slot| slot.borrow().as_ref().unwrap().running()));
            win.set_selected(1);
            let epoch = app.state.picker_session_epoch();
            let state = app.state.clone();
            overlay::after_show(
                hwnd,
                false,
                ForegroundPolicy::Keep,
                ForegroundPolicy::Keep,
                move || state.is_current_picker_activation(epoch),
            );
            hide_for_editor(&app);
            restore_after_editor(&app, app.state.picker_session());
            assert_eq!(win.get_selected(), 1, "编辑返回不得重置选中");
            // 拉开新显示与旧补挂的执行时间，让 release 的短等待也覆盖此交错。
            REVEAL_TIMER.with(|timer| timer.borrow().set_interval(Duration::from_millis(200)));
            // 暂时剥掉置顶位，旧显示的 50ms 补挂不得在新一代准备期重新抬窗。
            use windows::Win32::{
                Foundation::HWND,
                UI::WindowsAndMessaging::{
                    SetWindowPos, HWND_NOTOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                },
            };
            unsafe {
                SetWindowPos(
                    HWND(hwnd as *mut _),
                    Some(HWND_NOTOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                )
                .unwrap();
            }
            pump(80);
            parked();
            assert!(
                !window_control::is_topmost(hwnd),
                "旧代际补挂不能操作新一代窗口"
            );
            hide_for_editor(&app);
            pump(REVEAL_DELAY_MS + 80);
            parked();
            assert!(
                !app.state.is_picker_active(),
                "取消后不能被延迟回调重新打开"
            );
        }

        // 交互桌面可用时检查编辑返回归还前台，以及等待期间切换窗口的保留。
        let previous = ActiveAppResolver::current_foreground_hwnd();
        let source = crate::EditorWindow::new().unwrap();
        let other = SearchWindow::new().unwrap();
        // 焦点契约只需要真实可见窗口；首次 show 前停屏，避免辅助窗在
        // 默认位置闪出原生边框或空白内容。屏外窗口仍可取得前台。
        for window in [source.window(), other.window()] {
            window.set_position(slint::PhysicalPosition::new(-32000, -32000));
        }
        source.show().unwrap();
        other.show().unwrap();
        pump(100);
        let source_hwnd = win32_ext::window_hwnd(&source).unwrap();
        let other_hwnd = win32_ext::window_hwnd(&other).unwrap();
        if ActiveAppResolver::force_foreground_window(other_hwnd) {
            let target = TargetSession {
                target_window_hwnd: Some(source_hwnd),
                ..Default::default()
            };
            restore_after_editor(&app, target);
            assert_eq!(
                ActiveAppResolver::current_foreground_hwnd(),
                Some(source_hwnd),
                "编辑返回应先归还前台"
            );
            assert!(ActiveAppResolver::force_foreground_window(other_hwnd));
            pump(REVEAL_DELAY_MS + 80);
            assert_eq!(
                ActiveAppResolver::current_foreground_hwnd(),
                Some(other_hwnd),
                "显示与补挂不能覆盖等待期的前台切换"
            );
            hide(&app, true);
            pump(80);
            assert_eq!(
                ActiveAppResolver::current_foreground_hwnd(),
                Some(other_hwnd),
                "关闭延迟恢复不能覆盖新前台"
            );
            eprintln!("速贴交互桌面前台恢复契约通过");
        } else {
            eprintln!("无可控交互前台，跳过速贴前台恢复契约");
        }
        for hwnd in [source_hwnd, other_hwnd] {
            let rect = win32_ext::physical_rect(hwnd).unwrap();
            assert_eq!(
                (rect.left, rect.top),
                (-32000, -32000),
                "焦点辅助窗须保持屏外"
            );
        }
        restore_after_editor(&app, app.state.picker_session());
        app.core().begin_quit();
        pump(REVEAL_DELAY_MS + 80);
        parked();
        hide(&app, false);
        if let Some(hwnd) = previous {
            let _ = ActiveAppResolver::restore_foreground_window(hwnd);
        }
    }

    struct TempDirGuard(std::path::PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_preview_with;

    /// 确定性排版模型：每个字符（含省略号）占一格，每 CAPACITY 字符折一行，
    /// 行高 LINE_HEIGHT。足以驱动裁排语义，不依赖真实字体
    const CAPACITY: usize = 10;
    const LINE_HEIGHT: f32 = 20.0;

    fn fake_measure(text: &str, _width: f32) -> f32 {
        let lines = text.chars().count().div_ceil(CAPACITY).max(1);
        lines as f32 * LINE_HEIGHT
    }

    fn clamp(text: &str, max_lines: usize, source_truncated: bool) -> String {
        let mut measure = fake_measure;
        clamp_preview_with(
            &mut measure,
            text,
            1000.0,
            source_truncated,
            LINE_HEIGHT,
            max_lines,
        )
    }

    #[test]
    fn short_text_passes_through_without_ellipsis() {
        let text = "放得下的短文本";
        assert_eq!(clamp(text, 4, false), text);
    }

    #[test]
    fn long_text_is_clamped_within_budget_with_ellipsis() {
        let text = "a".repeat(CAPACITY * 10);
        let result = clamp(&text, 4, false);
        assert!(result.ends_with('…'));
        assert!(fake_measure(&result, 1000.0) <= 4.0 * LINE_HEIGHT + 0.5);
        // 二分取到最大可放前缀：补一个字符即超预算
        let mut longer = result.trim_end_matches('…').to_string();
        longer.push('a');
        longer.push('…');
        assert!(fake_measure(&longer, 1000.0) > 4.0 * LINE_HEIGHT + 0.5);
    }

    #[test]
    fn truncated_source_adds_ellipsis_even_when_text_fits() {
        let text = "放得下但源已截断";
        let result = clamp(text, 4, true);
        assert_eq!(result, format!("{text}…"));
    }

    #[test]
    fn trailing_whitespace_is_trimmed_before_ellipsis() {
        let text = "abc   ";
        let result = clamp(text, 4, true);
        assert_eq!(result, "abc…");
    }

    #[test]
    fn budget_too_small_for_any_prefix_returns_bare_ellipsis() {
        // 0 行预算：连「…」都放不下，走空串兜底返回裸省略号
        let result = clamp("任意文本", 0, false);
        assert_eq!(result, "…");
    }

    #[test]
    fn empty_text_returns_empty() {
        assert_eq!(clamp("", 4, false), "");
    }
}

#[cfg(test)]
pub(crate) mod action_sheet_tests {
    //! 速贴更多操作菜单契约（docs/testing.md L2 层）：菜单
    //! 展开期间会话键先服务浮层——↑↓ 移动高亮并循环，Esc 只收浮层，
    //! 次级键/数字直达收浮层但不上屏；指针点击选中/未选中行的更多按钮
    //! 只开浮层。不执行真实上屏（系统剪贴板属 L3 串行资源）。
    //! 由 l2_headless_ui_contracts 单线程串行调用。

    use super::*;
    use std::sync::Arc;

    use floatpaste_core::repository::sqlite_repository::SqliteRepository;
    use floatpaste_core::services::image_storage::ImageStorage;
    use floatpaste_core::state::CoreState;

    use crate::app_state::SharedState;

    fn file_summary() -> ClipItemSummary {
        ClipItemSummary {
            has_html: false,
            id: "sheet-file".into(),
            r#type: "file".into(),
            content_preview: r"C:\dir\a.txt".into(),
            source_app: None,
            is_favorited: false,
            file_count: 1,
            directory_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            image_path: None,
            image_width: None,
            image_height: None,
            image_format: None,
            file_size: None,
            tags: vec![],
            text_kind: Default::default(),
        }
    }

    /// 单测试函数串行多段中的一段（见模块注释）
    pub(crate) fn run() {
        let stamp = std::process::id();
        let dir = std::env::temp_dir().join(format!("floatpaste-sheet-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let repository = SqliteRepository::new(&dir.join("floatpaste.db")).unwrap();
        let core = CoreState::new(
            repository,
            ImageStorage::new(dir).unwrap(),
            UserSetting::default(),
        );
        let win = QuickPasteWindow::new().expect("实例化速贴窗失败");
        let app = App {
            state: Arc::new(SharedState::new(core)),
            picker: win.as_weak(),
            tooltip: Default::default(),
            tray_menu: Default::default(),
            search: Default::default(),
            editor: Default::default(),
            settings: Default::default(),
        };
        app.state.set_items(vec![file_summary()]);

        // 展开：选中切到该行，高亮回主动作（目录首项）
        open_actions(&app, 0);
        assert_eq!(win.get_actions_row(), 0);
        assert_eq!(win.get_action_selected(), 0);
        assert_eq!(win.get_selected(), 0);

        // 文件条目菜单 = 4 项动作目录（含「打开所在文件夹」）+ 3 项管理操作
        use slint::Model;
        assert_eq!(win.get_menu_actions().row_count(), 7);
        let reveal = win.get_menu_actions().row_data(3).unwrap();
        assert_eq!(reveal.label, "打开所在文件夹");
        assert_eq!(reveal.shortcut, "");

        // ↑↓ 在 7 项间循环
        use floatpaste_core::platform::windows::session_keyboard::SessionAction;
        handle_session_action_with_actions(&app, 0, SessionAction::NavigateDown);
        assert_eq!(win.get_action_selected(), 1);
        for _ in 0..6 {
            handle_session_action_with_actions(&app, 0, SessionAction::NavigateDown);
        }
        assert_eq!(win.get_action_selected(), 0, "高亮循环回主动作");
        handle_session_action_with_actions(&app, 0, SessionAction::NavigateUp);
        assert_eq!(win.get_action_selected(), 6, "向上移动循环到删除");

        // 激活「打开所在文件夹」收起菜单；该测试条目无库详情，读取
        // 详情即失败，不会真开资源管理器（真开属桌面验收）
        activate_menu_item(&app, 0, 3);
        assert_eq!(win.get_actions_row(), -1);

        // 次级键与数字直达：只收浮层，不上屏（无剪贴板写入即通过）
        open_actions(&app, 0);
        handle_session_action_with_actions(&app, 0, SessionAction::ConfirmAsPath);
        assert_eq!(win.get_actions_row(), -1);

        // Esc 同样只收浮层；面板开关状态不在本契约范围
        open_actions(&app, 0);
        handle_session_action_with_actions(&app, 0, SessionAction::Dismiss);
        assert_eq!(win.get_actions_row(), -1);

        pointer_actions_button(&app, &win);
        equal_height_cards(&app, &win);
        deletion_interactions(&app, &win);

        let _ =
            std::fs::remove_dir_all(std::env::temp_dir().join(format!("floatpaste-sheet-{stamp}")));
    }

    fn pointer_actions_button(app: &App, win: &QuickPasteWindow) {
        use slint::platform::{PointerEventButton, WindowEvent};

        let mut second = file_summary();
        second.id = "sheet-file-2".into();
        let items = vec![file_summary(), second];
        app.state.set_items(items.clone());
        win.window().set_size(slint::LogicalSize::new(360.0, 420.0));
        build_rows(app, win, &items);
        win.set_selected(0);

        let hovered = Rc::new(std::cell::Cell::new(-1));
        let hovered_cb = hovered.clone();
        win.on_row_hover(move |row, _, _| hovered_cb.set(row));
        let row_clicks = Rc::new(std::cell::Cell::new(0));
        let clicks_cb = row_clicks.clone();
        win.on_row_clicked(move |_| clicks_cb.set(clicks_cb.get() + 1));
        let app_cb = app.clone();
        win.on_row_actions_requested(move |row| open_actions(&app_cb, row as usize));

        // 用实际布局的悬停事件确定行顶部，再命中右上方更多按钮。
        let _ = win.window().take_snapshot().unwrap();
        let mut bounds = [(i32::MAX, 0); 2];
        for y in 40..320 {
            hovered.set(-1);
            win.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(100.0, y as f32),
            });
            if let Some(bound) = bounds.get_mut(hovered.get() as usize) {
                bound.0 = bound.0.min(y);
                bound.1 = y;
            }
        }
        for (row, (top, bottom)) in bounds.into_iter().enumerate() {
            assert!(top < bottom, "应找到第 {row} 行的完整命中区域");
            let y = (top + bottom) as f32 / 2.0;
            win.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(100.0, y),
            });
            let position = slint::LogicalPosition::new(318.0, top as f32 + 20.0);
            win.window()
                .dispatch_event(WindowEvent::PointerMoved { position });
            let _ = win.window().take_snapshot().unwrap();
            win.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            win.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
            assert_eq!(row_clicks.get(), 0, "更多按钮不能触发行点击上屏");
            assert_eq!(win.get_actions_row(), row as i32, "应打开对应行的浮层");
            assert_eq!(win.get_selected(), row as i32);
            close_actions(app);
        }
    }

    fn deletion_interactions(app: &App, win: &QuickPasteWindow) {
        use floatpaste_core::services::normalize_service::NormalizeService;
        use session_keyboard::SessionAction;
        use slint::platform::{PointerEventButton, WindowEvent};

        for text in ["速贴删除甲", "速贴删除乙", "速贴删除丙"] {
            let item = NormalizeService::normalize_text(text, None).unwrap();
            app.core().repository.save_text_item(&item).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        wire(app);
        win.window().set_size(slint::LogicalSize::new(360.0, 420.0));
        refresh_list(app, false);
        let mut settings = app.state.current_settings();
        settings.session_keys.delete_entry = "Ctrl+L".into();
        settings.session_keys.dismiss = "Ctrl+Escape".into();
        settings.session_keys.confirm = "Alt+Enter".into();
        settings.session_keys.open_editor = "Ctrl+Shift+E".into();
        settings.session_keys.toggle_favorite = "Ctrl+Shift+D".into();
        settings.picker_digit_shortcuts_enabled = false;
        app.core().update_settings(settings).unwrap();
        refresh_copy(app);
        assert_eq!(win.get_delete_shortcut(), "Ctrl+L");
        assert_eq!(win.get_header_hint(), "Alt+Enter 粘贴为文本");
        assert_eq!(win.get_rows().row_data(0).unwrap().digit, "");
        request_delete(app, 0);
        assert_eq!(win.get_delete_armed_row(), -1, "失活时不能删除");
        app.state.begin_picker_activation();
        set_selected(app, 1);
        handle_session_action(app, SessionAction::DeleteEntry);
        assert_eq!(win.get_delete_armed_row(), 1);
        assert_eq!(win.get_header_hint(), "Ctrl+L 确认删除 · Ctrl+Escape 取消");
        handle_session_action(app, SessionAction::Dismiss);
        assert_eq!(win.get_delete_armed_row(), -1);
        assert!(app.state.is_picker_active(), "Esc 先取消确认，不关闭面板");
        request_delete(app, 1);
        set_selected(app, 0);
        assert_eq!(win.get_delete_armed_row(), -1, "切换条目取消确认");
        request_delete(app, 0);
        open_actions(app, 0);
        assert_eq!(win.get_delete_armed_row(), -1, "打开浮层取消确认");
        handle_session_action(app, SessionAction::DeleteEntry);
        assert_eq!(win.get_actions_row(), 0);
        assert_eq!(win.get_delete_armed_row(), 0, "Delete 在浮层内确认发起行");
        refresh_list(app, true);
        assert_eq!(win.get_delete_armed_row(), -1, "刷新取消确认，防止行号漂移");

        // 即使事件循环尚未执行超时回调，超时的第二击也只能重新确认。
        request_delete(app, 0);
        ARMED_DELETE.with(|slot| {
            slot.borrow_mut().as_mut().unwrap().1 = Instant::now() - DELETE_ARM_TIMEOUT
        });
        request_delete(app, 0);
        assert_eq!(app.state.items().len(), 3);
        slint::Timer::single_shot(DELETE_ARM_TIMEOUT + Duration::from_millis(50), || {
            slint::quit_event_loop().unwrap();
        });
        slint::run_event_loop_until_quit().unwrap();
        assert_eq!(win.get_delete_armed_row(), -1, "超时自动解除确认");

        close_actions(app);
        set_selected(app, 1);
        let hovered = Rc::new(std::cell::Cell::new(-1));
        let hovered_cb = hovered.clone();
        win.on_row_hover(move |row, _, _| hovered_cb.set(row));
        let clicks = Rc::new(std::cell::Cell::new(0));
        let clicks_cb = clicks.clone();
        win.on_row_clicked(move |_| clicks_cb.set(clicks_cb.get() + 1));
        let double_clicks_cb = clicks.clone();
        win.on_row_double_clicked(move |_| double_clicks_cb.set(double_clicks_cb.get() + 1));
        let hint = Rc::new(std::cell::RefCell::new(String::new()));
        let hint_cb = hint.clone();
        win.on_button_hint(move |text, _, _| *hint_cb.borrow_mut() = text.to_string());
        let _ = win.window().take_snapshot().unwrap();
        let top = (0..180)
            .find(|y| {
                hovered.set(-1);
                win.window().dispatch_event(WindowEvent::PointerMoved {
                    position: slint::LogicalPosition::new(100.0, *y as f32),
                });
                hovered.get() == 0
            })
            .expect("找到首行顶部");
        let position = slint::LogicalPosition::new(318.0, top as f32 + 20.0);
        win.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        let _ = win.window().take_snapshot().unwrap();
        assert_eq!(hint.borrow().as_str(), "更多操作", "普通文本也有更多入口");
        win.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        win.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        assert_eq!(clicks.get(), 0, "更多按钮不能触发行点击或双击上屏");
        assert_eq!(win.get_actions_row(), 0);
        use slint::Model;
        assert_eq!(win.get_menu_actions().row_count(), 4);
        assert_eq!(
            win.get_menu_actions().row_data(0).unwrap().shortcut,
            "Alt+Enter"
        );
        assert_eq!(
            win.get_menu_actions().row_data(1).unwrap().shortcut,
            "Ctrl+Shift+D"
        );
        assert_eq!(
            win.get_menu_actions().row_data(2).unwrap().shortcut,
            "Ctrl+Shift+E"
        );
        assert_eq!(
            win.get_menu_actions().row_data(3).unwrap().shortcut,
            "Ctrl+L"
        );
        activate_menu_item(app, 0, 1);
        assert!(
            app.state.item_at(0).unwrap().is_favorited,
            "菜单收藏更新条目"
        );
        assert_eq!(win.get_actions_row(), -1);
        open_actions(app, 0);
        assert_eq!(
            win.get_menu_actions().row_data(1).unwrap().label,
            "取消收藏"
        );
        slint::Timer::single_shot(Duration::from_millis(180), || {
            slint::quit_event_loop().unwrap()
        });
        slint::run_event_loop_until_quit().unwrap();
        // 菜单底部删除项，首次点击不关闭菜单或上屏。
        let position = slint::LogicalPosition::new(100.0, 385.0);
        win.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        win.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        win.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        assert_eq!(win.get_delete_armed_row(), 0);
        assert_eq!(win.get_actions_row(), 0);
        assert_eq!(clicks.get(), 0, "菜单删除不能触发行上屏");

        if std::env::var_os("FLOATPASTE_UI_SNAPSHOTS").is_some() {
            for (mode, resolved) in [
                ("light", theme::ResolvedTheme::Light),
                ("dark", theme::ResolvedTheme::Dark),
            ] {
                theme_bridge::reapply_theme(app, &theme::derive_tokens("default", "", resolved));
                for armed in [false, true] {
                    win.set_delete_armed_row(if armed { 0 } else { -1 });
                    slint::Timer::single_shot(Duration::from_millis(180), || {
                        slint::quit_event_loop().unwrap()
                    });
                    slint::run_event_loop_until_quit().unwrap();
                    let pixels = win.window().take_snapshot().unwrap();
                    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                        "../../.artifacts/snapshot/picker-more-{mode}-{armed}-{}.png",
                        (win.window().scale_factor() * 100.0).round() as u32
                    ));
                    image::save_buffer(
                        path,
                        pixels.as_bytes(),
                        pixels.width(),
                        pixels.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
        reset_delete_arm(app);
        // 较矮窗口内，删除键应把最后一项滚入视口，点击仍命中删除。
        win.window().set_size(slint::LogicalSize::new(280.0, 180.0));
        open_actions(app, 0);
        handle_session_action(app, SessionAction::DeleteEntry);
        slint::Timer::single_shot(Duration::from_millis(180), || {
            slint::quit_event_loop().unwrap()
        });
        slint::run_event_loop_until_quit().unwrap();
        let activated = Rc::new(std::cell::Cell::new(-1));
        let activated_cb = activated.clone();
        win.on_action_item_clicked(move |index| activated_cb.set(index));
        let position = slint::LogicalPosition::new(100.0, 145.0);
        win.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        win.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        win.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        assert_eq!(activated.get(), 3, "窄窗口可滚动并点击菜单末项");
        handle_session_action(app, SessionAction::Dismiss);
        assert_eq!(win.get_delete_armed_row(), -1);
        assert_eq!(win.get_actions_row(), 0, "Esc 先取消确认");
        handle_session_action(app, SessionAction::Dismiss);
        assert_eq!(win.get_actions_row(), -1, "再次 Esc 只收菜单");
        assert!(app.state.is_picker_active());
        activate_menu_item(app, 0, 99);
        assert_eq!(app.state.items().len(), 3, "越界菜单项不能回退成上屏动作");
        wire(app);
        win.window().set_size(slint::LogicalSize::new(360.0, 420.0));
        set_selected(app, 1);
        let expected_next = app.state.item_at(2).unwrap().id;
        request_delete(app, 1);
        handle_session_action(app, SessionAction::Confirm);
        assert_eq!(win.get_selected(), 1);
        assert_eq!(
            app.state.selected_id(),
            Some(expected_next),
            "删除中间项后选中下一条"
        );
        request_delete(app, 1);
        request_delete(app, 1);
        assert_eq!(win.get_selected(), 0, "删除末项后选中上一条");
        request_delete(app, 0);
        request_delete(app, 0);
        assert!(app.state.items().is_empty());
        assert_eq!(app.state.selected_id(), None);
        assert!(app.state.is_picker_active(), "删空仍保持面板打开");
        request_delete(app, 0);
        assert_eq!(win.get_delete_armed_row(), -1, "空列表不能进入确认");
        app.state.end_picker_activation();
    }

    fn equal_height_cards(app: &App, win: &QuickPasteWindow) {
        use slint::platform::WindowEvent;
        use slint::Model;

        let mut items = vec![file_summary(); 4];
        items[0].r#type = "text".into();
        items[0].content_preview = "短文本".into();
        items[1].r#type = "text".into();
        items[1].has_html = true;
        items[1].content_preview = "多行中文与 emoji 😀\n".repeat(12);
        items[1].tags = vec![
            "一个需要省略的长标签名称".into(),
            "工作".into(),
            "常用".into(),
        ];
        items[1].is_favorited = true;
        items[1].created_at = "2026-01-01T00:00:00Z".into();
        items[1].source_app = Some("一个需要省略的很长的来源应用名称".into());
        items[2].r#type = "image".into();
        items[2].content_preview = "图片预览".into();
        items[3].r#type = "text".into();
        items[3].text_kind = TextKind::Code;
        items[3].content_preview = "fn main() {\n    println!(\"Hello, 浮贴 😀\");\n}".into();
        app.state.set_items(items.clone());
        let hovered = Rc::new(std::cell::Cell::new(-1));
        let hovered_cb = hovered.clone();
        win.on_row_hover(move |row, _, _| hovered_cb.set(row));

        for width in [360.0, 280.0] {
            win.window().set_size(slint::LogicalSize::new(width, 600.0));
            build_rows(app, win, &items);
            let rows = win.get_rows();
            assert_eq!(rows.row_data(1).unwrap().tags.row_count(), 1);
            assert_eq!(rows.row_data(1).unwrap().tag_more, 2);
            assert!(rows.row_data(1).unwrap().preview.ends_with('…'));
            assert!(rows.row_data(3).unwrap().is_code);
            assert!(rows.row_data(3).unwrap().preview.contains("\n    "));
            for with_thumb in [false, true] {
                let mut image_row = rows.row_data(2).unwrap();
                image_row.has_thumb = with_thumb;
                image_row.thumb = slint::Image::from_rgba8(slint::SharedPixelBuffer::new(60, 60));
                rows.set_row_data(2, image_row);
                win.set_selected(if with_thumb { 1 } else { 0 });
                close_actions(app);
                let _ = win.window().take_snapshot().unwrap();
                let mut bounds = [(i32::MAX, 0); 4];
                for y in 0..580 {
                    hovered.set(-1);
                    win.window().dispatch_event(WindowEvent::PointerMoved {
                        position: slint::LogicalPosition::new(100.0, y as f32),
                    });
                    if let Some(bound) = bounds.get_mut(hovered.get() as usize) {
                        bound.0 = bound.0.min(y);
                        bound.1 = y;
                    }
                }
                let expected = win.global::<PanelGeometry>().get_row_height();
                for (row, (top, bottom)) in bounds.into_iter().enumerate() {
                    assert!(top < bottom, "应找到第 {row} 行的完整命中区域");
                    assert!(
                        ((bottom - top + 1) as f32 - expected).abs() <= 1.0,
                        "行 {row} 在宽度 {width}、缩略图 {with_thumb} 时应保持等高：{top}..={bottom}"
                    );
                }
                if with_thumb && std::env::var_os("FLOATPASTE_UI_SNAPSHOTS").is_some() {
                    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../.artifacts/snapshot");
                    std::fs::create_dir_all(&dir).unwrap();
                    for (mode, resolved) in [
                        ("light", theme::ResolvedTheme::Light),
                        ("dark", theme::ResolvedTheme::Dark),
                    ] {
                        theme_bridge::reapply_theme(
                            app,
                            &theme::derive_tokens("default", "", resolved),
                        );
                        for armed in [false, true] {
                            if armed {
                                open_actions(app, 1);
                                win.set_delete_armed_row(1);
                                win.set_action_selected(
                                    win.get_menu_actions().row_count() as i32 - 1,
                                );
                            } else {
                                close_actions(app);
                            }
                            slint::Timer::single_shot(Duration::from_millis(180), || {
                                slint::quit_event_loop().unwrap()
                            });
                            slint::run_event_loop_until_quit().unwrap();
                            let pixels = win.window().take_snapshot().unwrap();
                            image::save_buffer(
                                dir.join(format!(
                                    "picker-equal-height-{width}-{mode}{}-{}.png",
                                    if armed { "-menu" } else { "-idle" },
                                    (win.window().scale_factor() * 100.0).round() as u32
                                )),
                                pixels.as_bytes(),
                                pixels.width(),
                                pixels.height(),
                                image::ColorType::Rgba8,
                            )
                            .unwrap();
                            close_actions(app);
                        }
                    }
                }
            }
        }
        win.set_delete_armed_row(-1);

        // 「速贴卡片高度」设置项：注入非默认总高后，行高与裁排行数预算
        // 跟随变化且各行保持等高；结束后恢复默认几何不影响后续用例
        win.window().set_size(slint::LogicalSize::new(360.0, 760.0));
        apply_card_height(win, 140);
        build_rows(app, win, &items);
        let geo = win.global::<PanelGeometry>();
        assert_eq!(geo.get_row_height(), 140.0);
        assert!(
            geo.get_preview_max_lines() > 3,
            "更高卡片的预览行数预算应增加"
        );
        win.set_selected(0);
        let _ = win.window().take_snapshot().unwrap();
        let mut tall_bounds = [(i32::MAX, 0); 4];
        for y in 0..740 {
            hovered.set(-1);
            win.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(100.0, y as f32),
            });
            if let Some(bound) = tall_bounds.get_mut(hovered.get() as usize) {
                bound.0 = bound.0.min(y);
                bound.1 = y;
            }
        }
        for (row, (top, bottom)) in tall_bounds.into_iter().enumerate() {
            assert!(top < bottom, "应找到第 {row} 行的完整命中区域");
            assert!(
                ((bottom - top + 1) as f32 - 140.0).abs() <= 1.0,
                "行 {row} 在卡片高度 140 时应保持等高：{top}..={bottom}"
            );
        }
        apply_card_height(win, app.state.current_settings().picker_card_height);
    }
}
