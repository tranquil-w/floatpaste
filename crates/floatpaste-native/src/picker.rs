//! 速贴面板会话逻辑：显示/隐藏/切换、列表刷新、确认上屏、收藏、导航。
//!
//! 行为逐项对齐原版 WindowCoordinator / ShortcutManager / PickerShell：
//! - 显隐不抢焦点（WS_EX_NOACTIVATE，hide/show 一律以屏幕外停屏复现），
//!   并把前台还给目标窗口；
//! - 会话期键盘由 LL 钩子接管（长按连发在 core::session_keyboard）；
//! - 外击关闭经 WH_MOUSE_LL 钩子；
//! - 尺寸/位置持久化与三种定位模式在 core::PickerPositionService。

use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tracing::{info, warn};

use floatpaste_core::domain::clip_item::{ClipItemSummary, PasteOption};
use floatpaste_core::domain::settings::{PasteTrigger, UserSetting};
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::window_control::{self, GestureMode, ResizeDirection};
use floatpaste_core::platform::windows::{mouse_monitor, session_keyboard};
use floatpaste_core::services::clip_display::clip_type_label;
use floatpaste_core::services::clip_service::ClipService;
use floatpaste_core::services::picker_position_service::{
    default_window_size, resolve_near_cursor, PickerPositionService, WindowGeometry,
    PICKER_MIN_HEIGHT, PICKER_MIN_WIDTH,
};
use floatpaste_core::services::time_format::format_relative_time_or_unused;
use floatpaste_core::state::CoreState;
use floatpaste_core::theme;

use crate::app_state::{SharedState, TargetSession};
use crate::overlay::{self, ForegroundPolicy};
use crate::paste_flow;
use crate::search;
use crate::theme_bridge;
use crate::thumbnails;
use crate::tooltip;
use crate::win32_ext;
use crate::{ClipRow, PanelGeometry, QuickPasteWindow, SearchWindow, TooltipWindow};

/// 预览最多显示行数（对齐原版 line-clamp-4）
const PREVIEW_MAX_LINES: usize = 4;
/// 入库预览的字符截断上限（normalize_service 同值）：达到即认为原文更长
const PREVIEW_SOURCE_LIMIT: usize = 120;

/// 事件循环线程上的应用上下文（克隆廉价，闭包捕获后经 invoke 回到事件循环）
#[derive(Clone)]
pub struct App {
    pub state: Arc<SharedState>,
    pub picker: slint::Weak<QuickPasteWindow>,
    pub tooltip: slint::Weak<TooltipWindow>,
    pub search: slint::Weak<SearchWindow>,
    pub editor: slint::Weak<crate::EditorWindow>,
    pub settings: slint::Weak<crate::SettingsWindow>,
}

impl App {
    pub fn with_picker<R>(&self, f: impl FnOnce(&QuickPasteWindow) -> R) -> Option<R> {
        self.picker.upgrade().map(|win| f(&win))
    }

    pub fn with_search<R>(&self, f: impl FnOnce(&SearchWindow) -> R) -> Option<R> {
        self.search.upgrade().map(|win| f(&win))
    }

    pub fn core(&self) -> &CoreState {
        &self.state.core
    }
}

/* ───────────────── 显示 / 隐藏 / 切换 ───────────────── */

