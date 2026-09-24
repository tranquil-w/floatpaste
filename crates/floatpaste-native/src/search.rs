//! 搜索窗口会话逻辑：对齐原版 SearchShell / useSearchSession / 搜索相关
//! WindowCoordinator 分支的完整行为。
//!
//! - 会话：打开时捕获回贴目标并定位到光标所在显示器工作区中央，真实获得
//!   键盘焦点（无全局钩子）；失焦且光标在窗外 → 边沿触发自动关闭（不还原
//!   目标，用户点击的位置已持有前台）。
//! - 查询：200ms 关键词防抖 + 类型/标签筛选 + 50 条分页 + 触底预载
//!   （240px），keyword 非空按相关度排序，否则按最近使用。
//! - 高度：target = min(620, 5 + chrome + 列表内容)，棘轮同步——结构变化
//!   （关键词/筛选/条目数）允许收缩，选中展开只允许增高。读数与应用统一
//!   由布局的 heights-changed 驱动，命令式调用点只置收缩许可不自读数。
//! - 挂起态（速贴会话把回贴目标定为搜索窗口）：输入框失焦、底栏切换为速
//!   贴键位；恢复路径覆盖速贴隐藏还原与粘贴还原两条链路。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, PhysicalPosition, PhysicalSize, VecModel};
use tracing::{info, warn};

use floatpaste_core::domain::clip_item::{
    ClipItemSummary, ClipType, PasteOption, SearchFilters, SearchQuery, SearchResult, SearchSort,
};
use floatpaste_core::domain::settings::{PasteTrigger, UserSetting};
use floatpaste_core::domain::error::AppError;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::picker_position::{
    current_cursor_point, work_area_from_point, ScreenRect,
};
use floatpaste_core::platform::windows::session_keyboard::parse_session_combo;
use floatpaste_core::platform::windows::window_control;
use floatpaste_core::services::clip_display::build_meta;
use floatpaste_core::services::clip_service::ClipService;
use floatpaste_core::services::paste_support;
use floatpaste_core::services::picker_position_service::center_in_work_area;
use floatpaste_core::services::time_format::format_relative_time_or_unused;

use crate::app_state::SearchSession;
use crate::overlay;
use crate::paste_flow;
use crate::picker::{self, App};
use crate::thumbnails;
use crate::tooltip::{self, HoverHost};
use crate::win32_ext;
use crate::{SearchGeometry, SearchRow, SearchTagChip, SearchWindow};

/// 单页条数（对齐旧版 SEARCH_PAGE_SIZE）
const PAGE_SIZE: u32 = 50;
/// 关键词防抖（对齐 SEARCH_INPUT_DEBOUNCE_MS）
const INPUT_DEBOUNCE_MS: u64 = 200;
/// 二次确认删除的保持时长（对齐 ARMED_DELETE_RESET_DELAY_MS）
const DELETE_ARM_TIMEOUT_MS: u64 = 3000;
/// 错误条自动消失时长（对齐 ERROR_TIMEOUT_MS）
const ERROR_TIMEOUT_MS: u64 = 3000;
/// 焦点监视轮询间隔（对齐 SEARCH_FOCUS_WATCH_INTERVAL_MS）
const FOCUS_WATCH_INTERVAL_MS: u64 = 120;
const RESTORE_DELAY: Duration = Duration::from_millis(90);
const INJECT_DELAY: Duration = Duration::from_millis(60);

/* ───────────────── 会话生命周期 ───────────────── */

/// 全局搜索快捷键命中：活跃则关闭并还原目标；速贴活跃则先收起速贴（不还
/// 原目标，焦点交给搜索窗口），再打开搜索（对齐 open_search_global）
pub fn toggle_from_shortcut(app: &App) {
    if app.state.is_search_active() {
        hide(app, true);
        return;
    }
    if app.state.is_picker_active() {
        picker::hide(app, false);
    }
    open(app);
}

/// 全局「打开搜索」语义（托盘菜单，对齐旧壳 open_search_global）：
/// 已活跃时仅聚回前台（保留关键词与列表状态，对齐 is_search_active 分支）；
/// 否则速贴活跃先收起（不还原目标，焦点交给搜索窗口）再走完整打开流程。
pub fn open_global(app: &App) {
    if app.state.is_search_active() {
        let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            if let Err(error) = window_control::restore_window_and_focus(hwnd) {
                warn!("搜索窗口获取焦点失败: {error}");
            }
        }
        return;
    }
    if app.state.is_picker_active() {
        picker::hide(app, false);
    }
    open(app);
}

/// 打开搜索会话（对齐 open_search_global）
pub fn open(app: &App) {
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd == 0 {
        warn!("搜索窗口 HWND 尚未就绪，无法显示");
        return;
    }

    // 先捕获回贴目标再显示（此刻前台还是用户正在用的窗口）
    let target = ActiveAppResolver::current_foreground_window_handle();
    app.state.set_search_session(SearchSession {
        target_window_hwnd: target,
    });
    app.state.begin_search_activation();
    info!("打开 Search，target_window={target:?}");

    let Some(win) = app.search.upgrade() else {
        app.state.end_search_activation();
        return;
    };

    // 窗口自启动起保持 Slint 可见（停屏态），重现 = 移回屏上，不走
    // Slint show：hide→show 周期中 winit 清空表面且脏区跟踪失效，是
    // 开窗透明闪烁的根源。若有未消费的编辑期停屏记录，在此作废
    if let Ok(mut slot) = PARKED_POSITION.lock() {
        *slot = None;
    }
    // 会话状态先重置（仍停屏），再改尺寸并同步泵帧：尺寸变更与泵帧同处
    // 一个消息回合，泵出的已是新尺寸的加载态表面，移回屏上首帧即完整
    // 内容。若泵帧先于尺寸变更，落盘/上屏的还是旧尺寸表面，与窗口不
    // 匹配即闪一下（对齐 picker::activate 的「尺寸/数据就绪 → 暖表面 →
    // 上屏」顺序）
    reset_session_state(app);
    // 尺寸先于定位：首开会话时窗口还是装配期自然尺寸（根布局钳制，不是
    // 设计尺寸），直接拿 window().size() 居中必偏（winit set_size 亦非
    // 同步可读回）。定位用同一组计算值，不经窗口回读
    let scale = win.window().scale_factor();
    let geo = win.global::<SearchGeometry>();
    let width_px = (geo.get_window_width() * scale).round() as i32;
    let last = LAST_HEIGHT.with(|value| value.get()) as i32;
    // 首开（无历史高度）直接用最大高度：内容加载后棘轮目标通常也是
    // max，避免开窗后再 resize 造成跳动/闪烁
    let height_px = if last > 0 {
        last
    } else {
        (geo.get_window_max_height() * scale).round() as i32
    };
    // 高度基线对齐到本次实际应用值：加载态的中间高度（低于基线）在无
    // 收缩许可时被跳过，内容落定只发生一次尺寸变化
    LAST_HEIGHT.with(|value| value.set(height_px as f32));
    JUST_OPENED.with(|flag| flag.set(true));
    // 停屏窗口在屏外，几何变化在移回屏上前完成，无可见中间帧；一次
    // SetWindowPos 同步定尺寸与位置（窗口仍在屏外，不受「先长高后移位」
    // 影响）。定位失败（光标/工作区不可得）才需要单独定尺寸，再退回
    // 上次隐藏前的原位（等价旧行为的「原位显示」）
    if !position_centered_on_cursor(hwnd, width_px, height_px) {
        win.window().set_size(PhysicalSize::new(
            width_px.max(1) as u32,
            height_px.max(1) as u32,
        ));
        let fallback = LAST_HIDDEN_POSITION.lock().ok().and_then(|slot| *slot);
        if let Some((x, y)) = fallback {
            win.window().set_position(PhysicalPosition::new(x, y));
        } else {
            warn!("搜索窗口定位失败且无历史位置，保持停屏");
        }
    }
    win32_ext::warm_surface(hwnd);

    if let Err(error) = window_control::restore_window_and_focus(hwnd) {
        warn!("搜索窗口获取焦点失败: {error}");
    }
    // 兜底重挂浮层样式（可聚焦变体，不带 WS_EX_NOACTIVATE）并复查置顶
    overlay::after_show_focusable(hwnd);

    begin_focus_watcher(app);
    refresh_tags(app);
    // 行模型已清空：首帧显示加载态（对齐旧版 keyword 重置后 isLoading）
    run_query(app);
}

