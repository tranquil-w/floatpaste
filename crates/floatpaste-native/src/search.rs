//! 搜索窗口会话逻辑：对齐原版 SearchShell / useSearchSession / 搜索相关
//! WindowCoordinator 分支的完整行为。
//!
//! - 会话：打开时捕获回贴目标并定位到光标所在显示器工作区中央，真实获得
//!   键盘焦点（无全局钩子）；失焦且光标在窗外 → 边沿触发自动关闭（不还原
//!   目标，用户点击的位置已持有前台）。
//! - 查询：200ms 关键词防抖 + 类型/标签筛选 + 50 条分页 + 触底预载
//!   （240px），keyword 非空按相关度排序，否则按最近使用。
//! - 高度：target = min(620, 5 + chrome + 列表内容)，棘轮同步——结构变化
//!   （关键词/筛选/错误条/条目数）允许收缩，选中展开只允许增高。
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
use floatpaste_core::domain::error::AppError;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::picker_position::{
    current_cursor_point, work_area_from_point,
};
use floatpaste_core::platform::windows::window_control;
use floatpaste_core::services::clip_service::ClipService;
use floatpaste_core::services::paste_support;
use floatpaste_core::services::time_format::format_relative_time_or_unused;

use crate::app_state::SearchSession;
use crate::overlay;
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
/// 选中预览最多行数（对齐 line-clamp-3）
const PREVIEW_MAX_LINES: usize = 3;
/// 入库预览的字符截断上限（normalize_service 同值）：达到即认为原文更长
const PREVIEW_SOURCE_LIMIT: usize = 120;
/// 大图预览最高（对齐 max-h-[120px]）
const LARGE_PREVIEW_MAX_H: f32 = 120.0;

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
    // 会话状态先重置（仍停屏），同步泵帧让空关键词加载态落盘，再移回
    // 屏上：首帧即加载态，不闪旧内容
    reset_session_state(app);
    win32_ext::warm_surface(hwnd);
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
    win.window().set_size(PhysicalSize::new(
        width_px.max(1) as u32,
        height_px.max(1) as u32,
    ));
    JUST_OPENED.with(|flag| flag.set(true));
    if !position_on_cursor_monitor(&win, width_px, height_px) {
        // 光标/工作区不可得：退回上次隐藏前的位置（等价旧行为的
        // 「原位显示」）；无记录时保持停屏并告警
        let fallback = LAST_HIDDEN_POSITION.lock().ok().and_then(|slot| *slot);
        if let Some((x, y)) = fallback {
            win.window().set_position(PhysicalPosition::new(x, y));
        } else {
            warn!("搜索窗口定位失败且无历史位置，保持停屏");
        }
    }

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