// 会话期置顶位守护：速贴靠 WS_EX_TOPMOST 压住普通窗口。两种失效都要
// 兜住——① winit 异步样式重排/系统操作把置顶位剥掉；② 另一个置顶
// 窗口在速贴之后抬升（topmost 带内后来者在上有权盖住速贴）。会话期间
// 每 500ms 检查一次，失效即重抬；会话结束停表。仅在事件循环线程触达。
thread_local! {
    static TOPMOST_GUARD: std::cell::RefCell<Option<slint::Timer>> =
        const { std::cell::RefCell::new(None) };
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
        // 兜底：标志位与真实可见性脱节时（Win+D/多屏切换/被全屏应用抢占）
        // 重置状态后走完整显示流程，避免后续打开沦为空操作
        if window_control::is_window_visible(hwnd) {
            // 尺寸不动：用窗口当前实测尺寸（Slint 的 size() 即物理像素，
            // 不要再乘缩放因子，乘了约束用的尺寸会虚大）
            apply_window_position(app, &settings, win.window().size(), hwnd, None);
            // 该路径绕过 after_show：置顶与守护需自行保证
            window_control::set_window_topmost_no_activate(hwnd);
            start_topmost_guard(app);
            return;
        }
        warn!("Picker 标志位为激活但窗口实际不可见，重置状态后重新显示");
        app.state.end_picker_activation();
        session_keyboard::end_session();
        mouse_monitor::end_session();
    }

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

    // 捕获目标窗口（显示后恢复其前台，粘贴时回贴）
    let focus_target = ActiveAppResolver::current_foreground_focus_target();
    let session = TargetSession {
        target_window_hwnd: focus_target.window_hwnd,
        target_focus_hwnd: focus_target.focus_hwnd,
    };
    app.state.set_picker_session(session);
    info!(
        "显示 Picker，hwnd={hwnd}, target_window={:?}, target_focus={:?}",
        session.target_window_hwnd, session.target_focus_hwnd
    );

    // 空态快捷键提示跟随当前设置（用户改过热键后提示不失效）
    win.set_empty_shortcut(settings.shortcut.clone().into());

    // 主题随设置刷新（设置可能在后台被改变）
    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
    theme_bridge::apply_theme(
        Some(&win),
        app.tooltip.upgrade().as_ref(),
        app.search.upgrade().as_ref(),
        app.editor.upgrade().as_ref(),
        app.settings.upgrade().as_ref(),
        &tokens,
    );

    // 速贴无焦点窗：SWCA HOSTBACKDROP + SystemBackdrop Acrylic（非前台
    // 不降级，材质与搜索窗同源）；面板底用 material-layer 近实心压住
    // 渗出，材质感以 ~10% 保留
    let material_active = overlay::apply_material(app, hwnd, overlay::MaterialSurface::HostAcrylic);
    info!("速贴材质挂载: active={material_active}");
    win.set_material_active(material_active);

    app.state.begin_picker_activation();

    // 重现（无激活 + 置顶，对齐原版 always_on_top）。窗口自启动起保持
    // Slint 可见（停屏态），这里只做最小化兜位恢复，不走 Slint show：
    // hide→show 周期中 winit 清空表面且脏区跟踪失效，是开窗透明闪烁的
    // 根源。样式重挂与前台策略仍交 overlay 公共层处理
    let _ = window_control::show_window_no_activate(hwnd);

    // 出现滑入准备：停屏期关动画重置内容偏移，暖首帧落盘的即 4px 起步态
    // （见 picker.slint enter-offset 注释）
    win.set_enter_animated(false);
    win.set_enter_offset(4.0);

    // 会话开始：刷新列表、选中归零、滚回顶部、清空消息。放在上屏之前：
    // 列表重活（查询 + 逐行裁排）跑在停屏期间，上屏即最新内容
    refresh_list_reset(app, &settings);

    // 上屏前先暖表面：停屏窗口收不到自发 WM_PAINT，同步泵一次呈现，
    // 让数据就绪后的首帧在屏外落盘——移回屏上的第一帧即完整内容，
    // 既不透明也不闪旧列表
    win32_ext::warm_surface(hwnd);
    apply_window_position(app, &settings, size, hwnd, session.target_window_hwnd);
    win32_ext::force_full_repaint(hwnd);

    // 上屏即滑入：解锁动画归零，内容从 4px 滑到位（窗底不动，不露窗外）
    win.set_enter_animated(true);
    win.set_enter_offset(0.0);

    let immediate = session
        .target_window_hwnd
        .map_or(ForegroundPolicy::Keep, ForegroundPolicy::Restore);
    let deferred = session
        .target_window_hwnd
        .map_or(ForegroundPolicy::Keep, ForegroundPolicy::RestoreIfStolen);
    overlay::after_show(hwnd, false, immediate, deferred);
    start_topmost_guard(app);

    // 搜索窗口正被作为回贴目标（用户在搜索中按了主快捷键）：键盘交给
    // 速贴会话，搜索侧输入框失焦、底栏切换为速贴键位（对齐
    // SEARCH_INPUT_SUSPEND_EVENT 语义）
    if session.target_window_hwnd == Some(app.state.search_hwnd.load(Ordering::SeqCst)) {
        search::suspend_input(app);
    }

    // 键鼠会话必须最后装配：LL 鼠标钩子回调由安装线程（事件循环）
    // 泵出，列表查询与逐行裁排若在安装之后运行，会阻塞回调泵送、
    // 造成全系统光标短暂冻结
    begin_input_session(app, hwnd, &settings);
}