/// 隐藏（对齐 hide_search_window / hide_search_and_restore_target）。
/// restore_target=true 立即把前台还给会话目标（粘贴/快捷键关闭路径）；
/// false 时不动前台（失焦自动关闭/Esc，用户点击处已持有前台）
pub fn hide(app: &App, restore_target: bool) {
    tooltip::cancel(app);
    app.state.end_search_activation();
    let session = app.state.search_session();
    if let Some(win) = app.search.upgrade() {
        // 记录原位后停屏到屏幕外而非 Slint hide（同 hide_for_editor）：
        // hide→show 周期会清空窗口表面，下次显示出现透明空壳
        let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            if let Some(rect) = win32_ext::physical_rect(hwnd) {
                if let Ok(mut slot) = LAST_HIDDEN_POSITION.lock() {
                    *slot = Some((rect.left, rect.top));
                }
            }
        }
        win.window()
            .set_position(slint::PhysicalPosition::new(-32000, -32000));
    }
    if restore_target {
        if let Some(target_hwnd) = session.target_window_hwnd {
            ActiveAppResolver::restore_foreground_window(target_hwnd);
        }
    }
    app.state.set_search_session(Default::default());
    info!("隐藏 Search，会话已清理");
}

/// 键盘交给速贴面板（对齐 SEARCH_INPUT_SUSPEND_EVENT）
pub fn suspend_input(app: &App) {
    app.with_search(|win| {
        win.set_input_suspended(true);
        win.invoke_blur_input();
    });
}

/// 进编辑器期间停屏的原位（物理坐标）：恢复时移回
static PARKED_POSITION: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

/// 常规隐藏（快捷键/Esc/失焦）停屏前的原位（物理坐标）：重新打开时
/// 光标定位失败则退回这里，等价旧行为的「原位显示」
static LAST_HIDDEN_POSITION: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

thread_local! {
    /// 打开后首次内容高度落定的一次性重居中标记：open 按默认/历史高度
    /// 居中，内容加载完成后棘轮变高，若不重居中会保持左上角锚定而偏下
    static JUST_OPENED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 进入编辑器前的隐藏：结束激活、收 tooltip、停屏窗口，但**不清理
/// 会话与列表状态**（对齐 hide_search_for_editor_transition）——
/// 编辑器关闭后原样恢复选中、关键词与滚动位置
pub fn hide_for_editor(app: &App) {
    tooltip::cancel(app);
    app.state.end_search_activation();
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if let Some(win) = app.search.upgrade() {
        // 记录原位后停屏到屏幕外（对齐 picker::hide_for_editor）：
        // Slint hide→show 周期会让 winit 抑制渲染，恢复出的窗口全透明；
        // 停屏保持 Slint "已显示"状态持续渲染，移回即原样恢复
        if hwnd != 0 {
            if let Some(rect) = win32_ext::physical_rect(hwnd) {
                if let Ok(mut slot) = PARKED_POSITION.lock() {
                    *slot = Some((rect.left, rect.top));
                }
            }
        }
        win.window()
            .set_position(slint::PhysicalPosition::new(-32000, -32000));
    }
    info!("隐藏 Search（进入编辑器）");
}

/// 从编辑器返回搜索：仅恢复窗口可见性与焦点，**不发会话开始**
/// （对齐 restore_search_after_editor）——保留进入编辑器时的选中、
/// 关键词与滚动位置，自然衔接原上下文
pub fn restore_after_editor(app: &App) {
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let Some(win) = app.search.upgrade() else {
        return;
    };
    // 上屏前先暖表面（同步泵一次 WM_PAINT 呈现），移回原位即有内容
    win32_ext::warm_surface(hwnd);
    if let Ok(mut slot) = PARKED_POSITION.lock() {
        if let Some((x, y)) = slot.take() {
            win.window()
                .set_position(slint::PhysicalPosition::new(x, y));
        }
    }
    // 窗口全程保持 Slint 可见（停屏态），移回原位即原样恢复，无需
    // Slint show；restore_window_and_focus 负责显化、置顶与聚焦
    if let Err(error) = window_control::restore_window_and_focus(hwnd) {
        warn!("搜索窗口恢复焦点失败: {error}");
    }
    overlay::after_show_focusable(hwnd);
    app.state.begin_search_activation();
    // 失焦自动关闭的监视随激活重启：旧壳是常驻 Focused(false) 监听按
    // is_search_active 门控，编辑器返回后依然生效；本实现的轮询线程
    // 生命周期与会话绑定，必须在此重新拉起（边沿触发不会误判首拍失焦）
    begin_focus_watcher(app);
    info!("从 Editor 返回 Search");
}

/// 键盘交还搜索窗口（对齐 SEARCH_INPUT_RESUME_EVENT）
pub fn resume_input(app: &App) {
    app.with_search(|win| {
        win.set_input_suspended(false);
        win.invoke_focus_input();
    });
}

/// 光标所在显示器工作区（物理坐标）；光标/工作区不可得返回 None
fn cursor_monitor_work_area() -> Option<ScreenRect> {
    let cursor = current_cursor_point().ok()?;
    work_area_from_point(cursor).ok()
}

/// 以新尺寸一次 SetWindowPos 完成缩放+移动并按光标所在工作区居中；
/// 工作区不可得返回 false，调用方退回自身兜底路径
fn position_centered_on_cursor(hwnd: isize, width_px: i32, height_px: i32) -> bool {
    let Some(work) = cursor_monitor_work_area() else {
        return false;
    };
    let (width, height) = (width_px.max(1), height_px.max(1));
    let point = center_in_work_area(work, width, height);
    window_control::set_window_bounds(hwnd, point.x, point.y, width, height);
    true
}

/// 会话开始重置（对齐 SEARCH_SESSION_START）：关键词/筛选/选中/滚动/错误
/// 全部归零，行模型清空进入加载态
fn reset_session_state(app: &App) {
    let Some(win) = app.search.upgrade() else {
        return;
    };
    QUERY.with(|state| *state.borrow_mut() = QueryState::default());
    CANCEL_DEBOUNCE.with(|slot| *slot.borrow_mut() = None);
    SELECTED_ID.with(|slot| *slot.borrow_mut() = None);
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = None);
    DELETE_TOKEN.with(|token| token.set(token.get() + 1));
    ERROR_TOKEN.with(|token| token.set(token.get() + 1));
    // LAST_HEIGHT 不清零：开窗直接用上次内容高度（open 会把基线对齐到
    // 实际应用的高度）。清零会让加载态的中间高度被视为「增长」而应用，
    // 窗口先缩后长、两次跳动（闪烁 + 最终偏下）
    ALLOW_SHRINK.with(|flag| flag.set(false));

    win.set_keyword("".into());
    win.set_input_suspended(false);
    win.set_selected(0);
    win.set_delete_armed(false);
    win.set_active_filter(0);
    win.set_error_text("".into());
    win.set_load_failed(false);
    win.set_fetching_next(false);
    win.set_has_next_page(false);
    win.set_show_result_count(false);
    win.set_result_count(0);
    win.set_result_total(0);
    win.set_rows(ModelRc::new(Rc::new(VecModel::from(
        Vec::<SearchRow>::new(),
    ))));
    win.set_loading(true);
    win.set_tag_chips(ModelRc::new(Rc::new(VecModel::from(
        Vec::<SearchTagChip>::new(),
    ))));
    win.set_scroll_generation(win.get_scroll_generation() + 1);
    win.invoke_focus_input();
}