/// 打开时定位：光标所在显示器工作区居中（不持久化位置，对齐
/// center_window_on_cursor_monitor）。尺寸由调用方算好后传入——
/// winit set_size 不同步反映到 window().size()，回读会拿到旧值。
/// 定位失败返回 false，由调用方兜底
fn position_on_cursor_monitor(win: &SearchWindow, width_px: i32, height_px: i32) -> bool {
    let Ok(cursor) = current_cursor_point() else {
        return false;
    };
    let Ok(work) = work_area_from_point(cursor) else {
        return false;
    };
    let x = work.left + (work.width() - width_px).max(0) / 2;
    let y = work.top + (work.height() - height_px).max(0) / 2;
    win.window().set_position(PhysicalPosition::new(x, y));
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
    SELECTED_DETAIL.with(|slot| *slot.borrow_mut() = None);
    SELECTED_FULL_IMAGE.with(|slot| *slot.borrow_mut() = None);
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = None);
    DELETE_TOKEN.with(|token| token.set(token.get() + 1));
    ERROR_TOKEN.with(|token| token.set(token.get() + 1));
    LAST_HEIGHT.with(|value| value.set(0.0));
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
    /// 选中条目详情全文（id, full_text）；仅文本条目预览需要
    static SELECTED_DETAIL: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// 选中图片条目的全图解码结果（id, image）；None 表示解码中/未就绪
    static SELECTED_FULL_IMAGE: RefCell<Option<(String, Option<slint::Image>)>> =
        const { RefCell::new(None) };
    static ARMED_DELETE: RefCell<Option<String>> = const { RefCell::new(None) };
    static DELETE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static ERROR_TOKEN: Cell<u64> = const { Cell::new(0) };
    static LAST_HEIGHT: Cell<f32> = const { Cell::new(0.0) };
    static ALLOW_SHRINK: Cell<bool> = const { Cell::new(false) };
    static HEIGHT_TOKEN: Cell<u64> = const { Cell::new(0) };
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
    refresh_selected_detail(app);

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
    sync_height(app, true);
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
    let previous = SELECTED_ID.with(|slot| slot.borrow().clone());
    let previous_index = app
        .with_search(|win| win.get_selected().max(0) as usize)
        .unwrap_or(0);
    SELECTED_ID.with(|slot| *slot.borrow_mut() = Some(item.id.clone()));
    reset_delete_arm(app);
    refresh_selected_detail(app);

    // 只更新受影响的两行（旧选中行恢复非选中形态、新行展开选中形态），
    // 模型整体重建只留给查询落地/删除/收藏等结构性变化
    let updated = app.with_search(|win| -> bool {
        let model = ROW_MODEL.with(|slot| slot.borrow().clone());
        let Some(model) = model else {
            return false;
        };
        if model.row_count() != app.state.search_items().len() {
            return false;
        }
        let ctx = row_ctx(win);
        let previous_still_listed = previous.as_ref().is_some_and(|id| {
            app.state
                .search_item_at(previous_index)
                .is_some_and(|row| row.id == *id)
        });
        if previous_still_listed && previous.as_deref() != Some(item.id.as_str()) {
            if let Some(previous_item) = app.state.search_item_at(previous_index) {
                model.set_row_data(previous_index, make_row(win, &ctx, &previous_item, false));
            }
        }
        model.set_row_data(index, make_row(win, &ctx, &item, true));
        win.set_selected(index as i32);
        true
    });
    if !updated.unwrap_or(false) {
        build_rows(app);
    }
    // 选中展开只允许增高（对齐 allowWindowShrinkRef 不置位的棘轮语义）
    sync_height(app, false);
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

fn next_navigation_index(count: usize, current: usize, up: bool) -> usize {
    if up {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    }
}

/// 选中条目详情与全图：文本取全文（同步单行读），图片调度全图解码
fn refresh_selected_detail(app: &App) {
    let selected_id = SELECTED_ID.with(|slot| slot.borrow().clone());
    let Some(id) = selected_id else {
        SELECTED_DETAIL.with(|slot| *slot.borrow_mut() = None);
        SELECTED_FULL_IMAGE.with(|slot| *slot.borrow_mut() = None);
        return;
    };
    let Some(item) = app
        .state
        .search_items()
        .into_iter()
        .find(|item| item.id == id)
    else {
        return;
    };

    if item.r#type == "text" {
        // 旧版选中预览对文本条目替换为详情全文（detailQuery.data.fullText）
        let full = app
            .core()
            .repository
            .get_item_detail(&id)
            .ok()
            .map(|detail| (detail.id, detail.full_text));
        SELECTED_DETAIL.with(|slot| *slot.borrow_mut() = full);
    } else {
        SELECTED_DETAIL.with(|slot| *slot.borrow_mut() = None);
    }
    ensure_selected_full_image(app, &item);
}