/// 隐藏（对齐 WindowCoordinator::hide_picker + hide_picker_and_restore_target）
pub fn hide(app: &App, restore_target: bool) {
    stop_topmost_guard();
    let Some(win) = app.picker.upgrade() else {
        return;
    };
    let hwnd = app
        .state
        .picker_hwnd
        .load(std::sync::atomic::Ordering::SeqCst);

    session_keyboard::end_session();
    mouse_monitor::end_session();

    if let Some(rect) = (hwnd != 0)
        .then(|| win32_ext::physical_rect(hwnd))
        .flatten()
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

    // 停屏到屏幕外而非 Slint hide（同 hide_for_editor）：hide→show 周期中
    // winit 清空窗口表面且 Slint 脏区跟踪失效，下次显示会出现透明空壳；
    // 停屏保持表面内容有效，重现即原样恢复
    win.window()
        .set_position(slint::PhysicalPosition::new(-32000, -32000));

    app.state.end_picker_activation();
    tooltip::cancel(app);
    info!("隐藏 Picker");

    if restore_target {
        let session = app.state.picker_session();
        let quitting = app.core().clone();
        let app_for_resume = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if quitting.is_quitting() {
                return;
            }
            if let Some(target_hwnd) = session.target_window_hwnd {
                let _ = ActiveAppResolver::restore_foreground_window(target_hwnd);
                // 目标是搜索窗口：把输入焦点还给搜索框（SEARCH_INPUT_RESUME）
                if target_hwnd == app_for_resume.state.search_hwnd.load(Ordering::SeqCst) {
                    let _ = slint::invoke_from_event_loop(move || {
                        search::resume_input(&app_for_resume);
                    });
                }
            }
        });
    }
}