/// 焦点监视（对齐旧版 searchFocusWatcher）：上一拍还在前台、这一拍丢失
/// 且光标不在窗口内 → 关闭且不还原目标。边沿触发：焦点从未拿到时不误关
fn begin_focus_watcher(app: &App) {
    let app_for_watch = app.clone();
    thread::spawn(move || {
        let mut was_foreground = false;
        loop {
            thread::sleep(Duration::from_millis(FOCUS_WATCH_INTERVAL_MS));
            if app_for_watch.core().is_quitting() || !app_for_watch.state.is_search_active() {
                return;
            }
            let hwnd = app_for_watch.state.search_hwnd.load(Ordering::SeqCst);
            if hwnd == 0 {
                return;
            }
            let is_foreground = ActiveAppResolver::current_foreground_hwnd() == Some(hwnd);
            if was_foreground && !is_foreground {
                let cursor_outside = window_control::is_cursor_inside_window(hwnd)
                    .map(|inside| !inside)
                    .unwrap_or(false);
                if cursor_outside {
                    let app = app_for_watch.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        hide(&app, false);
                    });
                    return;
                }
            }
            was_foreground = is_foreground;
        }
    });
}

/* ───────────────── 查询引擎 ───────────────── */

#[derive(Default)]
struct QueryState {
    /// 已生效（防抖后）进入查询的关键词
    keyword: String,
    /// 0=全部 1=收藏 2=文本 3=图片 4=文件
    filter: u32,
    /// 标签筛选（原始大小写，AND 语义）
    tags: Vec<String>,
    /// 已加载条数（分页游标）
    offset: u32,
    total: u32,
    has_more: bool,
    fetching: bool,
    fetching_next: bool,
    /// 失效在途响应：会话重置/新查询使旧回调丢弃
    seq: u64,
}

thread_local! {
    static QUERY: RefCell<QueryState> = RefCell::new(QueryState::default());
    /// 关键词防抖定时器（restartable：覆盖即停旧计时）
    static CANCEL_DEBOUNCE: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static SELECTED_ID: RefCell<Option<String>> = const { RefCell::new(None) };
    static ARMED_DELETE: RefCell<Option<String>> = const { RefCell::new(None) };
    static DELETE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static ERROR_TOKEN: Cell<u64> = const { Cell::new(0) };
    static LAST_HEIGHT: Cell<f32> = const { Cell::new(0.0) };
    static ALLOW_SHRINK: Cell<bool> = const { Cell::new(false) };
}

fn build_query(keyword: &str, filter: u32, tags: &[String], offset: u32) -> SearchQuery {
    let mut filters = SearchFilters::default();
    match filter {
        1 => filters.favorited_only = Some(true),
        2 => filters.clip_type = Some(ClipType::Text),
        3 => filters.clip_type = Some(ClipType::Image),
        4 => filters.clip_type = Some(ClipType::File),
        _ => {}
    }
    if !tags.is_empty() {
        filters.tag_names = Some(tags.to_vec());
    }
    SearchQuery {
        keyword: keyword.to_string(),
        filters,
        offset,
        limit: PAGE_SIZE,
        sort: if keyword.trim().is_empty() {
            SearchSort::RecentDesc
        } else {
            SearchSort::RelevanceDesc
        },
    }
}

/// 发起查询（offset 归零）。旧行保留显示直到新结果落地（对齐
/// keepPreviousData）；无旧行才显示加载态
fn run_query(app: &App) {
    let Some(win) = app.search.upgrade() else {
        return;
    };
    let keyword = win.get_keyword().to_string();
    let (filter, tags, seq) = QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.keyword = keyword.clone();
        state.offset = 0;
        state.fetching = true;
        state.fetching_next = false;
        state.seq += 1;
        (state.filter, state.tags.clone(), state.seq)
    });
    win.set_fetching_next(false);
    let keep_loading = win.get_rows().row_count() == 0;
    win.set_loading(keep_loading);

    let core = app.core().clone();
    let app_cb = app.clone();
    thread::spawn(move || {
        let query = build_query(&keyword, filter, &tags, 0);
        let result = core.repository.search(query);
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok(result) => apply_page(&app_cb, result, seq, false),
            Err(error) => {
                warn!("搜索查询失败: {error}");
                let message = error.to_string();
                let stale = QUERY.with(|state| {
                    let mut state = state.borrow_mut();
                    if state.seq != seq {
                        return true;
                    }
                    state.fetching = false;
                    false
                });
                if stale {
                    return;
                }
                app_cb.with_search(|win| {
                    win.set_fetching_next(false);
                    // 有旧行时保留旧数据静默失败（对齐 isError && items.length === 0）
                    if win.get_rows().row_count() == 0 {
                        win.set_loading(false);
                        win.set_load_failed(true);
                        win.set_load_error_text(message.into());
                    }
                });
            }
        });
    });
}