/// 全图异步解码回填（缩略图 36px 拉伸会糊化，选中后换全图）。
/// 高度在行模型里按元数据先占位，解码完成只补选中行的像素
fn ensure_selected_full_image(app: &App, item: &ClipItemSummary) {
    if item.r#type != "image" {
        SELECTED_FULL_IMAGE.with(|slot| *slot.borrow_mut() = None);
        return;
    }
    let already = SELECTED_FULL_IMAGE
        .with(|slot| matches!(&*slot.borrow(), Some((id, Some(_))) if id == &item.id));
    if already {
        return;
    }
    let Some(path) = item.image_path.clone() else {
        SELECTED_FULL_IMAGE.with(|slot| *slot.borrow_mut() = None);
        return;
    };

    let id = item.id.clone();
    SELECTED_FULL_IMAGE.with(|slot| *slot.borrow_mut() = Some((id.clone(), None)));
    let core = app.core().clone();
    let app_cb = app.clone();
    thread::spawn(move || {
        let raw = thumbnails::load_full_image_raw(&core, &path);
        let _ = slint::invoke_from_event_loop(move || {
            let still_selected =
                SELECTED_ID.with(|slot| slot.borrow().as_deref() == Some(id.as_str()));
            if !still_selected {
                return;
            }
            SELECTED_FULL_IMAGE
                .with(|slot| *slot.borrow_mut() = Some((id, raw.map(thumbnails::image_from_rgba))));
            update_selected_row(&app_cb);
        });
    });
}

/// 只重算并写回当前选中行（全图解码回填等单行更新）
fn update_selected_row(app: &App) {
    app.with_search(|win| {
        let model = ROW_MODEL.with(|slot| slot.borrow().clone());
        let Some(model) = model else {
            return;
        };
        let index = win.get_selected().max(0) as usize;
        let Some(item) = app.state.search_item_at(index) else {
            return;
        };
        let selected_id = SELECTED_ID.with(|slot| slot.borrow().clone());
        if selected_id.as_deref() != Some(item.id.as_str()) {
            return;
        }
        let ctx = row_ctx(win);
        model.set_row_data(index, make_row(win, &ctx, &item, true));
    });
}

/// 行构建上下文：一次测量，多行复用
struct RowCtx {
    base: f32,
    reserve: f32,
}

fn row_ctx(win: &SearchWindow) -> RowCtx {
    let base = preview_base_widths(win);
    let reserve = win
        .global::<SearchGeometry>()
        .get_selected_preview_reserve();
    RowCtx { base, reserve }
}

/// 行基础预览宽（逻辑像素，图标列恒占位已扣除）：按窗口几何常量推导。
/// 不读 slint 上报值——首帧行构建可能早于窗口到位，读到陈旧小值会把
/// 选中预览硬折成窄条（与 slint 侧 report-preview-widths 同式）
fn preview_base_widths(win: &SearchWindow) -> f32 {
    let geo = win.global::<SearchGeometry>();
    let scale = win.window().scale_factor();
    let panel_logical = win.window().size().width as f32 / scale;
    let base = panel_logical
        - geo.get_scroll_slot()
        - 2.0 * geo.get_content_pad_h()
        - geo.get_row_border_l()
        - 2.0 * geo.get_row_pad_h()
        - geo.get_thumb_size()
        - geo.get_thumb_gap();
    base.max(40.0)
}