/// 进入编辑器前的收起：会话/监视结束、几何记忆、停激活、收 tooltip。
/// 隐藏**不能**走 Win32 SW_HIDE 或 Slint hide：
/// - Slint hide 让适配层认为窗口已隐藏并停止渲染；
/// - Win32 隐藏期间 winit 抑制重绘，且 Slint 不知道窗口经历过直接
///   显隐——之后无论 Win32 还是 Slint 重现，都只会得到透明空壳。
/// 因此改为把窗口平移到屏幕外：Slint 全程视为已显示、持续渲染，
/// 表面内容始终有效，返回时移回原位即恢复
pub fn hide_for_editor(app: &App) {
    let Some(win) = app.picker.upgrade() else {
        return;
    };
    let hwnd = app
        .state
        .picker_hwnd
        .load(std::sync::atomic::Ordering::SeqCst);

    session_keyboard::end_session();
    mouse_monitor::end_session();

    if let Some(rect) = (hwnd != 0)
        .then(|| win32_ext::physical_rect(hwnd))
        .flatten()
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

    // 停屏到屏幕外（Windows 坐标下限），不用隐藏
    win.window()
        .set_position(slint::PhysicalPosition::new(-32000, -32000));
    app.state.end_picker_activation();
    stop_topmost_guard();
    tooltip::cancel(app);
    info!("隐藏 Picker（进入编辑器）");
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

/// 从编辑器返回速贴：恢复会话快捷键与外击关闭，窗口以无激活方式重现，
/// **故意不重置列表与选中**（对齐旧版 restore_picker_after_editor 不发
/// SESSION_START，保留进入编辑器时的上下文与滚动位置）。前台留在用户
/// 此前所在位置，键盘会话经 LL 钩子接回，无需窗口持有焦点
pub fn restore_after_editor(app: &App, target: TargetSession) {
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
    let settings = app.state.current_settings();

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
    // 出现滑入准备：同 activate，停屏期无动画重置内容偏移，暖首帧即起步态
    win.set_enter_animated(false);
    win.set_enter_offset(4.0);
    // 上屏前先暖表面（同步泵一次 WM_PAINT 呈现），移回即有内容
    win32_ext::warm_surface(hwnd);
    apply_window_position(app, &settings, size, hwnd, target.target_window_hwnd);

    app.state.begin_picker_activation();

    // 无激活兜位重现（对齐旧版 show_window_no_activate）：编辑期间窗口
    // 只是停屏，移回屏上后表面内容原样有效；这里顺带从最小化恢复。
    // 主题随设置刷新照常执行
    let _ = window_control::show_window_no_activate(hwnd);
    // 上屏即滑入（同 activate）
    win.set_enter_animated(true);
    win.set_enter_offset(0.0);

    // 主题随设置刷新（编辑期间设置可能已被外部修改）
    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
    theme_bridge::apply_theme(
        Some(&win),
        app.tooltip.upgrade().as_ref(),
        app.search.upgrade().as_ref(),
        app.editor.upgrade().as_ref(),
        app.settings.upgrade().as_ref(),
        &tokens,
    );
    // 键鼠会话最后装配（理由同 activate：LL 钩子回调泵送不能被阻塞）
    begin_input_session(app, hwnd, &settings);
    info!("从 Editor 返回 Picker");
}

fn begin_input_session(app: &App, hwnd: isize, settings: &UserSetting) {
    let app_for_mouse = app.clone();
    // 外击关闭不还原前台：用户点击的位置已取得焦点，此时再把原目标
    // 拉回前台会被前台所有权检查拒绝，Windows 拒绝时闪烁目标窗口的
    // 任务栏按钮（Esc/主快捷键关闭仍走还原，见 hide）
    mouse_monitor::begin_session(
        hwnd,
        Box::new(move || {
            let app = app_for_mouse.clone();
            let _ = slint::invoke_from_event_loop(move || {
                hide(&app, false);
            });
        }),
    );

    let app_for_keys = app.clone();
    session_keyboard::begin_session(
        session_keyboard::SessionKeyConfig::from_settings(settings),
        Box::new(move |action| {
            let app = app_for_keys.clone();
            let _ = slint::invoke_from_event_loop(move || {
                handle_session_action(&app, action);
            });
        }),
    );
}

fn handle_session_action(app: &App, action: session_keyboard::SessionAction) {
    use session_keyboard::SessionAction;
    if !app.state.is_picker_active() {
        // 钩子常驻，会话残留动作（如窗口消失瞬间的按键）在此过滤；
        // 留痕用于诊断「面板已显但按键无响应」类问题
        info!("会话动作忽略（面板非活跃）: {action:?}");
        return;
    }
    match action {
        SessionAction::NavigateUp => navigate(app, true),
        SessionAction::NavigateDown => navigate(app, false),
        SessionAction::Confirm => confirm(app, current_index(app), false),
        SessionAction::ConfirmAsPath => confirm(app, current_index(app), true),
        SessionAction::Dismiss => hide(app, true),
        SessionAction::ToggleFavorite => toggle_favorite(app),
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
        win.set_message_text("".into());
        win.set_message_tone(0);
        win.set_empty_shortcut(settings.shortcut.clone().into());
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

/// 编辑器内保存/删除/改标签后的刷新：速贴此时处于失活态，
/// 跳过活跃判定强制重排，保证返回时列表已反映变更
pub fn refresh_after_editor(app: &App) {
    refresh_list(app, true);
}

/// 重新查询列表并重建行模型。
/// keep_anchor=true 时保持当前选中（按 id 恢复）；false 时选中归零
fn refresh_list(app: &App, keep_anchor: bool) {
    let settings = app.state.current_settings();
    let limit = settings.picker_record_limit.clamp(9, 1000);

    match app.core().repository.list_recent(limit) {
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

            build_rows(&win, &items);
            ensure_thumbnails(app, &items);
        }
        Err(error) => {
            warn!("加载速贴列表失败: {error}");
            let message = error.to_string();
            app.with_picker(|win| {
                win.set_load_failed(true);
                win.set_load_error_text(message.into());
            });
        }
    }
}

thread_local! {
    /// 列表版本号：异步缩略图回来时校验列表是否已变
    /// （缓存本体在 thumbnails 模块，速贴与搜索共用）
    static LIST_VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

// 已知规模边界：行模型全量重建，且每行各带一把隐藏度量标尺；当前
// picker_record_limit（9..=1000）内够用，若要放开上限需先做裁排结果
// 缓存与模型 diff 更新，否则软件渲染与逐行度量会成为刷新瓶颈

/// 预览单行排版高度（逻辑像素）：同一文本一行与两行的度量之差，
/// 不依赖具体字体行框数值。度量走 measure-preview 标尺，与裁排同字重
fn preview_line_height(win: &QuickPasteWindow) -> f32 {
    let one = win.invoke_measure_preview("文本Ag".into(), 1000.0);
    let two = win.invoke_measure_preview("文本Ag\n文本Ag".into(), 1000.0);
    (two - one).max(1.0)
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
) -> String {
    clamp_preview_with(
        &mut |candidate, width| win.invoke_measure_preview(candidate.into(), width),
        text,
        avail_logical,
        source_truncated,
        line_height,
        PREVIEW_MAX_LINES,
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
    let chrome = 2.0 * geo.get_card_border() + geo.get_scroll_slot() + geo.get_preview_pad_h();
    let thumb_column = geo.get_thumb_size() + geo.get_thumb_gap();
    // window().size() 为物理像素；度量标尺与布局都以逻辑像素运作
    let scale = win.window().scale_factor();
    let panel_logical = win.window().size().width as f32 / scale;
    let no_thumb = (panel_logical - chrome).max(40.0);
    (no_thumb, (no_thumb - thumb_column).max(40.0))
}

fn build_rows(win: &QuickPasteWindow, items: &[ClipItemSummary]) {
    let digit_enabled = win.get_digit_shortcuts_enabled();
    let (avail_no_thumb, avail_with_thumb) = preview_avail_widths(win);
    let line_height = preview_line_height(win);

    let rows: Vec<ClipRow> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let thumb = thumbnails::cached(&item.id);
            let has_thumb = thumb.is_some();
            // 预览达到入库截断上限即认为原文更长，行末始终要有省略号
            let source_truncated = item.content_preview.chars().count() >= PREVIEW_SOURCE_LIMIT;
            let preview = clamp_preview(
                win,
                &item.content_preview,
                if has_thumb {
                    avail_with_thumb
                } else {
                    avail_no_thumb
                },
                source_truncated,
                line_height,
            );
            ClipRow {
                id: item.id.clone().into(),
                preview: preview.into(),
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
                        .take(2)
                        .map(|tag| SharedString::from(tag.clone()))
                        .collect();
                    ModelRc::new(Rc::new(VecModel::from(shown)))
                },
                tag_more: item.tags.len().saturating_sub(2) as i32,
            }
        })
        .collect();

    win.set_rows(ModelRc::new(Rc::new(VecModel::from(rows))));
    LIST_VERSION.with(|version| version.set(thumbnails::bump_list_version()));
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
        app.with_picker(|win| build_rows(&win, &items));
    });
}