/// 查询落地：写入缓存/模型，按 id 锚点恢复选中（对齐 items 变更 effect）
fn apply_page(app: &App, result: SearchResult, seq: u64, append: bool) {
    let valid = QUERY.with(|state| {
        let mut state = state.borrow_mut();
        if state.seq != seq {
            return false;
        }
        state.fetching = false;
        state.fetching_next = false;
        state.total = result.total;
        true
    });
    if !valid {
        return;
    }

    let mut items = if append {
        app.state.search_items()
    } else {
        Vec::new()
    };
    items.extend(result.items.iter().cloned());
    app.state.set_search_items(items.clone());
    // 分页游标与是否还有下一页（对齐 getNextPageParam：offset + items.length < total）
    QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.offset = items.len() as u32;
        state.has_more = state.offset < result.total;
    });

    let previous_selected = SELECTED_ID.with(|slot| slot.borrow().clone());
    let next_selected = match &previous_selected {
        Some(id) if items.iter().any(|item| &item.id == id) => previous_selected,
        _ => items.first().map(|item| item.id.clone()),
    };
    SELECTED_ID.with(|slot| *slot.borrow_mut() = next_selected);

    let has_more = QUERY.with(|state| state.borrow().has_more);
    app.with_search(|win| {
        win.set_loading(false);
        win.set_load_failed(false);
        win.set_show_result_count(true);
        win.set_result_count(items.len() as i32);
        win.set_result_total(result.total as i32);
        win.set_has_next_page(has_more);
        win.set_fetching_next(false);
    });
    update_empty_state(app);
    build_rows(app);
    ensure_thumbnails(app, &items);
    sync_height();
}

/// 触底预载（对齐 fetchNextPage：offset + items.length < total）
pub fn fetch_next(app: &App) {
    let (has_more, fetching, fetching_next, keyword, filter, tags) = QUERY.with(|state| {
        let state = state.borrow();
        (
            state.has_more,
            state.fetching,
            state.fetching_next,
            state.keyword.clone(),
            state.filter,
            state.tags.clone(),
        )
    });
    if !has_more || fetching || fetching_next {
        return;
    }
    let offset = app.state.search_items().len() as u32;
    let seq = QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.fetching_next = true;
        state.seq += 1;
        state.seq
    });
    app.with_search(|win| win.set_fetching_next(true));

    let core = app.core().clone();
    let app_cb = app.clone();
    thread::spawn(move || {
        let query = build_query(&keyword, filter, &tags, offset);
        let result = core.repository.search(query);
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok(result) => apply_page(&app_cb, result, seq, true),
            Err(error) => {
                warn!("搜索翻页失败: {error}");
                QUERY.with(|state| {
                    let mut state = state.borrow_mut();
                    if state.seq == seq {
                        state.fetching_next = false;
                    }
                });
                app_cb.with_search(|win| win.set_fetching_next(false));
            }
        });
    });
}

/// 新剪贴内容入库：在途分页失效，从第一页重查（关键词/筛选保持，
/// 选中按 id 锚点恢复；对齐旧版 queryClient.invalidateQueries）
pub fn notify_clips_changed(app: &App) {
    if !app.state.is_search_active() {
        return;
    }
    run_query(app);
}

/// 编辑器内保存/删除/改标签后的刷新：搜索窗此时处于隐藏失活态，
/// 跳过活跃判定强制重查，保证返回时列表已反映变更
pub fn refresh_after_editor(app: &App) {
    run_query(app);
}

/// 关键词编辑：重启 200ms 防抖（输入框文本已双向绑定到窗口属性）
pub fn keyword_edited(app: &App) {
    let app_cb = app.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_millis(INPUT_DEBOUNCE_MS),
        move || run_query(&app_cb),
    );
    CANCEL_DEBOUNCE.with(|slot| *slot.borrow_mut() = Some(timer));
}

/// 清除关键词：立即清空并重查（对齐旧版同时置 keyword 与 debouncedKeyword）
pub fn clear_keyword(app: &App) {
    CANCEL_DEBOUNCE.with(|slot| *slot.borrow_mut() = None);
    QUERY.with(|state| state.borrow_mut().keyword.clear());
    app.with_search(|win| win.set_keyword("".into()));
    run_query(app);
}

pub fn filter_selected(app: &App, filter: i32) {
    QUERY.with(|state| state.borrow_mut().filter = filter.max(0) as u32);
    app.with_search(|win| win.set_active_filter(filter));
    run_query(app);
}

pub fn clear_filters(app: &App) {
    QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.filter = 0;
        state.tags.clear();
    });
    app.with_search(|win| {
        win.set_active_filter(0);
        rebuild_tag_chips(win);
    });
    run_query(app);
}

pub fn tag_toggled(app: &App, index: usize) {
    let Some(win) = app.search.upgrade() else {
        return;
    };
    let Some(chip) = win.get_tag_chips().iter().nth(index) else {
        return;
    };
    let name = chip.name.to_string();
    QUERY.with(|state| {
        let mut state = state.borrow_mut();
        match state
            .tags
            .iter()
            .position(|tag| tag.to_lowercase() == name.to_lowercase())
        {
            Some(position) => {
                state.tags.remove(position);
            }
            None => state.tags.push(name.clone()),
        }
    });
    rebuild_tag_chips(&win);
    run_query(app);
}

/// 标签 chips 重建：active 标记按当前筛选（大小写不敏感）恢复
fn rebuild_tag_chips(win: &SearchWindow) {
    let active: HashSet<String> = QUERY.with(|state| {
        state
            .borrow()
            .tags
            .iter()
            .map(|tag| tag.to_lowercase())
            .collect()
    });
    let chips: Vec<SearchTagChip> = win
        .get_tag_chips()
        .iter()
        .map(|chip| SearchTagChip {
            active: active.contains(&chip.name.to_string().to_lowercase()),
            ..chip
        })
        .collect();
    win.set_tag_chips(ModelRc::new(Rc::new(VecModel::from(chips))));
}

/// 会话打开时加载标签列表（对齐 tagsQuery）
fn refresh_tags(app: &App) {
    let Ok(tags) = app.core().repository.list_tags() else {
        return;
    };
    let active: HashSet<String> = QUERY.with(|state| {
        state
            .borrow()
            .tags
            .iter()
            .map(|tag| tag.to_lowercase())
            .collect()
    });
    let chips: Vec<SearchTagChip> = tags
        .into_iter()
        .map(|tag| SearchTagChip {
            active: active.contains(&tag.name.to_lowercase()),
            name: tag.name.into(),
        })
        .collect();
    app.with_search(|win| win.set_tag_chips(ModelRc::new(Rc::new(VecModel::from(chips)))));
}

/* ───────────────── 选中与行模型 ───────────────── */

thread_local! {
    /// 持久行模型：导航只 set_row_data 更新受影响的行，行元素不重建——
    /// changed is-selected 触发的滚动跟随依赖行持续存活
    static ROW_MODEL: RefCell<Option<Rc<VecModel<SearchRow>>>> = const { RefCell::new(None) };
}

pub fn set_selected(app: &App, index: usize) {
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    SELECTED_ID.with(|slot| *slot.borrow_mut() = Some(item.id.clone()));
    reset_delete_arm(app);
    // 行高恒定后选中切换不改行数据：行元素以 selected: i == root.selected
    // 绑定驱动高亮/左条/操作条，模型更新只留给结构性变化
    app.with_search(|win| win.set_selected(index as i32));
}

/// ↑↓ 循环导航（对齐 (i ± 1 + n) % n）
pub fn navigate(app: &App, up: bool) {
    let count = app.state.search_items().len();
    if count == 0 {
        return;
    }
    let current = app
        .with_search(|win| win.get_selected().max(0) as usize)
        .unwrap_or(0)
        .min(count - 1);
    set_selected(app, next_navigation_index(count, current, up));
}