/// 单行数据构建。selected=true 时携带选中形态：文本条目取详情全文按
/// break-words 语义折到 3 行预算内，图片条目携带大图几何
fn make_row(win: &SearchWindow, ctx: &RowCtx, item: &ClipItemSummary, selected: bool) -> SearchRow {
    let selected_id = SELECTED_ID.with(|slot| slot.borrow().clone());
    let is_selected_row = selected && selected_id.as_deref() == Some(item.id.as_str());
    let thumb = thumbnails::cached(&item.id);
    let has_thumb = thumb.is_some();
    let base_width = ctx.base;
    let normal_preview = flatten_preview_newlines(&item.content_preview);

    // 选中形态预览：文本条目换详情全文（对齐 detailQuery.data.fullText），
    // 其余类型沿用入库预览（旧版 selectedPreviewText 分支同构）
    let selected_preview = if is_selected_row {
        let source_truncated = item.content_preview.chars().count() >= PREVIEW_SOURCE_LIMIT;
        let full = SELECTED_DETAIL
            .with(|slot| {
                slot.borrow()
                    .as_ref()
                    .filter(|(id, _)| id == &item.id)
                    .map(|(_, text)| text.clone())
            })
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| item.content_preview.clone());
        if item.r#type == "text" {
            let width = (base_width - ctx.reserve).max(40.0);
            hard_wrap_preview(
                &mut |candidate| win.invoke_measure_natural_width(candidate.into()),
                &full,
                width,
                source_truncated,
                PREVIEW_MAX_LINES,
            )
        } else {
            normal_preview.clone()
        }
    } else {
        normal_preview.clone()
    };

    // 大图预览仅选中行携带（内存考量）：高度按列宽等比、上限 120
    let (has_large, large_height) = if is_selected_row && item.r#type == "image" {
        large_preview_geometry(item, base_width)
            .map(|height| (true, height))
            .unwrap_or((false, 0.0))
    } else {
        (false, 0.0)
    };
    let large_image = if has_large {
        SELECTED_FULL_IMAGE
            .with(|slot| {
                slot.borrow()
                    .as_ref()
                    .filter(|(id, _)| id == &item.id)
                    .and_then(|(_, image)| image.clone())
            })
            .unwrap_or_default()
    } else {
        slint::Image::default()
    };

    SearchRow {
        id: item.id.clone().into(),
        preview: normal_preview.clone().into(),
        selected_preview: selected_preview.into(),
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
        has_large_preview: has_large,
        large_preview: large_image,
        large_preview_height: large_height,
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

    let ctx = row_ctx(&win);
    let rows: Vec<SearchRow> = items
        .iter()
        .map(|item| {
            let selected = selected_id.as_deref() == Some(item.id.as_str());
            make_row(&win, &ctx, item, selected)
        })
        .collect();

    let model = Rc::new(VecModel::from(rows));
    ROW_MODEL.with(|slot| *slot.borrow_mut() = Some(model.clone()));
    win.set_selected(selected_index.unwrap_or(0) as i32);
    win.set_rows(ModelRc::from(model));
}