/// 异步补齐缩略图：先渲染已有缓存，后台解码缺失项后回填行模型
/// （对齐原版 img loading=lazy 的非阻塞观感）
fn ensure_thumbnails(app: &App, items: &[ClipItemSummary]) {
    thumbnails::ensure(app, items, |app| {
        let items = app.state.items();
        app.with_picker(|win| build_rows(&win, &items));
    });
}

/* ───────────────── 会话动作 ───────────────── */

/// 确认上屏（对齐 PickerShell.confirmSelection）：as_path_text 走次级
/// 形态——图片上屏为图片路径、文件上屏为逐行路径列表（Shift+Enter）
pub fn confirm(app: &App, index: usize, as_path_text: bool) {
    let Some(item) = app.state.item_at(index) else {
        return;
    };

    tooltip::cancel(app);

    let settings = app.state.current_settings();
    let option = PasteOption {
        restore_clipboard_after_paste: settings.restore_clipboard_after_paste,
        paste_to_target: true,
        as_path_text,
    };

    if let Err(error) = paste_flow::paste_item(app, &item.id, option) {
        set_message(app, &format!("粘贴失败：{error}"), 2);
        warn!("粘贴失败: {error}");
    }
}

/// 粘贴结果回写（paste_flow 后台线程经 invoke 调用）。
/// 失败消息保留（面板此时多半已隐藏，下次会话开始会清空），
/// 成功消息置空以避免下次打开闪过旧消息（对齐前端 confirmSelection）。
pub fn set_outcome_message(app: &App, success: bool, message: &str) {
    if success {
        set_message(app, "", 0);
    } else {
        set_message(app, message, 2);
    }
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
        Ok(()) => {
            refresh_list(app, true);
            set_message(
                app,
                if next_favorited {
                    "已收藏"
                } else {
                    "已取消收藏"
                },
                1,
            );
        }
        Err(error) => {
            warn!("更新收藏状态失败: {error}");
            set_message(app, "更新收藏失败，请稍后重试", 2);
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
    let next = search::next_navigation_index(count, current, up);
    set_selected(app, next);
}

fn set_message(app: &App, text: &str, tone: i32) {
    app.with_picker(|win| {
        win.set_message_text(text.into());
        win.set_message_tone(tone);
    });
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
fn apply_window_position(
    app: &App,
    settings: &floatpaste_core::domain::settings::UserSetting,
    physical_size: slint::PhysicalSize,
    hwnd: isize,
    target_window_hwnd: Option<isize>,
) {
    let width = physical_size.width.max(1) as i32;
    let height = physical_size.height.max(1) as i32;
    let mode = settings.picker_position_mode.clone();

    // 解析失败（仓储报错 / 工作区不可得）回退到贴光标：直接放弃会把窗口
    // 留在屏外停屏位，用户按快捷键却什么都看不到
    let Some(point) = PickerPositionService::resolve_window_position(
        &app.core().repository,
        &mode,
        width,
        height,
        target_window_hwnd,
    )
    .ok()
    .flatten()
    .or_else(|| resolve_near_cursor(width, height)) else {
        warn!("速贴定位失败：光标位置与工作区均不可得，窗口保持原位");
        return;
    };

    // 尺寸与位置一次 SetWindowPos 落地：定位用的尺寸必须与真正生效的
    // 尺寸一致，分开调用还会露出「先长高后移位」的中间帧
    window_control::set_window_bounds(hwnd, point.x, point.y, width, height);
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

    // 单击：单击触发模式下立即上屏，否则仅选中
    {
        let app_cb = app.clone();
        win.on_row_clicked(move |index| {
            let index = index.max(0) as usize;
            set_selected(&app_cb, index);
            if app_cb.state.current_settings().paste_trigger == PasteTrigger::Click {
                confirm(&app_cb, index, false);
            }
        });
    }

    // 双击上屏
    {
        let app_cb = app.clone();
        win.on_row_double_clicked(move |index| {
            let index = index.max(0) as usize;
            set_selected(&app_cb, index);
            confirm(&app_cb, index, false);
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