/// 循环导航的下一索引（速贴 / 搜索共用，对齐 (i ± 1 + n) % n）
pub(crate) fn next_navigation_index(count: usize, current: usize, up: bool) -> usize {
    if up {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    }
}

/// 单行数据构建（行高恒定：选中态由 slint 侧 selected 绑定驱动，
/// 行数据不区分选中形态）
fn make_row(item: &ClipItemSummary) -> SearchRow {
    let thumb = thumbnails::cached(&item.id);
    let has_thumb = thumb.is_some();
    let normal_preview = flatten_preview_newlines(&item.content_preview);

    SearchRow {
        id: item.id.clone().into(),
        preview: normal_preview.into(),
        kind: match item.r#type.as_str() {
            "image" => 1,
            "file" => 2,
            _ => 0,
        },
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
        meta: build_meta(item).into(),
        favorited: item.is_favorited,
        has_thumb,
        thumb: thumb.unwrap_or_default(),
        tags: {
            let shown: Vec<slint::SharedString> = item
                .tags
                .iter()
                .take(3)
                .map(|tag| tag.clone().into())
                .collect();
            ModelRc::new(Rc::new(VecModel::from(shown)))
        },
        tag_more: item.tags.len().saturating_sub(3) as i32,
    }
}

/// 全量重建行模型（查询落地/删除/收藏视图移除/缩略图回填/宽度重裁）
fn build_rows(app: &App) {
    let Some(win) = app.search.upgrade() else {
        return;
    };
    let items = app.state.search_items();
    let selected_id = SELECTED_ID.with(|slot| slot.borrow().clone());
    let selected_index = items
        .iter()
        .position(|item| Some(&item.id) == selected_id.as_ref());

    let rows: Vec<SearchRow> = items.iter().map(make_row).collect();

    let model = Rc::new(VecModel::from(rows));
    ROW_MODEL.with(|slot| *slot.borrow_mut() = Some(model.clone()));
    win.set_selected(selected_index.unwrap_or(0) as i32);
    win.set_rows(ModelRc::from(model));
}

/// 异步补齐缩略图（与速贴共用缓存；解码回填后重建搜索行模型）
fn ensure_thumbnails(app: &App, items: &[ClipItemSummary]) {
    thumbnails::ensure(app, items, |app| build_rows(&app));
}

/// 非选中预览的换行折行（对齐旧版 preview.replace(/\r?\n/g, " ")）
fn flatten_preview_newlines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(current) = chars.next() {
        match current {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                result.push(' ');
            }
            '\n' => result.push(' '),
            _ => result.push(current),
        }
    }
    result
}

/* ───────────────── 元信息行 ───────────────── */
// 元信息与文件大小文案见 core::clip_display（build_meta / format_file_size）。

/* ───────────────── 空状态与错误 ───────────────── */

fn update_empty_state(app: &App) {
    let settings = app.state.current_settings();
    let open_hint =
        if settings.search_shortcut_enabled && !settings.search_shortcut.trim().is_empty() {
            format!("复制内容后使用 {} 打开此窗口", settings.search_shortcut)
        } else {
            "复制内容后即可在此查看".to_string()
        };
    let (keyword, filter, tags_len) = QUERY.with(|state| {
        let state = state.borrow();
        (state.keyword.clone(), state.filter, state.tags.len())
    });
    let (title, description, action) = empty_state(&keyword, filter, tags_len, &open_hint);
    app.with_search(|win| {
        win.set_empty_title(title.into());
        win.set_empty_description(description.into());
        win.set_empty_action(action);
    });
}

/// 空状态三分支（对齐 EMPTY_STATES）：有关键词 → 未找到匹配；有筛选 →
/// 筛选下暂无；否则 → 全空 + 打开方式提示
fn empty_state(
    keyword: &str,
    filter: u32,
    tags_len: usize,
    open_hint: &str,
) -> (String, String, i32) {
    if !keyword.trim().is_empty() {
        return (
            "未找到匹配记录".to_string(),
            "尝试调整搜索关键词".to_string(),
            1,
        );
    }
    if filter != 0 || tags_len > 0 {
        return (
            "当前筛选下暂无记录".to_string(),
            "尝试切换其他筛选或复制更多内容".to_string(),
            2,
        );
    }
    ("暂无剪贴板记录".to_string(), open_hint.to_string(), 0)
}

fn show_error(app: &App, message: &str) {
    app.with_search(|win| win.set_error_text(message.into()));
    let token = ERROR_TOKEN.with(|value| {
        value.set(value.get() + 1);
        value.get()
    });
    let app_cb = app.clone();
    slint::Timer::single_shot(Duration::from_millis(ERROR_TIMEOUT_MS), move || {
        if ERROR_TOKEN.with(|value| value.get()) != token {
            return;
        }
        app_cb.with_search(|win| win.set_error_text("".into()));
    });
}

/* ───────────────── 条目动作 ───────────────── */

pub fn paste_index(app: &App, index: usize, as_path_text: bool) {
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    paste_item(app, &item, as_path_text);
}

/// 选中条目上屏（键盘路径）；次级形态按类型生效：图片上屏为图片路径、
/// 文件上屏为逐行路径列表文本（文本暂无次级形态，等同主上屏）
pub fn paste_selected(app: &App, as_path_text: bool) {
    let index = app
        .with_search(|win| win.get_selected().max(0) as usize)
        .unwrap_or(0);
    paste_index(app, index, as_path_text);
}

fn paste_item(app: &App, item: &ClipItemSummary, as_path_text: bool) {
    tooltip::cancel(app);
    let settings = app.state.current_settings();
    let option = PasteOption {
        restore_clipboard_after_paste: settings.restore_clipboard_after_paste,
        paste_to_target: true,
        as_path_text,
    };
    if let Err(error) = execute_paste(app, &item.id, option) {
        warn!("搜索粘贴失败: {error}");
        show_error(app, "执行粘贴失败，请稍后重试");
    }
}

/// 上屏流（对齐 PasteExecutor 搜索分支）：趁搜索窗口可见写剪贴板 →
/// 隐藏并还原会话目标 → 90ms → 恢复前台 → 60ms → 注入 Ctrl+V →
/// 调度快照恢复 → mark_used。窗口已隐藏，结果消息无处展示（对齐旧版）
fn execute_paste(app: &App, id: &str, option: PasteOption) -> Result<(), AppError> {
    let detail = app.core().repository.get_item_detail(id)?;
    let session = app.state.search_session();

    // 管理员目标：照常写入剪贴板并还原目标焦点（用户可手动 Ctrl+V），
    // 仅跳过必然无效的按键注入，经托盘气泡一次性说明
    let admin_target = paste_flow::paste_target_requires_elevation(session.target_window_hwnd);
    if admin_target {
        paste_flow::notify_admin_target_once(app);
    }

    let previous_clipboard = paste_support::capture_snapshot_if_needed(&option)?;
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;

    let owner_hwnd = {
        let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
        (hwnd != 0).then_some(hwnd)
    };
    paste_support::write_item_to_clipboard(
        app.core(),
        &mut clipboard,
        &detail,
        option.as_path_text,
        owner_hwnd,
    )?;

    hide(app, true);

    let core = app.core().clone();
    let id = id.to_string();
    thread::spawn(move || {
        match session.target_window_hwnd {
            Some(target_hwnd) => {
                thread::sleep(RESTORE_DELAY);
                if core.is_quitting() {
                    return;
                }
                if ActiveAppResolver::restore_foreground_window_with_focus(target_hwnd, None) {
                    if !admin_target {
                        thread::sleep(INJECT_DELAY);
                        if !paste_support::trigger_ctrl_v() {
                            warn!("搜索回贴 Ctrl+V 注入失败");
                        }
                    }
                } else {
                    warn!("搜索回贴恢复目标窗口失败");
                }
            }
            None => warn!("搜索会话没有可恢复的目标窗口"),
        }
        if let Some(snapshot) = previous_clipboard {
            let _ = paste_support::schedule_clipboard_restore(&core, snapshot, owner_hwnd);
        }
        let _ = core.repository.mark_used(&id);
    });
    Ok(())
}