/// 异步补齐缩略图（与速贴共用缓存；解码回填后重建搜索行模型）
fn ensure_thumbnails(app: &App, items: &[ClipItemSummary]) {
    let missing: Vec<(String, String)> = items
        .iter()
        .filter(|item| item.r#type == "image" && item.image_path.is_some())
        .filter(|item| !thumbnails::contains(&item.id))
        .filter_map(|item| {
            let path = item.image_path.clone()?;
            Some((item.id.clone(), path))
        })
        .collect();
    if missing.is_empty() {
        return;
    }

    let version = thumbnails::list_version();
    let core = app.core().clone();
    let app_cb = app.clone();
    thread::spawn(move || {
        // 跨线程只传原始像素；slint::Image 非 Send，事件循环侧再构造
        let mut decoded: Vec<(String, Option<thumbnails::RawImage>)> = Vec::new();
        for (id, path) in missing {
            let raw = thumbnails::load_thumbnail_raw(&core, &path);
            decoded.push((id, raw));
        }
        let _ = slint::invoke_from_event_loop(move || {
            if thumbnails::list_version() != version {
                return; // 列表已刷新，等待下一轮 ensure_thumbnails
            }
            for (id, raw) in decoded {
                thumbnails::insert(id, raw.map(thumbnails::image_from_rgba));
            }
            build_rows(&app_cb);
        });
    });
}

/// 按宽度把文本硬折到 max_lines 行内（旧版 whitespace-pre-wrap +
/// break-words + line-clamp 的合成语义）：
/// - 显式换行保留为行界（pre-wrap）
/// - 行内放不下时优先在词间空格断行（词整体移到下一行），长词无空格
///   可依才按字符边界硬折（break-words 的兜底）
/// - 超出预算行或源本身截断 → 末行以省略号收尾（line-clamp 观感）
fn hard_wrap_preview(
    measure_width: &mut dyn FnMut(&str) -> f32,
    text: &str,
    avail: f32,
    source_truncated: bool,
    max_lines: usize,
) -> String {
    if text.is_empty() || avail <= 0.0 {
        return text.to_string();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut overflow = false;
    for segment in text.split('\n') {
        let mut rest = segment;
        while !rest.is_empty() {
            if lines.len() == max_lines {
                overflow = true;
                break;
            }
            if measure_width(rest) <= avail {
                lines.push(rest.to_string());
                break;
            }
            // 二分行内最大可容前缀（自然宽随前缀单调不减）
            let chars: Vec<char> = rest.chars().collect();
            let mut low = 0usize;
            let mut high = chars.len();
            while high - low > 1 {
                let mid = (low + high) / 2;
                let candidate: String = chars[..mid].iter().collect();
                if measure_width(&candidate) <= avail {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            if low == 0 {
                low = 1; // 单字符超宽也必须推进，避免死循环
            }
            // 对齐 break-words：可容纳前缀内存在空格时优先在最后一个空格
            // 断行（词整体下移，断点空格随换行消耗）；仅长词无空格可依时
            // 才按字符硬折，硬折后紧随的空格属词间间隔，一并消耗
            let mut cut = low;
            let mut consumed = low;
            if let Some(space) = chars[..low].iter().rposition(|&c| c == ' ') {
                if space > 0 {
                    cut = space;
                    consumed = space + 1;
                }
            } else {
                while consumed < chars.len() && chars[consumed] == ' ' {
                    consumed += 1;
                }
            }
            let byte_len: usize = chars[..consumed].iter().map(|c| c.len_utf8()).sum();
            lines.push(chars[..cut].iter().collect());
            rest = &rest[byte_len..];
        }
        if overflow {
            break;
        }
    }
    if (overflow || source_truncated) && !lines.is_empty() {
        let last = lines.last_mut().unwrap();
        loop {
            let mut candidate = last.clone();
            candidate.push('…');
            if measure_width(&candidate) <= avail {
                *last = candidate;
                break;
            }
            if last.is_empty() {
                *last = "…".to_string();
                break;
            }
            last.pop();
        }
    }
    lines.join("\n")
}

/// 大图预览高度：实图解码完成后用实图尺寸，否则退回元数据；等比缩放宽
/// 到列宽，上限 120。无尺寸信息时不渲染
fn large_preview_geometry(item: &ClipItemSummary, base_width: f32) -> Option<f32> {
    let decoded_size = SELECTED_FULL_IMAGE.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|(id, _)| id == &item.id)
            .and_then(|(_, image)| image.as_ref())
            .map(|image| image.size())
    });
    let (width, height) = match decoded_size {
        Some(size) => (size.width as f32, size.height as f32),
        None => (item.image_width? as f32, item.image_height? as f32),
    };
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some(
        (base_width * height / width)
            .min(LARGE_PREVIEW_MAX_H)
            .max(1.0),
    )
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

/* ───────────────── 元信息行（对齐 getItemDetailMeta）───────────────── */

fn build_meta(item: &ClipItemSummary) -> String {
    let mut parts = vec![
        item.source_app.clone().unwrap_or_else(|| "未知来源".into()),
        format_relative_time_or_unused(
            item.last_used_at
                .as_deref()
                .or(Some(item.created_at.as_str())),
        ),
    ];
    if item.r#type == "image" {
        if let (Some(width), Some(height)) = (item.image_width, item.image_height) {
            parts.push(format!("{width} × {height}"));
        }
    }
    if let Some(label) = format_file_size(item.file_size) {
        parts.push(label);
    }
    if item.r#type == "file" {
        if item.file_count > 0 {
            parts.push(format!("{} 个文件", item.file_count));
        }
        if item.directory_count > 0 {
            parts.push(format!("{} 个文件夹", item.directory_count));
        }
    }
    parts.join(" • ")
}

/// 文件大小人类可读（对齐 formatFileSize：1024 进制，按值选小数位）
pub(crate) fn format_file_size(bytes: Option<i64>) -> Option<String> {
    let bytes = bytes?;
    if bytes <= 0 {
        return None;
    }
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit_index = 0usize;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    let digits = if unit_index == 0 {
        0
    } else if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    Some(format!("{value:.digits$} {}", UNITS[unit_index]))
}

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

pub fn paste_index(app: &App, index: usize, as_file_requested: bool) {
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    paste_item(app, &item, as_file_requested);
}

/// 选中条目上屏（键盘路径）；Shift+Enter 仅对图片以文件形式
pub fn paste_selected(app: &App, as_file_requested: bool) {
    let index = app
        .with_search(|win| win.get_selected().max(0) as usize)
        .unwrap_or(0);
    paste_index(app, index, as_file_requested);
}