/// 收藏切换（对齐 toggleFavorite：乐观更新 + 收藏视图内取消收藏即时移除）
pub fn toggle_favorite(app: &App) {
    if app.state.favorite_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let selected_id = SELECTED_ID.with(|slot| slot.borrow().clone());
    let mut items = app.state.search_items();
    let result = items
        .iter()
        .find(|item| Some(item.id.as_str()) == selected_id.as_deref())
        .map(|item| {
            let next = !item.is_favorited;
            (next, ClipService::set_favorited(app.core(), &item.id, next))
        });
    app.state.favorite_pending.store(false, Ordering::SeqCst);

    let Some((next_favorited, result)) = result else {
        return;
    };
    match result {
        Err(error) => {
            warn!("更新收藏状态失败: {error}");
            show_error(app, "更新收藏状态失败，请稍后重试");
        }
        Ok(()) => {
            items
                .iter_mut()
                .filter(|item| Some(item.id.as_str()) == selected_id.as_deref())
                .for_each(|item| item.is_favorited = next_favorited);

            let favorite_view_removed =
                !next_favorited && QUERY.with(|state| state.borrow().filter == 1);
            if favorite_view_removed {
                // 收藏视图内取消收藏：从当前页移除、总数回补、选中重锚
                items.retain(|item| Some(item.id.as_str()) != selected_id.as_deref());
                let total = QUERY.with(|state| {
                    let mut state = state.borrow_mut();
                    state.total = state.total.saturating_sub(1);
                    state.total
                });
                app.state.set_search_items(items.clone());
                app.with_search(|win| {
                    win.set_result_count(items.len() as i32);
                    win.set_result_total(total as i32);
                });
                update_empty_state(app);
                build_rows(app);
                sync_height();
            } else {
                app.state.set_search_items(items);
                build_rows(app);
            }
        }
    }
}

/* ───────────────── 两段式删除 ───────────────── */

/// Del 或删除按钮：首次进入待确认态（3 秒后自动解除），再次触发执行
pub fn request_delete(app: &App) {
    let Some(id) = SELECTED_ID.with(|slot| slot.borrow().clone()) else {
        return;
    };
    let armed = ARMED_DELETE.with(|slot| slot.borrow().as_deref() == Some(id.as_str()));
    if armed {
        perform_delete(app, &id);
        return;
    }
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = Some(id.clone()));
    app.with_search(|win| win.set_delete_armed(true));
    let token = DELETE_TOKEN.with(|value| {
        value.set(value.get() + 1);
        value.get()
    });
    let app_cb = app.clone();
    slint::Timer::single_shot(Duration::from_millis(DELETE_ARM_TIMEOUT_MS), move || {
        if DELETE_TOKEN.with(|value| value.get()) != token {
            return;
        }
        ARMED_DELETE.with(|slot| *slot.borrow_mut() = None);
        app_cb.with_search(|win| win.set_delete_armed(false));
    });
}

fn reset_delete_arm(app: &App) {
    DELETE_TOKEN.with(|value| value.set(value.get() + 1));
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = None);
    app.with_search(|win| win.set_delete_armed(false));
}

fn perform_delete(app: &App, id: &str) {
    reset_delete_arm(app);
    if let Err(error) = ClipService::delete(app.core(), id) {
        warn!("删除条目失败: {error}");
        show_error(app, "删除条目失败，请稍后重试");
        return;
    }
    thumbnails::evict(id);
    // 即时从列表缓存移除（对齐 applyClipsChanged deleted）
    let mut items = app.state.search_items();
    let removed_was_selected = SELECTED_ID.with(|slot| {
        let current = slot.borrow().clone();
        let matches = current.as_deref() == Some(id);
        if matches {
            *slot.borrow_mut() = None;
        }
        matches
    });
    items.retain(|item| item.id != id);
    app.state.set_search_items(items.clone());
    let total = QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.total = state.total.saturating_sub(1);
        state.total
    });
    if removed_was_selected {
        SELECTED_ID.with(|slot| {
            *slot.borrow_mut() = items.first().map(|item| item.id.clone());
        });
    }
    app.with_search(|win| {
        win.set_result_count(items.len() as i32);
        win.set_result_total(total as i32);
    });
    update_empty_state(app);
    build_rows(app);
    sync_height();
}

/* ───────────────── 悬停预览（全类型条目）───────────────── */

pub fn search_is_active(app: &App) -> bool {
    app.state.is_search_active()
}

pub fn row_hover(app: &App, index: usize, mouse_x: f32, mouse_y: f32) {
    if !app.state.is_search_active() {
        return;
    }
    // 所有类型都出悬浮预览：文本/文件给全文，图片给大图（tooltip.rs 按类型分流）
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let host = HoverHost {
        hwnd,
        is_active: search_is_active,
    };
    tooltip::schedule_with(app, host, item, mouse_x, mouse_y);
}

pub fn hover_left(app: &App) {
    tooltip::cancel(app);
}

/* ───────────────── 高度棘轮同步 ───────────────── */

/// 请求高度同步（结构变化语义：查询落地/删除/取消收藏）：仅置一次
/// 收缩许可，读数与应用由 heights-changed 驱动（见 apply_height）。
/// 命令式调用点不得自行读数——list-content-height 依赖布局 pass 写入的
/// viewport-height，0ms 定时器读数可能先于渲染帧，拿到的是布局未跟上的
/// 陈旧值（曾致开窗会话按陈旧的空态高度重居中、消费 JUST_OPENED，真值
/// 落地后窗口永久偏下）
pub fn sync_height() {
    ALLOW_SHRINK.with(|flag| flag.set(true));
}

/// 列表内容实测高变化（list-stack 自然高由布局 pass 写入）：布局真值
/// 信号，触发一次读数应用。日常滚动等无高度净变化的触发在 apply_height
/// 内被 target==last 拦下
pub fn heights_changed(app: &App) {
    apply_height(app);
}

/// 读数并应用目标高度：target = min(620, 5 + chrome + 列表内容高)。
/// ALLOW_SHRINK 由结构性变化的 sync_height() 置位、应用后消费（棘轮）；
/// 开窗会话内容未落地（加载态）的中间高度不应用，等内容落地一次到位
fn apply_height(app: &App) {
    let Some(win) = app.search.upgrade() else {
        return;
    };
    let geo = win.global::<SearchGeometry>();
    let target_logical =
        (geo.get_height_slack() + win.get_chrome_height() + win.get_list_content_height())
            .min(geo.get_window_max_height());
    let scale = win.window().scale_factor();
    let target = (target_logical * scale).round() as i32;
    let last = LAST_HEIGHT.with(|value| value.get()) as i32;
    if target == last {
        return;
    }
    // 加载态高度不应用：矮历史开窗会在旧位置分步长高、露出「已长高
    // 未居中」的偏下中间帧，等内容落地一次到位
    if JUST_OPENED.with(|flag| flag.get()) && win.get_loading() {
        return;
    }
    let allow = ALLOW_SHRINK.with(|flag| flag.get());
    ALLOW_SHRINK.with(|flag| flag.set(false));
    if !allow && target < last {
        return;
    }
    LAST_HEIGHT.with(|value| value.set(target as f32));
    let width_px = (geo.get_window_width() * scale).round() as i32;
    // 打开后首次高度落定：以新尺寸重新居中（左上角锚定的增长会让
    // 窗口偏离打开时的居中位置）。一次 SetWindowPos 原子完成缩放+
    // 移动，避免「先长高后移位」的两帧呈现；工作区不可得则退回仅缩放
    if JUST_OPENED.with(|flag| flag.replace(false)) {
        let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
        if position_centered_on_cursor(hwnd, width_px, target) {
            return;
        }
    }
    win.window().set_size(PhysicalSize::new(
        width_px.max(1) as u32,
        target.max(1) as u32,
    ));
}

/* ───────────────── 窗口拖拽 ───────────────── */

pub fn drag_started(app: &App) {
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd != 0 {
        window_control::begin_window_gesture(
            hwnd,
            floatpaste_core::platform::windows::window_control::GestureMode::Move,
            0,
            0,
        );
    }
}

pub fn drag_moved(app: &App) {
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd != 0 {
        window_control::update_window_gesture(hwnd);
    }
}

pub fn drag_finished(app: &App) {
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd != 0 {
        window_control::end_window_gesture(hwnd);
    }
}

/// 加载失败重试：按当前关键词/筛选重查
pub fn retry(app: &App) {
    run_query(app);
}

/// Ctrl+Enter / 编辑按钮：打开编辑器（搜索窗停屏、会话让位，关闭后
/// 原样恢复选中与关键词；对齐旧版 SEARCH_EDIT_ITEM_EVENT）
pub fn edit_requested(app: &App) {
    let Some(id) = SELECTED_ID.with(|slot| slot.borrow().clone()) else {
        return;
    };
    crate::editor::open_from_search(app, id);
}

/* ───────────────── 回调装配 ───────────────── */

/// 搜索窗口回调绑定（main 装配期调用一次）
/// Slint 事件文本 → 规范键名（与设置页录制端同名）。特殊键取 Slint 的
/// 控制字符/私有区码位（i-slint-common key_codes 表）；非单字符文本
/// （IME 组合串等）不参与会话匹配
fn event_key_name(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match ch {
        '\u{0008}' => "Backspace".to_string(),
        '\u{0009}' => "Tab".to_string(),
        '\u{000a}' => "Enter".to_string(),
        '\u{001b}' => "Escape".to_string(),
        '\u{007f}' => "Delete".to_string(),
        '\u{0020}' => "Space".to_string(),
        '\u{F700}' => "Up".to_string(),
        '\u{F701}' => "Down".to_string(),
        '\u{F702}' => "Left".to_string(),
        '\u{F703}' => "Right".to_string(),
        '\u{F727}' => "Insert".to_string(),
        '\u{F729}' => "Home".to_string(),
        '\u{F72B}' => "End".to_string(),
        '\u{F72C}' => "PageUp".to_string(),
        '\u{F72D}' => "PageDown".to_string(),
        ',' => "Comma".to_string(),
        '.' => "Period".to_string(),
        '/' => "Slash".to_string(),
        '`' => "`".to_string(),
        c if c.is_ascii_alphanumeric() => c.to_ascii_uppercase().to_string(),
        _ => return None,
    })
}

/// 会话键解析（窗口 capture 阶段调用）：按当前设置的键位精确匹配
/// （键名忽略大小写、修饰键集合一致），返回动作码：
/// 0=无 1=向上 2=向下 3=上屏 4=次级上屏（粘贴为路径） 5=编辑 6=收藏 7=关闭 8=删除
fn resolve_session_action(
    settings: &UserSetting,
    text: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
) -> i32 {
    let Some(name) = event_key_name(text) else {
        return 0;
    };
    let keys = &settings.session_keys;
    let matches = |combo_text: &str| {
        parse_session_combo(combo_text).is_some_and(|combo| {
            combo.key.eq_ignore_ascii_case(&name)
                && combo.ctrl == ctrl
                && combo.alt == alt
                && combo.shift == shift
                && combo.win == win
        })
    };
    if matches(&keys.navigate_up) {
        1
    } else if matches(&keys.navigate_down) {
        2
    } else if matches(&keys.confirm) {
        3
    } else if matches(&keys.confirm_as_file) {
        4
    } else if matches(&keys.open_editor) {
        5
    } else if matches(&keys.toggle_favorite) {
        6
    } else if matches(&keys.dismiss) {
        7
    } else if matches(&keys.delete_entry) {
        8
    } else {
        0
    }
}