fn paste_item(app: &App, item: &ClipItemSummary, as_file_requested: bool) {
    tooltip::cancel(app);
    let settings = app.state.current_settings();
    let option = PasteOption {
        restore_clipboard_after_paste: settings.restore_clipboard_after_paste,
        paste_to_target: true,
        as_file: as_file_requested && item.r#type == "image",
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
        option.as_file,
        owner_hwnd,
    )?;

    let session = app.state.search_session();
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
                    thread::sleep(INJECT_DELAY);
                    if !paste_support::trigger_ctrl_v() {
                        warn!("搜索回贴 Ctrl+V 注入失败");
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
                refresh_selected_detail(app);
                update_empty_state(app);
                build_rows(app);
                sync_height(app, true);
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
        refresh_selected_detail(app);
    }
    app.with_search(|win| {
        win.set_result_count(items.len() as i32);
        win.set_result_total(total as i32);
    });
    update_empty_state(app);
    build_rows(app);
    sync_height(app, true);
}

/* ───────────────── 悬停预览（仅图片条目）───────────────── */

pub fn search_is_active(app: &App) -> bool {
    app.state.is_search_active()
}

pub fn row_hover(app: &App, index: usize, mouse_x: f32, mouse_y: f32) {
    if !app.state.is_search_active() {
        return;
    }
    // 旧版搜索窗口 shouldShow 仅对图片条目启用悬浮预览
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    if item.r#type != "image" {
        return;
    }
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

/// 目标高度 = min(620, 5 + chrome + 列表内容高)（对齐
/// syncWindowHeightWithContent）。0ms 延迟合并同帧多次触发并等布局把新
/// 模型高度算出；ALLOW_SHRINK 由结构性变化置位、应用后消费（棘轮）：
/// 选中展开的 sync(false) 不会取消结构性收缩许可
pub fn sync_height(app: &App, allow_shrink: bool) {
    if allow_shrink {
        ALLOW_SHRINK.with(|flag| flag.set(true));
    }
    let token = HEIGHT_TOKEN.with(|value| {
        value.set(value.get() + 1);
        value.get()
    });
    let app_cb = app.clone();
    slint::Timer::single_shot(Duration::from_millis(0), move || {
        if HEIGHT_TOKEN.with(|value| value.get()) != token {
            return;
        }
        let Some(win) = app_cb.search.upgrade() else {
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
            // 布局尚未把新模型的高度算出（0ms 定时器可能先于渲染帧）：
            // 提前返回并保留收缩许可，等随后的 heights-changed 带新值重入
            return;
        }
        let allow = ALLOW_SHRINK.with(|flag| flag.get());
        ALLOW_SHRINK.with(|flag| flag.set(false));
        if !allow && target < last {
            return;
        }
        LAST_HEIGHT.with(|value| value.set(target as f32));
        win.window().set_size(PhysicalSize::new(
            (geo.get_window_width() * scale).round() as u32,
            target as u32,
        ));
        // 打开后首次高度落定：以新尺寸重新居中（左上角锚定的增长会让
        // 窗口偏离打开时的居中位置；后续会话内的高度变化不再干预，
        // 用户拖动后的位置也不受影响）
        if JUST_OPENED.with(|flag| flag.get()) {
            JUST_OPENED.with(|flag| flag.set(false));
            let _ = position_on_cursor_monitor(
                &win,
                (geo.get_window_width() * scale).round() as i32,
                target,
            );
        }
    });
}

/// 列表内容实测高变化（viewport-height）：视作安全网重同步（允许增长；
/// 收缩许可仍由结构性变化的 sync(true) 供给）
pub fn heights_changed(app: &App) {
    sync_height(app, false);
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
    fn file_size_matches_legacy_digit_rules() {
        assert_eq!(format_file_size(None), None);
        assert_eq!(format_file_size(Some(0)), None);
        assert_eq!(format_file_size(Some(-5)), None);
        assert_eq!(format_file_size(Some(512)), Some("512 B".into()));
        assert_eq!(format_file_size(Some(2048)), Some("2.00 KB".into()));
        assert_eq!(format_file_size(Some(15 * 1024)), Some("15.0 KB".into()));
        assert_eq!(format_file_size(Some(200 * 1024)), Some("200 KB".into()));
    }

    #[test]
    fn meta_joins_source_time_and_details() {
        let mut item = summary_of("image");
        item.image_width = Some(1920);
        item.image_height = Some(1080);
        item.file_size = Some(2048);
        let meta = build_meta(&item);
        assert!(meta.starts_with("浏览器 • "));
        assert!(meta.contains("1920 × 1080"));
        assert!(meta.contains("2.00 KB"));
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

    /// 确定性度量：每字符 10 逻辑像素，换行符不计
    fn fake_measure() -> impl FnMut(&str) -> f32 {
        |text: &str| text.chars().filter(|c| *c != '\n').count() as f32 * 10.0
    }

    #[test]
    fn hard_wrap_keeps_short_text_on_one_line() {
        let out = hard_wrap_preview(&mut fake_measure(), "短文本", 100.0, false, 3);
        assert_eq!(out, "短文本");
    }

    #[test]
    fn hard_wrap_breaks_long_text_at_char_boundary() {
        // 每行容量 4 字符（40px），10 字符文本折成 3 行
        let out = hard_wrap_preview(&mut fake_measure(), "0123456789", 40.0, false, 3);
        assert_eq!(out, "0123\n4567\n89");
    }

    #[test]
    fn hard_wrap_prefers_space_boundary_over_mid_word_cut() {
        // 每行容量 6 字符（60px）："hello " 恰好可容 → 在空格断行，
        // 词整体移到下一行（对齐 break-words），而不是切成 "hello w"
        let out = hard_wrap_preview(&mut fake_measure(), "hello world", 60.0, false, 3);
        assert_eq!(out, "hello\nworld");
    }

    #[test]
    fn hard_wrap_consumes_break_space_after_hard_word_split() {
        // 每行容量 5 字符（50px）：'hello' 无空格可依被硬折，紧随的词间
        // 空格随断行消耗，下一行从 'world' 整词开始
        let out = hard_wrap_preview(&mut fake_measure(), "hello world", 50.0, false, 3);
        assert_eq!(out, "hello\nworld");
    }

    #[test]
    fn hard_wrap_fills_lines_exactly_without_ellipsis() {
        // 12 字符恰好占满 3 行 × 4 字符：不溢出不补省略号
        let out = hard_wrap_preview(&mut fake_measure(), "0123456789AB", 40.0, false, 3);
        assert_eq!(out, "0123\n4567\n89AB");
    }

    #[test]
    fn hard_wrap_appends_ellipsis_when_line_budget_exceeded() {
        // 13 字符超出 3 行预算：截断后末行收缩以容纳省略号
        let out = hard_wrap_preview(&mut fake_measure(), "0123456789ABC", 40.0, false, 3);
        assert_eq!(out, "0123\n4567\n89A…");
    }

    #[test]
    fn hard_wrap_appends_ellipsis_for_truncated_source() {
        // 源本身被入库预览截断：未超行预算也要补省略号
        let out = hard_wrap_preview(&mut fake_measure(), "0123456789", 40.0, true, 3);
        assert_eq!(out, "0123\n4567\n89…");
    }

    #[test]
    fn hard_wrap_preserves_explicit_newlines() {
        let out = hard_wrap_preview(&mut fake_measure(), "ab\ncd", 100.0, false, 3);
        assert_eq!(out, "ab\ncd");
    }

    #[test]
    fn hard_wrap_never_stalls_on_oversized_char() {
        // 单字符宽度超过可用宽：每行仍推进一个字符，预算耗尽后以裸省略号收尾
        let out = hard_wrap_preview(&mut fake_measure(), "宽宽宽", 5.0, false, 2);
        assert_eq!(out, "宽\n…");
    }

    #[test]
    fn hard_wrap_handles_empty_and_zero_width() {
        let mut measure = fake_measure();
        assert_eq!(hard_wrap_preview(&mut measure, "", 100.0, false, 3), "");
        assert_eq!(
            hard_wrap_preview(&mut measure, "文本", 0.0, false, 3),
            "文本"
        );
    }
}