pub fn wire(app: &App) {
    let Some(win) = app.search.upgrade() else {
        return;
    };

    // 搜索框
    {
        let app_cb = app.clone();
        win.on_keyword_edited(move |_text| {
            keyword_edited(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_clear_keyword(move || {
            clear_keyword(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_filter_selected(move |filter| {
            filter_selected(&app_cb, filter);
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_toggled(move |index| {
            tag_toggled(&app_cb, index.max(0) as usize);
        });
    }
    {
        let app_cb = app.clone();
        win.on_clear_filters(move || {
            clear_filters(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_retry_clicked(move || {
            retry(&app_cb);
        });
    }

    // 行交互
    {
        let app_cb = app.clone();
        win.on_row_clicked(move |index| {
            let index = index.max(0) as usize;
            set_selected(&app_cb, index);
            // 单击触发模式：点击即上屏（双击模式的两次 click 也会到达，
            // 但首次 click 已结束会话，不会双重上屏）。挂起态（键盘已交给
            // 速贴面板）跳过，避免破坏进行中的速贴会话
            let suspended = app_cb
                .with_search(|win| win.get_input_suspended())
                .unwrap_or(false);
            if !suspended && app_cb.state.current_settings().paste_trigger == PasteTrigger::Click {
                paste_index(&app_cb, index, false);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_double_clicked(move |index| {
            paste_index(&app_cb, index.max(0) as usize, false);
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_hover(move |index, x, y| {
            row_hover(&app_cb, index.max(0) as usize, x, y);
        });
    }
    {
        let app_cb = app.clone();
        win.on_row_hover_left(move || {
            hover_left(&app_cb);
        });
    }
    // 图标按钮静态提示（悬浮气泡 simple 模式，锚点=按钮下方）
    {
        let app_cb = app.clone();
        win.on_button_hint(move |text, x, y| {
            let hwnd = app_cb.state.search_hwnd.load(Ordering::SeqCst);
            if hwnd == 0 {
                return;
            }
            let host = HoverHost {
                hwnd,
                is_active: search_is_active,
            };
            tooltip::schedule_static(&app_cb, host, text.to_string(), x, y);
        });
    }
    {
        let app_cb = app.clone();
        win.on_button_hint_left(move || {
            hover_left(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_paste(move |index| {
            paste_index(&app_cb, index.max(0) as usize, false);
        });
    }
    // 次级上屏按钮（图片/文件行）只在选中行上出现，作用于当前选中
    {
        let app_cb = app.clone();
        win.on_action_paste_as_path(move || {
            paste_selected(&app_cb, true);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_edit(move || {
            edit_requested(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_toggle_favorite(move || {
            toggle_favorite(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_delete(move || {
            request_delete(&app_cb);
        });
    }

    // 键盘会话（capture 阶段拦截）：键位判定统一在 Rust 侧按当前设置
    // 解析（自定义会话键），窗口只按返回的动作码分发
    {
        let app_cb = app.clone();
        win.on_resolve_session_action(move |text, ctrl, alt, shift, meta| {
            let settings = app_cb.state.current_settings();
            resolve_session_action(&settings, &text, ctrl, alt, shift, meta)
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_navigate_up(move || {
            navigate(&app_cb, true);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_navigate_down(move || {
            navigate(&app_cb, false);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_paste(move |as_path_text| {
            paste_selected(&app_cb, as_path_text);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_edit(move || {
            edit_requested(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_toggle_favorite(move || {
            toggle_favorite(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_delete(move || {
            request_delete(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_close(move || {
            hide(&app_cb, false);
        });
    }

    // 触底预载 / 高度棘轮 / 头部拖拽
    {
        let app_cb = app.clone();
        win.on_near_bottom(move || {
            fetch_next(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_heights_changed(move || {
            heights_changed(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_drag_started(move || {
            drag_started(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_drag_moved(move || {
            drag_moved(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_drag_finished(move || {
            drag_finished(&app_cb);
        });
    }
}

/* ───────────────── 单元测试 ───────────────── */

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_of(kind: &str) -> ClipItemSummary {
        ClipItemSummary {
            id: "id".into(),
            r#type: kind.into(),
            content_preview: String::new(),
            source_app: Some("浏览器".into()),
            is_favorited: false,
            file_count: 0,
            directory_count: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            last_used_at: None,
            image_path: None,
            image_width: None,
            image_height: None,
            image_format: None,
            file_size: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn navigation_wraps_in_both_directions() {
        assert_eq!(next_navigation_index(5, 0, true), 4);
        assert_eq!(next_navigation_index(5, 4, true), 3);
        assert_eq!(next_navigation_index(5, 4, false), 0);
        assert_eq!(next_navigation_index(1, 0, true), 0);
    }

    #[test]
    fn file_meta_lists_files_and_directories() {
        let mut item = summary_of("file");
        item.file_count = 5;
        item.directory_count = 2;
        let meta = build_meta(&item);
        assert!(meta.contains("5 个文件"));
        assert!(meta.contains("2 个文件夹"));
    }

    #[test]
    fn query_maps_filter_and_sort() {
        let query = build_query("  关键词 ", 3, &["工作".to_string()], 50);
        assert_eq!(query.keyword, "  关键词 ");
        assert_eq!(query.filters.clip_type, Some(ClipType::Image));
        assert_eq!(
            query.filters.tag_names.as_deref(),
            Some(["工作".to_string()].as_slice())
        );
        assert!(matches!(query.sort, SearchSort::RelevanceDesc));
        assert_eq!(query.offset, 50);
        assert_eq!(query.limit, PAGE_SIZE);

        let query = build_query("", 1, &[], 0);
        assert!(matches!(query.sort, SearchSort::RecentDesc));
        assert_eq!(query.filters.favorited_only, Some(true));
    }

    #[test]
    fn empty_state_picks_branch_by_keyword_then_filters() {
        assert_eq!(
            empty_state("  abc ", 0, 0, "提示"),
            ("未找到匹配记录".into(), "尝试调整搜索关键词".into(), 1)
        );
        assert_eq!(
            empty_state("", 2, 0, "提示"),
            (
                "当前筛选下暂无记录".into(),
                "尝试切换其他筛选或复制更多内容".into(),
                2
            )
        );
        assert_eq!(
            empty_state("", 0, 1, "提示"),
            (
                "当前筛选下暂无记录".into(),
                "尝试切换其他筛选或复制更多内容".into(),
                2
            )
        );
        assert_eq!(
            empty_state("", 0, 0, "提示"),
            ("暂无剪贴板记录".into(), "提示".into(), 0)
        );
    }

    #[test]
    fn preview_newlines_flatten_to_spaces() {
        assert_eq!(flatten_preview_newlines("a\r\nb\nc"), "a b c");
        assert_eq!(flatten_preview_newlines("无换行"), "无换行");
    }

    #[test]
    fn session_action_resolves_default_combinations() {
        let settings = UserSetting::default();
        // Slint 事件文本：Return=\u{a}、Escape=\u{1b}、UpArrow=\u{F700}
        assert_eq!(resolve_session_action(&settings, "\u{a}", false, false, false, false), 3);
        assert_eq!(resolve_session_action(&settings, "\u{a}", false, false, true, false), 4);
        assert_eq!(resolve_session_action(&settings, "\u{a}", true, false, false, false), 5);
        assert_eq!(resolve_session_action(&settings, "\u{1b}", false, false, false, false), 7);
        assert_eq!(resolve_session_action(&settings, "\u{F700}", false, false, false, false), 1);
        assert_eq!(resolve_session_action(&settings, "\u{F701}", false, false, false, false), 2);
        assert_eq!(resolve_session_action(&settings, "\u{20}", true, false, false, false), 6);
        assert_eq!(resolve_session_action(&settings, "\u{7f}", false, false, false, false), 8);
        // 未配置组合放行（0），修饰键集合不一致不命中
        assert_eq!(resolve_session_action(&settings, "\u{20}", false, false, false, false), 0);
        assert_eq!(resolve_session_action(&settings, "x", false, false, false, false), 0);
    }

    #[test]
    fn session_action_follows_customized_keys_case_insensitively() {
        let settings = UserSetting {
            session_keys: floatpaste_core::domain::settings::SessionKeys {
                confirm: "Ctrl+K".to_string(),
                dismiss: "Q".to_string(),
                ..floatpaste_core::domain::settings::SessionKeys::default()
            },
            ..UserSetting::default()
        };
        assert_eq!(resolve_session_action(&settings, "k", true, false, false, false), 3);
        assert_eq!(resolve_session_action(&settings, "q", false, false, false, false), 7);
        // 旧默认键位不再命中
        assert_eq!(resolve_session_action(&settings, "\u{1b}", false, false, false, false), 0);
    }

    #[test]
    fn event_key_name_rejects_multi_char_text() {
        assert_eq!(event_key_name("ab"), None);
        assert_eq!(event_key_name("剪"), None);
        assert_eq!(event_key_name("\u{a}"), Some("Enter".to_string()));
    }
}
