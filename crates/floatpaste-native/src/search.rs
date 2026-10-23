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
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, ModelRc, PhysicalPosition, PhysicalSize, VecModel};
use tracing::{info, warn};

use floatpaste_core::domain::clip_item::{
    available_paste_modes, resolve_secondary_mode, ClipItemSummary, ClipType, PasteMode,
    PasteOption, SearchFilters, SearchQuery, SearchResult, SearchSort, TextKind,
};
use floatpaste_core::domain::error::AppError;
use floatpaste_core::domain::settings::{PasteTrigger, SecondaryPasteDefaults, UserSetting};
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::picker_position::{
    current_cursor_point, primary_monitor_work_area, work_area_from_point, ScreenRect,
};
use floatpaste_core::platform::windows::session_keyboard::parse_session_combo;
use floatpaste_core::platform::windows::window_control;
use floatpaste_core::services::clip_display::{
    build_meta, paste_action_summaries, secondary_action_label,
};
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
use crate::{
    PasteMenuGeometry, SearchActionKeys, SearchGeometry, SearchRow, SearchTagChip, SearchWindow,
};

/// 单页条数（对齐旧版 SEARCH_PAGE_SIZE）
const PAGE_SIZE: u32 = 50;
/// 关键词防抖（对齐 SEARCH_INPUT_DEBOUNCE_MS）
const INPUT_DEBOUNCE_MS: u64 = 200;
/// 二次确认删除的保持时长（对齐 ARMED_DELETE_RESET_DELAY_MS）
const DELETE_ARM_TIMEOUT_MS: u64 = 3000;
/// 焦点监视轮询间隔（对齐 SEARCH_FOCUS_WATCH_INTERVAL_MS）
const FOCUS_WATCH_INTERVAL_MS: u64 = 120;
const RESTORE_DELAY: Duration = Duration::from_millis(90);
const INJECT_DELAY: Duration = Duration::from_millis(60);
/// 关闭路径还原前台后的激活落地等待上限：正常几十毫秒内完成，超时则
/// 尽力停屏（还原未落地时前台会由系统回收，不阻塞用户）
const RESTORE_SETTLE_MAX: Duration = Duration::from_millis(200);

/* ───────────────── 会话生命周期 ───────────────── */

/// 全局搜索快捷键命中：活跃则关闭并还原目标；速贴活跃则先收起速贴（不还
/// 原目标，焦点交给搜索窗口），再打开搜索（对齐 open_search_global）。
/// 每击必响应，不做防抖（系统重复由注册层 MOD_NOREPEAT 过滤）
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
/// 已活跃且在前台时仅聚回焦点（保留关键词与列表状态，对齐 is_search_active
/// 分支）；否则速贴活跃先收起（不还原目标，焦点交给搜索窗口）再走完整
/// 打开流程
pub fn open_global(app: &App) {
    // 互斥收口：托盘菜单开着时其键鼠会话（单例）保留无碍，但浮层不该与
    // 搜索窗同屏滞留
    crate::tray_menu::close(app);
    if app.state.is_search_active() {
        let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
        // 已不在前台 = 失焦自动关闭正待焦点看门狗执行（120ms 轮询间隔），
        // 中间态里打开语义应是新会话，落完整打开流程；否则旧会话（筛选
        // 空态的收缩尺寸与激活筛选）会原样回来
        if hwnd != 0 && ActiveAppResolver::current_foreground_hwnd() == Some(hwnd) {
            if let Err(error) = window_control::restore_window_and_focus(hwnd) {
                warn!("搜索窗口获取焦点失败: {error}");
            }
            return;
        }
    }
    if app.state.is_picker_active() {
        picker::hide(app, false);
    }
    open(app);
}

/// 打开搜索会话（对齐 open_search_global）
pub fn open(app: &App) {
    open_inner(app, Vec::new());
}

/// 从设置窗标签区打开：预置标签筛选。首开与快捷键打开同一条上屏链
/// （全尺寸居中、加载态起步），差异只有一点：落地后主动伸缩高度
/// （PRESET_RESIZE）——首开起步于默认视图高度、与内容自然高脱钩，
/// changed 哑火时窗口会停在起步高度不跟随内容高。会话已激活时（反复
/// 点条目数的浏览流）直接切筛选，不隐藏不重开
pub fn open_with_tags(app: &App, preset_tags: Vec<String>) {
    if app.state.is_search_active() {
        apply_preset_filter(app, preset_tags);
        return;
    }
    open_inner(app, preset_tags);
}

/// 预置标签与当前标签词汇求交集（大小写不敏感）：目标标签已被清理时
/// 被滤掉（chip 不渲染、只能靠清除筛选退出）。空结果由调用方决定语义
fn intersect_known_tags(app: &App, preset_tags: Vec<String>) -> Vec<String> {
    let known: HashSet<String> = app
        .core()
        .repository
        .list_tags()
        .map(|tags| {
            tags.into_iter()
                .map(|tag| tag.name.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    preset_tags
        .into_iter()
        .filter(|tag| known.contains(&tag.to_lowercase()))
        .collect()
}

/// 已打开会话上应用预置筛选（设置窗条目数反复点击）：等价点了一组
/// 别的标签 chip——替换筛选、清关键词与类型筛选、重建 chips、重查；
/// 落地经 PRESET_RESIZE 顶边锚定伸缩。同筛选重复点击不重查
fn apply_preset_filter(app: &App, preset_tags: Vec<String>) {
    let Some(win) = app.search.upgrade() else {
        return;
    };
    let tags = intersect_known_tags(app, preset_tags);
    let same = QUERY.with(|state| {
        let current: HashSet<String> = state
            .borrow()
            .tags
            .iter()
            .map(|tag| tag.to_lowercase())
            .collect();
        let next: HashSet<String> = tags.iter().map(|tag| tag.to_lowercase()).collect();
        current == next
    });
    if same {
        return;
    }
    QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.tags = tags;
        state.filter = 0;
        state.favorited_only = false;
        state.text_kind = 0;
    });
    // 「看这个标签下的条目」从全新筛选开始
    win.set_keyword("".into());
    win.set_active_filter(0);
    win.set_favorited_only(false);
    win.set_active_text_kind(0);
    PRESET_RESIZE.with(|flag| flag.set(true));
    refresh_tags(app);
    run_query(app);
}

fn open_inner(app: &App, preset_tags: Vec<String>) {
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd == 0 {
        warn!("搜索窗口 HWND 尚未就绪，无法显示");
        return;
    }

    // 先捕获回贴目标再显示（此刻前台还是用户正在用的窗口）
    let target = ActiveAppResolver::current_foreground_window_handle();
    let session = SearchSession {
        session_id: crate::app_state::next_session_id(),
        target_window_hwnd: target,
    };
    app.state.set_search_session(session);
    let epoch = app.state.begin_search_activation();
    info!(
        session_id = session.session_id,
        target_window = ?target,
        "打开 Search"
    );

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
    // 外部预置的标签筛选：与当前标签词汇求交集后再注入，交集为空则
    // 退回常规打开
    let mut preset_active = false;
    if !preset_tags.is_empty() {
        let tags = intersect_known_tags(app, preset_tags);
        if !tags.is_empty() {
            QUERY.with(|state| state.borrow_mut().tags = tags);
            preset_active = true;
        }
    }
    // 出现滑入准备：停屏期关动画重置内容偏移，暖首帧落盘的即 4px 起步态
    // （见 search.slint enter-offset 注释）
    win.set_enter_animated(false);
    win.set_enter_offset(4.0);
    // 尺寸先于定位：首开会话时窗口还是装配期自然尺寸（根布局钳制，不是
    // 设计尺寸），直接拿 window().size() 居中必偏（winit set_size 亦非
    // 同步可读回）。定位用同一组计算值，不经窗口回读
    let scale = win.window().scale_factor();
    let geo = win.global::<SearchGeometry>();
    let width_px = (geo.get_window_width() * scale).round() as i32;
    let base = OPEN_BASE_HEIGHT.with(|value| value.get()) as i32;
    // 首开（无历史高度）直接用最大高度：内容加载后棘轮目标通常也是
    // max，避免开窗后再 resize 造成跳动/闪烁
    let height_px = if base > 0 {
        base
    } else {
        (geo.get_window_max_height() * scale).round() as i32
    };
    // 高度基线对齐到本次实际应用值：加载态的中间高度（低于基线）在无
    // 收缩许可时被跳过，内容落定只发生一次尺寸变化。预置筛选首开同
    // 按默认视图高度起步（对齐快捷键打开表现），落地后收缩到结果高度
    LAST_HEIGHT.with(|value| value.set(height_px as f32));
    tracing::debug!("开窗起步高度: {height_px} (基线 {base})");

    finish_show(app, &win, hwnd, width_px, height_px, epoch);
    refresh_tags(app);
    if preset_active {
        PRESET_RESIZE.with(|flag| flag.set(true));
    }
    // 行模型已清空：首帧显示加载态（对齐旧版 keyword 重置后 isLoading）
    run_query(app);
}

/// 上屏尾段：光标所在工作区居中落位（失败退回仅定尺寸+上次原位兜底）
/// → 暖帧全量重绘 → 抢前台 → 解锁滑入 → 重挂浮层样式与材质 → 启动
/// 焦点监视
fn finish_show(
    app: &App,
    win: &SearchWindow,
    hwnd: isize,
    width_px: i32,
    height_px: i32,
    epoch: u64,
) {
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
            return;
        }
    }
    win32_ext::warm_surface(hwnd);
    win32_ext::force_full_repaint(win);

    if let Err(error) = window_control::restore_window_and_focus(hwnd) {
        warn!("搜索窗口获取焦点失败: {error}");
    }
    // 上屏即滑入：解锁动画归零，内容从 4px 滑到位（窗底不动，不露窗外）
    win.set_enter_animated(true);
    win.set_enter_offset(0.0);
    // 兜底重挂浮层样式（可聚焦变体，不带 WS_EX_NOACTIVATE）并复查置顶
    overlay::after_show_focusable(hwnd);
    apply_material(app, win, hwnd);

    begin_focus_watcher(app, epoch);
}

/// 隐藏（对齐 hide_search_window / hide_search_and_restore_target）。
/// restore_target=true 立即把前台还给会话目标（Esc/快捷键关闭/上屏路径，
/// 关闭时前台在搜索窗自己手上）；false 时不动前台（失焦自动关闭——
/// 用户点击处已持有前台）
pub fn hide(app: &App, restore_target: bool) {
    tooltip::cancel(app);
    app.state.end_search_activation();
    let session = app.state.search_session();
    // 先还原前台再停屏：此刻搜索窗还是前台进程，SetForegroundWindow 放行
    // 有保证；若先停屏（窗口移出屏幕），系统会立即把前台转让给下一 z 序
    // 窗口，被测实例随之失去前台进程身份，随后的还原被前台锁拒绝——
    // 还原退化为与系统转让赛跑（e2e 场景 8 间歇失败的实证根因）
    if restore_target {
        if let Some(target_hwnd) = session.target_window_hwnd {
            if ActiveAppResolver::restore_foreground_window(target_hwnd) {
                // 前台转移经跨线程消息往返异步落地（目标窗口线程处理
                // WM_ACTIVATE），立即停屏（前台窗移出屏幕）会打断转移、
                // 前台清空成 NULL——还原退化为碰运气。等激活落地再停屏，
                // 上限一拍
                let deadline = Instant::now() + RESTORE_SETTLE_MAX;
                while Instant::now() < deadline {
                    if ActiveAppResolver::current_foreground_hwnd() == Some(target_hwnd) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    if let Some(win) = app.search.upgrade() {
        // 会话收尾复位动作菜单（悬空行号不带入下次打开）
        win.set_actions_menu_row(-1);
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
    app.state.set_search_session(Default::default());
    // 未消费的预置伸缩闸随会话清理：不得泄漏到下一次打开的落地
    PRESET_RESIZE.with(|flag| flag.set(false));
    info!(session_id = session.session_id, "隐藏 Search，会话已清理");
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
    /// 预置筛选落地的主动伸缩闸：open_with_tags 首开/切筛选置位，
    /// apply_page 落地时消费——泵帧后经 apply_height 顶边锚定伸缩。
    /// 不依赖 heights-changed（值语义信号，值净变化才触发）：首开路径
    /// 窗口起步于默认视图高度、与内容自然高脱钩，中间态被合并渲染或
    /// 值巧合相等时 changed 哑火，窗口会停在起步高度不跟随内容
    static PRESET_RESIZE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 预置筛选落地后主动伸缩的延迟：跨过至少 2-3 个渲染帧，保证布局
/// pass 已把回填后的内容自然高写入属性，读数即终值
const PRESET_RESIZE_DELAY_MS: u64 = 50;

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
    info!(
        session_id = app.state.search_session().session_id,
        "隐藏 Search（进入编辑器）"
    );
}

/// 从编辑器返回搜索：仅恢复窗口可见性与焦点，**不发会话开始**
/// （对齐 restore_search_after_editor）——保留进入编辑器时的选中、
/// 关键词与滚动位置，自然衔接原上下文
pub fn restore_after_editor(app: &App) {
    let session = app.state.search_session();
    let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let Some(win) = app.search.upgrade() else {
        return;
    };
    // 出现滑入准备：同 open，停屏期无动画重置内容偏移，暖首帧即起步态
    win.set_enter_animated(false);
    win.set_enter_offset(4.0);
    // 上屏前先暖表面（同步泵一次 WM_PAINT 呈现），移回原位即有内容
    win32_ext::warm_surface(hwnd);
    win32_ext::force_full_repaint(&win);
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
    info!(session_id = session.session_id, "从编辑器恢复 Search");
    // 上屏即滑入（同 open）
    win.set_enter_animated(true);
    win.set_enter_offset(0.0);
    overlay::after_show_focusable(hwnd);
    apply_material(app, &win, hwnd);
    let epoch = app.state.begin_search_activation();
    // 失焦自动关闭的监视随激活重启：旧壳是常驻 Focused(false) 监听按
    // is_search_active 门控，编辑器返回后依然生效；本实现的轮询线程
    // 生命周期与会话绑定，必须在此重新拉起（边沿触发不会误判首拍失焦）
    begin_focus_watcher(app, epoch);
    info!("从 Editor 返回 Search");
}

/// 键盘交还搜索窗口（对齐 SEARCH_INPUT_RESUME_EVENT）
pub fn resume_input(app: &App) {
    app.with_search(|win| {
        win.set_input_suspended(false);
        win.invoke_focus_input();
    });
}

/// 光标所在显示器工作区（物理坐标）。GetCursorPos 在输入桌面短暂不可达时
/// 返回拒绝访问（会话切换/显示器栈重初始化、断连 RDP 会话）：先短重试
/// 消化瞬态，持续失败退回主屏工作区——落位宁可贵为主屏居中，不可让开窗
/// 停在停屏位成为不可见窗口
fn cursor_monitor_work_area() -> Option<ScreenRect> {
    let mut last_error = None;
    for _ in 0..5 {
        match current_cursor_point() {
            Ok(cursor) => {
                return match work_area_from_point(cursor) {
                    Ok(work) => Some(work),
                    Err(error) => {
                        tracing::warn!("搜索落位: cursor={cursor:?} 工作区读取失败: {error}");
                        None
                    }
                };
            }
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(30));
            }
        }
    }
    match primary_monitor_work_area() {
        Ok(work) => {
            tracing::warn!(
                "搜索落位: 光标读取持续失败（{last_error:?}），退回主屏工作区 work={work:?}"
            );
            Some(work)
        }
        Err(error) => {
            tracing::warn!("搜索落位: 光标读取失败（{last_error:?}）且主屏工作区不可得: {error}");
            None
        }
    }
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

/// 会话开始重置（对齐 SEARCH_SESSION_START）：关键词/筛选/选中/错误
/// 全部归零，行模型清空进入加载态；滚动回顶由紧随的 run_query 统一负责
fn reset_session_state(app: &App) {
    refresh_copy(app);
    let Some(win) = app.search.upgrade() else {
        return;
    };
    QUERY.with(|state| *state.borrow_mut() = QueryState::default());
    CANCEL_DEBOUNCE.with(|slot| *slot.borrow_mut() = None);
    SELECTED_ID.with(|slot| *slot.borrow_mut() = None);
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = None);
    DELETE_TOKEN.with(|token| token.set(token.get() + 1));
    // 高度基线（LAST_HEIGHT/OPEN_BASE_HEIGHT）跨会话保留不清零：open
    // 直接用「默认视图」高度起步并把 LAST_HEIGHT 对齐到实际应用值。
    // 清零会让加载态的中间高度被视为「增长」而应用，窗口先缩后长、
    // 两次跳动（闪烁 + 最终偏下）
    ALLOW_SHRINK.with(|flag| flag.set(false));

    win.set_keyword("".into());
    win.set_input_suspended(false);
    win.set_selected(0);
    win.set_delete_armed_row(-1);
    win.set_active_filter(0);
    win.set_favorited_only(false);
    win.set_active_text_kind(0);
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
    win.invoke_focus_input();
}

/// 焦点监视（对齐旧版 searchFocusWatcher）：上一拍还在前台、这一拍丢失
/// 且光标不在窗口内 → 关闭且不还原目标。边沿触发：焦点从未拿到时不误关。
/// `epoch` 是本次会话的代际：watcher 的退出检查（is_search_active）落在
/// sleep 醒后，若新会话已重新激活，旧 watcher 会误接管新会话——代际
/// 不匹配即退出，只监控自己启动的那一代
fn begin_focus_watcher(app: &App, epoch: u64) {
    let app_for_watch = app.clone();
    thread::spawn(move || {
        let mut was_foreground = false;
        loop {
            thread::sleep(Duration::from_millis(FOCUS_WATCH_INTERVAL_MS));
            if app_for_watch.core().is_quitting()
                || !app_for_watch.state.is_search_active()
                || app_for_watch.state.search_session_epoch() != epoch
            {
                return;
            }
            let hwnd = app_for_watch.state.search_hwnd.load(Ordering::SeqCst);
            if hwnd == 0 {
                return;
            }
            let foreground = ActiveAppResolver::current_foreground_hwnd();
            let is_foreground = foreground == Some(hwnd);
            if was_foreground && !is_foreground {
                // 前台转移到设置窗：设置窗标签区「条目数点击」的浏览流
                // 要求搜索窗保持显示、由 open_with_tags 切筛选而非关闭
                // 重开（关闭重开=可见的隐藏-再显示循环）。点桌面或其他
                // 应用仍照常自关
                let settings = app_for_watch.state.settings_hwnd.load(Ordering::SeqCst);
                let to_settings = settings != 0 && foreground == Some(settings);
                let cursor_outside = window_control::is_cursor_inside_window(hwnd)
                    .map(|inside| !inside)
                    .unwrap_or(false);
                if cursor_outside && !to_settings {
                    let app = app_for_watch.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        hide(&app, false);
                    });
                    return;
                }
                // 豁免时不得更新 was_foreground：边沿信号必须保持——
                // 否则本拍消费掉「失焦中」状态，前台再转移到别处（点
                // 桌面/其他应用）时不再触发边沿，窗口永不自关
                if to_settings {
                    continue;
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
    /// 0=全部类型 2=文本 3=图片 4=文件；收藏为独立条件
    filter: u32,
    favorited_only: bool,
    /// 文本用途细分筛选（仅 filter==2 时参与查询）：
    /// 0=全部 1=普通 2=链接 3=代码 4=路径
    text_kind: u32,
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

impl QueryState {
    fn select_filter(&mut self, filter: u32) {
        if filter == 1 {
            self.favorited_only = !self.favorited_only;
        } else {
            self.filter = filter;
            if filter != 2 {
                self.text_kind = 0;
            }
        }
    }
}

thread_local! {
    static QUERY: RefCell<QueryState> = RefCell::new(QueryState::default());
    /// 关键词防抖定时器（restartable：覆盖即停旧计时）
    static CANCEL_DEBOUNCE: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static SELECTED_ID: RefCell<Option<String>> = const { RefCell::new(None) };
    static ARMED_DELETE: RefCell<Option<String>> = const { RefCell::new(None) };
    static DELETE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static LAST_HEIGHT: Cell<f32> = const { Cell::new(0.0) };
    // 开窗起步基线：只记「默认视图」（无关键词/筛选/标签，会话重置后
    // 必然回到的形态）落地后的高度。任何会话内收缩态——筛选空态、
    // 少结果、关键词命中几条——都不代表下次开窗的终态，记入基线会让
    // 重开先见矮窗、依赖落地后再涨一次
    static OPEN_BASE_HEIGHT: Cell<f32> = const { Cell::new(0.0) };
    static ALLOW_SHRINK: Cell<bool> = const { Cell::new(false) };
}

fn build_query(
    keyword: &str,
    filter: u32,
    text_kind: u32,
    tags: &[String],
    offset: u32,
    favorited_only: bool,
) -> SearchQuery {
    let mut filters = SearchFilters::default();
    filters.favorited_only = favorited_only.then_some(true);
    match filter {
        2 => {
            filters.clip_type = Some(ClipType::Text);
            filters.text_kind = match text_kind {
                1 => Some(TextKind::Plain),
                2 => Some(TextKind::Link),
                3 => Some(TextKind::Code),
                4 => Some(TextKind::Path),
                _ => None,
            };
        }
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
    let (filter, text_kind, favorited_only, tags, seq) = QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.keyword = keyword.clone();
        state.offset = 0;
        state.fetching = true;
        state.fetching_next = false;
        state.seq += 1;
        (
            state.filter,
            state.text_kind,
            state.favorited_only,
            state.tags.clone(),
            state.seq,
        )
    });
    win.set_fetching_next(false);
    let keep_loading = win.get_rows().row_count() == 0;
    win.set_loading(keep_loading);
    // 行集即将整体替换：视口先回顶，深滚动位置上的空态/短结果会整体
    // 落在视口外（Flickable 不随内容收缩钳制 content-y）
    win.set_scroll_generation(win.get_scroll_generation() + 1);

    let core = app.core().clone();
    let app_cb = app.clone();
    thread::spawn(move || {
        let query = build_query(&keyword, filter, text_kind, &tags, 0, favorited_only);
        let result = core.repository.search(query);
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok(result) => apply_page(&app_cb, result, seq, false),
            Err(error) => {
                warn!("搜索查询失败: {error}");
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
                        win.set_load_error_text("数据读取失败，请重试".into());
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
    // 预置筛选触发的落地：主动伸缩高度，延迟一拍执行——apply_page 同
    // 步段读布局属性是陈旧值（行模型刚写入、布局 pass 未跑），50ms≈
    // 3 帧后 natural-height 必已写入终值，读数即真值。与 heights-changed
    // 双保险：changed 触发（值净变化）则先一步伸缩，此处 target==last
    // 幂等无事；changed 哑火（前后内容高相等，连续打开同标签）则由此
    // 处兜住——首开起步于默认视图高度，与内容高脱钩，不伸缩会停在
    // 起步高度
    if PRESET_RESIZE.with(|flag| flag.replace(false)) {
        let app_cb = app.clone();
        slint::Timer::single_shot(Duration::from_millis(PRESET_RESIZE_DELAY_MS), move || {
            preset_resize(&app_cb);
        });
    }
}

/// 预置筛选落地的主动伸缩：读布局终值经 apply_height 顶边锚定伸缩
/// （左上不动、底边伸缩），等价用户点 chip 筛选的几何表现
fn preset_resize(app: &App) {
    if !app.state.is_search_active() {
        tracing::debug!("预置伸缩跳过（会话已结束）");
        return; // 会话已被 hide 清理，不得改动已关闭窗口
    }
    tracing::debug!("预置伸缩触发（落地后延迟拍）");
    ALLOW_SHRINK.with(|flag| flag.set(true));
    apply_height(app);
}

/// 触底预载（对齐 fetchNextPage：offset + items.length < total）
pub fn fetch_next(app: &App) {
    let (has_more, fetching, fetching_next, keyword, filter, text_kind, favorited_only, tags) =
        QUERY.with(|state| {
            let state = state.borrow();
            (
                state.has_more,
                state.fetching,
                state.fetching_next,
                state.keyword.clone(),
                state.filter,
                state.text_kind,
                state.favorited_only,
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
        let query = build_query(&keyword, filter, text_kind, &tags, offset, favorited_only);
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
/// 跳过活跃判定强制重查（列表行与标签筛选 chips——chips 仅在会话
/// 打开时加载，不随查询重载），保证返回时已反映变更
pub fn refresh_after_editor(app: &App) {
    run_query(app);
    refresh_tags(app);
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
    let (filter, kind, favorited_only) = QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.select_filter(filter.max(0) as u32);
        (state.filter, state.text_kind, state.favorited_only)
    });
    app.with_search(|win| {
        win.set_active_filter(filter as i32);
        win.set_favorited_only(favorited_only);
        win.set_active_text_kind(kind as i32);
    });
    run_query(app);
}

/// 文本用途细分筛选（仅文本筛选激活时出现）：0=全部 1=普通 2=链接 3=代码 4=路径
pub fn text_kind_selected(app: &App, kind: i32) {
    let kind = kind.max(0) as u32;
    QUERY.with(|state| state.borrow_mut().text_kind = kind);
    app.with_search(|win| win.set_active_text_kind(kind as i32));
    run_query(app);
}

pub fn clear_filters(app: &App) {
    QUERY.with(|state| {
        let mut state = state.borrow_mut();
        state.filter = 0;
        state.favorited_only = false;
        state.text_kind = 0;
        state.tags.clear();
    });
    app.with_search(|win| {
        win.set_active_filter(0);
        win.set_favorited_only(false);
        win.set_active_text_kind(0);
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
/// 行数据不区分选中形态）。动作目录与次级动作提示随设置快照生成
pub(crate) fn make_row(
    item: &ClipItemSummary,
    keys: &floatpaste_core::domain::settings::SessionKeys,
    defaults: &SecondaryPasteDefaults,
) -> SearchRow {
    let thumb = thumbnails::cached(&item.id);
    let has_thumb = thumb.is_some();
    let normal_preview = flatten_preview_newlines(&item.content_preview);

    SearchRow {
        has_html: item.has_html,
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
        actions: picker::action_items(&paste_action_summaries(
            item,
            &keys.confirm,
            &keys.confirm_as_file,
            defaults,
        )),
        secondary_label: secondary_action_label(item, defaults)
            .unwrap_or_default()
            .into(),
    }
}

/// 全量重建行模型（查询落地/删除/收藏视图移除/缩略图回填/宽度重裁）
fn build_rows(app: &App) {
    let Some(win) = app.search.upgrade() else {
        return;
    };
    let items = app.state.search_items();
    let settings = app.state.current_settings();
    let selected_id = SELECTED_ID.with(|slot| slot.borrow().clone());
    let selected_index = items
        .iter()
        .position(|item| Some(&item.id) == selected_id.as_ref());

    let rows: Vec<SearchRow> = items
        .iter()
        .map(|item| make_row(item, &settings.session_keys, &settings.secondary_paste))
        .collect();

    let model = Rc::new(VecModel::from(rows));
    ROW_MODEL.with(|slot| *slot.borrow_mut() = Some(model.clone()));
    win.set_selected(selected_index.unwrap_or(0) as i32);
    // 行集整体重建：菜单锚定的行号可能漂移，收起动作菜单
    win.set_actions_menu_row(-1);
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

pub fn refresh_copy(app: &App) {
    let keys = app.state.current_settings().session_keys;
    app.with_search(|win| {
        win.set_action_keys(SearchActionKeys {
            confirm: keys.confirm.into(),
            confirm_as_file: keys.confirm_as_file.into(),
            open_editor: keys.open_editor.into(),
            toggle_favorite: keys.toggle_favorite.into(),
            dismiss: keys.dismiss.into(),
            delete_entry: keys.delete_entry.into(),
        });
    });
    update_empty_state(app);
}

fn update_empty_state(app: &App) {
    let (keyword, has_filters) = QUERY.with(|state| {
        let state = state.borrow();
        (
            state.keyword.clone(),
            state.filter != 0
                || state.text_kind != 0
                || state.favorited_only
                || !state.tags.is_empty(),
        )
    });
    let (title, description, action) = floatpaste_core::services::clip_display::search_empty_state(
        &keyword,
        has_filters,
        app.state.current_settings().pause_monitoring,
    );
    app.with_search(|win| {
        win.set_empty_title(title.into());
        win.set_empty_description(description.into());
        win.set_empty_action(action);
    });
}

/* ───────────────── 条目动作 ───────────────── */

/// 挂 Win11 材质并回写门控（策略在 overlay::apply_material 统一）
fn apply_material(app: &App, win: &SearchWindow, hwnd: isize) {
    win.set_material_active(overlay::apply_material(app, hwnd));
}

pub fn paste_index(app: &App, index: usize, secondary: bool) {
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    let mode = if secondary {
        resolve_secondary_mode(&item, &app.state.current_settings().secondary_paste)
    } else {
        PasteMode::Original
    };
    paste_with_mode(app, &item, mode);
}

/// 「更多上屏方式」菜单：按目录序号显式选一次（含主动作），不改默认
fn paste_menu_action(app: &App, index: usize, action_index: usize) {
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    let mode = available_paste_modes(&item)
        .get(action_index)
        .copied()
        .unwrap_or(PasteMode::Original);
    paste_with_mode(app, &item, mode);
}

/// 键盘路径的行号来源：当前选中行（鼠标路径由行回调直接带行号）
fn selected_index(app: &App) -> usize {
    app.with_search(|win| win.get_selected().max(0) as usize)
        .unwrap_or(0)
}

/// 选中条目上屏（键盘路径）；次级动作按类别配置解析（普通文本等同主上屏）
pub fn paste_selected(app: &App, secondary: bool) {
    paste_index(app, selected_index(app), secondary);
}

fn paste_with_mode(app: &App, item: &ClipItemSummary, mode: PasteMode) {
    tooltip::cancel(app);
    let settings = app.state.current_settings();
    let option = PasteOption {
        restore_clipboard_after_paste: settings.restore_clipboard_after_paste,
        paste_to_target: true,
        mode,
    };
    if let Err(error) = execute_paste(app, &item.id, option) {
        warn!("搜索粘贴失败: {error}");
        crate::tray::notify_failure(&format!("上屏失败：{error}"));
    }
}

/* ───────────────── 「更多上屏方式」菜单 ───────────────── */

/// 锚定并打开动作菜单：锚点取展开箭头左下角、向下展开；贴近窗口底边
/// 时改为向上展开，菜单高度取共享尺寸与实际动作数；Rust 侧只钳纵向，横向钳制
/// 由界面侧按菜单宽执行
fn open_actions_menu(app: &App, index: usize, x: f32, y: f32) {
    tooltip::cancel(app);
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    if available_paste_modes(&item).len() <= 1 {
        return;
    }
    set_selected(app, index);
    app.with_search(|win| {
        let scale = win.window().scale_factor();
        let logical = win.window().size().to_logical(scale);
        let geometry = win.global::<PasteMenuGeometry>();
        let count = available_paste_modes(&item).len() as f32;
        let menu_height = count * geometry.get_row_height()
            + (count - 1.0) * geometry.get_spacing()
            + 2.0 * geometry.get_padding();
        let y = if y + menu_height > logical.height - 8.0 {
            (y - menu_height - 36.0).max(8.0)
        } else {
            y
        };
        win.set_actions_menu_row(index as i32);
        win.set_actions_menu_highlight(0);
        win.set_actions_menu_x(x);
        win.set_actions_menu_y(y);
    });
}

/// 键盘 ↑↓：菜单内循环移动高亮（与悬停高亮同一状态）
fn move_actions_menu(app: &App, delta: i32) {
    app.with_search(|win| {
        let row = win.get_actions_menu_row();
        if row < 0 {
            return;
        }
        let Some(item) = app.state.search_item_at(row as usize) else {
            return;
        };
        let count = available_paste_modes(&item).len().max(1);
        let current = win.get_actions_menu_highlight().max(0) as usize;
        win.set_actions_menu_highlight(next_navigation_index(count, current, delta < 0) as i32);
    });
}

fn close_actions_menu(app: &App) {
    app.with_search(|win| win.set_actions_menu_row(-1));
}

/// Enter：执行菜单内高亮的动作（先收菜单；上屏失败时面板保留供重试）
fn confirm_actions_menu(app: &App) {
    let (row, highlight) = app
        .with_search(|win| (win.get_actions_menu_row(), win.get_actions_menu_highlight()))
        .unwrap_or((-1, 0));
    if row < 0 {
        return;
    }
    close_actions_menu(app);
    paste_menu_action(app, row as usize, highlight.max(0) as usize);
}

/// 上屏流（对齐 PasteExecutor 搜索分支）：趁搜索窗口可见写剪贴板 →
/// 隐藏并还原会话目标 → 90ms → 恢复前台 → 60ms → 注入 Ctrl+V →
/// 调度快照恢复 → mark_used。失败提示经托盘通知呈现。
fn execute_paste(app: &App, id: &str, option: PasteOption) -> Result<(), AppError> {
    let detail = app.core().repository.get_item_for_paste(id)?;
    let session = app.state.search_session();

    // 管理员目标：照常写入剪贴板并还原目标焦点（用户可手动 Ctrl+V），
    // 仅跳过必然无效的按键注入，经托盘气泡一次性说明
    let admin_target = paste_flow::paste_target_requires_elevation(session.target_window_hwnd);
    let operation_id = paste_flow::next_paste_operation_id();
    let operation_gate = paste_support::lock_paste_operation()?;
    let previous_clipboard = paste_support::capture_snapshot_if_needed(&option)?;

    let owner_hwnd = {
        let hwnd = app.state.search_hwnd.load(Ordering::SeqCst);
        (hwnd != 0).then_some(hwnd)
    };
    let written_sequence = paste_support::publish_item(
        app.core(),
        &detail,
        option.mode,
        owner_hwnd,
        previous_clipboard.as_ref(),
    )?;
    drop(operation_gate);

    if admin_target {
        paste_flow::notify_admin_target_once(app, option.restore_clipboard_after_paste);
    }
    let failure_hint = paste_support::paste_failure_hint(option.restore_clipboard_after_paste);
    hide(app, true);

    let core = app.core().clone();
    let id = id.to_string();
    let session_id = session.session_id;
    thread::spawn(move || {
        let started = Instant::now();
        let failure = match session.target_window_hwnd {
            Some(target_hwnd) => {
                thread::sleep(RESTORE_DELAY);
                if core.is_quitting()
                    || floatpaste_core::platform::windows::clipboard_formats::clipboard_sequence()
                        != written_sequence
                {
                    return;
                }
                if ActiveAppResolver::restore_foreground_window_with_focus(target_hwnd, None) {
                    if !admin_target {
                        thread::sleep(INJECT_DELAY);
                        if !paste_support::trigger_ctrl_v_if_current(
                            written_sequence,
                            session_id,
                            operation_id,
                        ) {
                            warn!(session_id, operation_id, "搜索回贴 Ctrl+V 注入失败");
                            Some("未能自动粘贴。")
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    warn!(session_id, operation_id, "搜索回贴恢复目标窗口失败");
                    Some("未能切回目标窗口。")
                }
            }
            None => {
                warn!(session_id, operation_id, "搜索会话没有可恢复的目标窗口");
                Some("未找到需要粘贴的窗口。")
            }
        };
        if let Some(snapshot) = previous_clipboard {
            if let Err(error) = paste_support::schedule_clipboard_restore(
                &core,
                snapshot,
                owner_hwnd,
                session_id,
                operation_id,
                written_sequence,
            ) {
                warn!(session_id, operation_id, error = %error, "搜索上屏后安排剪贴板恢复失败");
            }
        }
        if let Err(error) = core.repository.mark_used(&id) {
            warn!(session_id, operation_id, error = %error, "搜索上屏后更新条目使用时间失败");
        }
        if let Some(reason) = &failure {
            warn!(
                operation_id,
                session_id,
                outcome = "failed",
                reason,
                elapsed_ms = started.elapsed().as_millis(),
                "搜索上屏流程失败"
            );
        } else {
            info!(
                operation_id,
                session_id,
                outcome = "success",
                elapsed_ms = started.elapsed().as_millis(),
                "搜索上屏流程完成"
            );
        }
        if let Some(reason) = failure.filter(|_| !admin_target) {
            if let Err(error) = slint::invoke_from_event_loop(move || {
                crate::tray::notify_failure(&format!("{reason}{failure_hint}"));
            }) {
                warn!(session_id, operation_id, error = %error, "搜索上屏结果回传 UI 失败");
            }
        }
    });
    Ok(())
}

/// 收藏切换（对齐 toggleFavorite：乐观更新 + 收藏视图内取消收藏即时移除）。
/// 行级动作按行号作用：悬停行与选中行的按钮、键盘路径（选中行）共用
pub fn toggle_favorite(app: &App, index: usize) {
    if app.state.favorite_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(item) = app.state.search_item_at(index) else {
        app.state.favorite_pending.store(false, Ordering::SeqCst);
        return;
    };
    let target_id = item.id.clone();
    let next = !item.is_favorited;
    let result = ClipService::set_favorited(app.core(), &target_id, next);
    app.state.favorite_pending.store(false, Ordering::SeqCst);

    match result {
        Err(error) => {
            warn!("更新收藏状态失败: {error}");
            crate::tray::notify_failure("更新收藏失败，请稍后重试");
        }
        Ok(()) => {
            let mut items = app.state.search_items();
            items
                .iter_mut()
                .filter(|item| item.id == target_id)
                .for_each(|item| item.is_favorited = next);

            let favorite_view_removed = !next && QUERY.with(|state| state.borrow().favorited_only);
            if favorite_view_removed {
                // 收藏视图内取消收藏：从当前页移除、总数回补、选中重锚
                items.retain(|item| item.id != target_id);
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

/// Del 或删除按钮：首次进入待确认态（3 秒后自动解除），再次触发执行。
/// 行级动作按行号作用：待确认以条目 id 为准（列表位移不错删），
/// UI 标记记行号，只落在发起行
pub fn request_delete(app: &App, index: usize) {
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    let id = item.id.clone();
    let armed = ARMED_DELETE.with(|slot| slot.borrow().as_deref() == Some(id.as_str()));
    if armed {
        perform_delete(app, &id);
        return;
    }
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = Some(id.clone()));
    app.with_search(|win| win.set_delete_armed_row(index as i32));
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
        app_cb.with_search(|win| win.set_delete_armed_row(-1));
    });
}

fn reset_delete_arm(app: &App) {
    DELETE_TOKEN.with(|value| value.set(value.get() + 1));
    ARMED_DELETE.with(|slot| *slot.borrow_mut() = None);
    app.with_search(|win| win.set_delete_armed_row(-1));
}

fn perform_delete(app: &App, id: &str) {
    reset_delete_arm(app);
    if let Err(error) = ClipService::delete(app.core(), id) {
        warn!("删除条目失败: {error}");
        crate::tray::notify_failure("删除条目失败，请稍后重试");
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
/// content-height，0ms 定时器读数可能先于渲染帧，拿到的是布局未跟上的
/// 陈旧值，按陈旧值伸缩会让窗口高度与内容脱节
pub fn sync_height() {
    ALLOW_SHRINK.with(|flag| flag.set(true));
}

/// 列表内容实测高变化（list-stack 自然高由布局 pass 写入）：布局真值
/// 信号，触发一次读数应用。日常滚动等无高度净变化的触发在 apply_height
/// 内被 target==last 拦下
pub fn heights_changed(app: &App) {
    apply_height(app);
}

/// 读数并应用目标高度：target = min(max, slack + chrome + 列表内容高)。
/// 顶边锚定伸缩（左上不动、底边伸缩），开窗后任何高度变化都不重定位，
/// 上边沿保持 finish_show 居中落位。ALLOW_SHRINK 由结构性变化的
/// sync_height() 置位、应用后消费（棘轮）：加载态空列表的中间高度低于
/// 开窗起步，无收缩许可被拦下，等内容落地一次到位
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
    tracing::debug!(
        "高度读数: target={target} last={last} loading={} rows={} natural={:.0} chrome={:.0}",
        win.get_loading(),
        win.get_rows().row_count(),
        win.get_list_content_height(),
        win.get_chrome_height()
    );
    if target == last {
        return;
    }
    let allow = ALLOW_SHRINK.with(|flag| flag.get());
    ALLOW_SHRINK.with(|flag| flag.set(false));
    if !allow && target < last {
        tracing::debug!("高度同步跳过（无收缩许可）: target={target} last={last}");
        return;
    }
    tracing::debug!(
        "高度应用: {last} -> {target} (loading={} rows={})",
        win.get_loading(),
        win.get_rows().row_count()
    );
    LAST_HEIGHT.with(|value| value.set(target as f32));
    // 开窗基线只记「默认视图」落地高度：筛选空态/少结果/关键词命中的
    // 收缩态高度不作为下次开窗起步（会话重置后必然回到默认视图）。
    // 判定读 QUERY（防抖后生效的查询词）——落地必发生在查询生效后，
    // 与本次结果行同源
    let default_view = QUERY.with(|state| {
        let state = state.borrow();
        state.keyword.trim().is_empty()
            && state.filter == 0
            && !state.favorited_only
            && state.text_kind == 0
            && state.tags.is_empty()
    });
    if default_view && !win.get_loading() && win.get_rows().row_count() > 0 {
        OPEN_BASE_HEIGHT.with(|value| value.set(target as f32));
        tracing::debug!("开窗基线更新: {target}");
    }
    let width_px = (geo.get_window_width() * scale).round() as i32;
    win.window().set_size(PhysicalSize::new(
        width_px.max(1) as u32,
        target.max(1) as u32,
    ));
}

/* ───────────────── 窗口拖拽 ───────────────── */

pub fn drag_started(app: &App) {
    tooltip::cancel(app);
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
pub fn edit_requested(app: &App, index: usize) {
    let Some(item) = app.state.search_item_at(index) else {
        return;
    };
    crate::editor::open_from_search(app, item.id.clone());
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
/// 0=无 1=向上 2=向下 3=上屏 4=次级上屏（纯文本或路径） 5=编辑 6=收藏 7=关闭 8=删除
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
        win.on_text_kind_selected(move |kind| {
            text_kind_selected(&app_cb, kind);
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
    // 行级按钮动作：一律按发起行号作用（悬停行与选中行同款按钮）
    {
        let app_cb = app.clone();
        win.on_action_paste(move |index| {
            paste_index(&app_cb, index.max(0) as usize, false);
        });
    }
    // ── 「更多上屏方式」菜单 ──
    {
        let app_cb = app.clone();
        win.on_action_menu_requested(move |row, x, y| {
            open_actions_menu(&app_cb, row.max(0) as usize, x, y);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_menu_move(move |delta| {
            move_actions_menu(&app_cb, delta);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_menu_confirm(move || {
            confirm_actions_menu(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_menu_dismiss(move || {
            close_actions_menu(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_menu_paste(move |action_index| {
            let row = app_cb
                .with_search(|win| win.get_actions_menu_row())
                .unwrap_or(-1);
            if row < 0 {
                return;
            }
            close_actions_menu(&app_cb);
            paste_menu_action(&app_cb, row as usize, action_index.max(0) as usize);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_edit(move |index| {
            edit_requested(&app_cb, index.max(0) as usize);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_toggle_favorite(move |index| {
            toggle_favorite(&app_cb, index.max(0) as usize);
        });
    }
    {
        let app_cb = app.clone();
        win.on_action_delete(move |index| {
            request_delete(&app_cb, index.max(0) as usize);
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
        win.on_key_paste(move |secondary| {
            paste_selected(&app_cb, secondary);
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_edit(move || {
            edit_requested(&app_cb, selected_index(&app_cb));
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_toggle_favorite(move || {
            toggle_favorite(&app_cb, selected_index(&app_cb));
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_delete(move || {
            request_delete(&app_cb, selected_index(&app_cb));
        });
    }
    {
        let app_cb = app.clone();
        win.on_key_close(move || {
            // Esc 是键盘路径，关闭时前台在搜索窗自己手上，停屏后键盘焦点
            // 须显式还给会话目标（对齐旧壳 hideSearch 恒走 restore_target）；
            // 失焦自关不经此回调——用户点击处已持有前台，无需抢回
            hide(&app_cb, true);
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

    #[test]
    fn navigation_wraps_in_both_directions() {
        assert_eq!(next_navigation_index(5, 0, true), 4);
        assert_eq!(next_navigation_index(5, 4, true), 3);
        assert_eq!(next_navigation_index(5, 4, false), 0);
        assert_eq!(next_navigation_index(1, 0, true), 0);
    }

    #[test]
    fn query_maps_filter_and_sort() {
        let query = build_query("  关键词 ", 3, 0, &["工作".to_string()], 50, false);
        assert_eq!(query.keyword, "  关键词 ");
        assert_eq!(query.filters.clip_type, Some(ClipType::Image));
        assert_eq!(query.filters.text_kind, None);
        assert_eq!(
            query.filters.tag_names.as_deref(),
            Some(["工作".to_string()].as_slice())
        );
        assert!(matches!(query.sort, SearchSort::RelevanceDesc));
        assert_eq!(query.offset, 50);
        assert_eq!(query.limit, PAGE_SIZE);

        let query = build_query("", 0, 0, &[], 0, true);
        assert!(matches!(query.sort, SearchSort::RecentDesc));
        assert_eq!(query.filters.favorited_only, Some(true));
    }

    #[test]
    fn query_maps_text_kind_sub_filter_only_under_text_filter() {
        let query = build_query("", 2, 2, &[], 0, false);
        assert_eq!(query.filters.clip_type, Some(ClipType::Text));
        assert_eq!(query.filters.text_kind, Some(TextKind::Link));

        // 用途细分只在文本筛选下参与查询；索引 0 与越界值都是不筛选
        let query = build_query("", 2, 0, &[], 0, false);
        assert_eq!(query.filters.clip_type, Some(ClipType::Text));
        assert_eq!(query.filters.text_kind, None);

        let query = build_query("", 0, 2, &[], 0, false);
        assert_eq!(query.filters.clip_type, None);
        assert_eq!(query.filters.text_kind, None);
    }

    #[test]
    fn favorites_toggle_preserves_type_kind_and_combines_with_tags() {
        let mut state = super::QueryState::default();
        state.select_filter(2);
        state.text_kind = 2;
        state.tags.push("工作".into());
        state.select_filter(1);
        let query = build_query(
            "关键词",
            state.filter,
            state.text_kind,
            &state.tags,
            50,
            state.favorited_only,
        );
        assert_eq!(query.filters.favorited_only, Some(true));
        assert_eq!(query.filters.clip_type, Some(ClipType::Text));
        assert_eq!(query.filters.text_kind, Some(TextKind::Link));
        assert_eq!(query.filters.tag_names, Some(vec!["工作".into()]));
        assert_eq!(query.offset, 50);
        state.select_filter(1);
        assert!(!state.favorited_only);
        assert_eq!(state.text_kind, 2);
        state.select_filter(1);
        state.select_filter(3);
        assert!(state.favorited_only);
        assert_eq!(state.filter, 3);
        assert_eq!(state.text_kind, 0);
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
        assert_eq!(
            resolve_session_action(&settings, "\u{a}", false, false, false, false),
            3
        );
        assert_eq!(
            resolve_session_action(&settings, "\u{a}", false, false, true, false),
            4
        );
        assert_eq!(
            resolve_session_action(&settings, "\u{a}", true, false, false, false),
            5
        );
        assert_eq!(
            resolve_session_action(&settings, "\u{1b}", false, false, false, false),
            7
        );
        assert_eq!(
            resolve_session_action(&settings, "\u{F700}", false, false, false, false),
            1
        );
        assert_eq!(
            resolve_session_action(&settings, "\u{F701}", false, false, false, false),
            2
        );
        assert_eq!(
            resolve_session_action(&settings, "\u{20}", true, false, false, false),
            6
        );
        assert_eq!(
            resolve_session_action(&settings, "\u{7f}", false, false, false, false),
            8
        );
        // 未配置组合放行（0），修饰键集合不一致不命中
        assert_eq!(
            resolve_session_action(&settings, "\u{20}", false, false, false, false),
            0
        );
        assert_eq!(
            resolve_session_action(&settings, "x", false, false, false, false),
            0
        );
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
        assert_eq!(
            resolve_session_action(&settings, "k", true, false, false, false),
            3
        );
        assert_eq!(
            resolve_session_action(&settings, "q", false, false, false, false),
            7
        );
        // 旧默认键位不再命中
        assert_eq!(
            resolve_session_action(&settings, "\u{1b}", false, false, false, false),
            0
        );
    }

    #[test]
    fn event_key_name_rejects_multi_char_text() {
        assert_eq!(event_key_name("ab"), None);
        assert_eq!(event_key_name("剪"), None);
        assert_eq!(event_key_name("\u{a}"), Some("Enter".to_string()));
    }
}

/* ───────────────── L2 无头 UI 测试 ───────────────── */

/// 测试临时目录回收：Drop 时整体删除（覆盖 panic unwind 路径）
#[cfg(test)]
struct TempDirGuard(std::path::PathBuf);

#[cfg(test)]
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
pub(crate) mod scroll_reset_tests {
    //! 搜索列表回顶契约（docs/testing.md L2 层）：深滚动位上发起任何
    //! 新查询，视口必须回顶——Flickable 不随内容收缩钳制旧偏移，空
    //! 态/短结果会整体落在视口外。临时 SQLite 预置数据，不经系统剪
    //! 贴板。由 l2_headless_ui_contracts 单线程串行调用（独立挂
    //! #[test] 会与其他 UI 测试抢 Slint 平台线程绑定）。

    use super::*;
    use std::sync::Arc;

    use floatpaste_core::repository::sqlite_repository::SqliteRepository;
    use floatpaste_core::services::image_storage::ImageStorage;
    use floatpaste_core::services::normalize_service::NormalizeService;
    use floatpaste_core::state::CoreState;

    use crate::app_state::SharedState;

    /// 单测试函数串行多段中的一段（见模块注释）
    pub(crate) fn run() {
        // 临时库预置文本条目：图片筛选下恒为空 → 空态可稳定复现
        let stamp = std::process::id();
        let dir = std::env::temp_dir().join(format!("floatpaste-scroll-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("floatpaste.db");
        {
            let repository = SqliteRepository::new(&db_path).unwrap();
            for index in 0..10 {
                let item =
                    NormalizeService::normalize_text(&format!("滚动行 {index}"), None).unwrap();
                repository.save_text_item(&item).unwrap();
            }
        }
        let _cleanup = TempDirGuard(dir.clone());

        // 装配：真实 core（临时库）+ 仅搜索窗接线的 App
        let win = SearchWindow::new().expect("实例化搜索窗失败");
        win.window().set_size(slint::LogicalSize::new(780.0, 620.0));
        let repository = SqliteRepository::new(&db_path).unwrap();
        let core = CoreState::new(
            repository,
            ImageStorage::new(dir).unwrap(),
            UserSetting::default(),
        );
        let app = App {
            state: Arc::new(SharedState::new(core)),
            picker: Default::default(),
            tooltip: Default::default(),
            tray_menu: Default::default(),
            search: win.as_weak(),
            editor: Default::default(),
            settings: Default::default(),
        };

        // 手工注入行模型，等价一次查询落地后的满列表（60 行必超一屏）
        let rows: Vec<SearchRow> = (0..60)
            .map(|index| SearchRow {
                id: format!("row-{index}").into(),
                preview: format!("条目 {index}").into(),
                kind: 0,
                has_html: false,
                source_app: "".into(),
                time_text: "".into(),
                meta: "".into(),
                favorited: false,
                has_thumb: false,
                thumb: Default::default(),
                tags: Default::default(),
                tag_more: 0,
                actions: Default::default(),
                secondary_label: "".into(),
            })
            .collect();
        win.set_rows(ModelRc::new(Rc::new(VecModel::from(rows))));
        win.set_loading(false);
        // 60 行（行高 ≥44px）内容远超 620px 视口，深滚动位真实可达。
        // 不在此断言 content-height：无事件循环时布局 pass 未跑，读数
        // 停留在注入前的空内容布局（陈旧读数坑见 docs/testing.md）

        // 深滚动位（等价滚动条拖到列表深处；Flickable 偏移赋值即生效）
        win.invoke_set_list_scroll_y(-1200.0);
        assert!(
            win.get_list_scroll_y() < 0.0,
            "程序性深滚动应生效，视口在内容深处"
        );

        // 发起筛选查询：回顶信号同步递增（changed 守卫由事件循环调度，
        // 生产环境循环常驻、一帧内生效）
        let generation_before = win.get_scroll_generation();
        filter_selected(&app, 3); // 图片筛选，库中全是文本 → 必空
        assert_eq!(
            win.get_scroll_generation(),
            generation_before + 1,
            "新查询发起应递增回顶信号"
        );

        // 泵事件循环：消费回顶守卫（content-y=0）与查询落地（行集清空、
        // 空态为「清除筛选」）
        let closer = thread::spawn(|| {
            thread::sleep(Duration::from_millis(1000));
            if let Err(error) = slint::quit_event_loop() {
                eprintln!("quit_event_loop 失败: {error:?}");
            }
        });
        let _ = slint::run_event_loop_until_quit();
        closer.join().unwrap();

        assert_eq!(win.get_rows().row_count(), 0, "图片筛选下应无结果");
        assert_eq!(win.get_empty_action(), 2, "空态动作应为「清除筛选」");
        assert_eq!(
            win.get_list_scroll_y(),
            0.0,
            "深滚动位上的空态视口必须回顶（bug 现场：清除筛选按钮不可见）"
        );

        let link = NormalizeService::normalize_text("https://example.com/favorite", None).unwrap();
        let link_id = app.core().repository.save_text_item(&link).unwrap().id;
        app.core().repository.set_favorited(&link_id, true).unwrap();
        filter_selected(&app, 2);
        text_kind_selected(&app, 2);
        filter_selected(&app, 1);
        let closer = thread::spawn(|| {
            thread::sleep(Duration::from_millis(1000));
            slint::quit_event_loop().unwrap();
        });
        let _ = slint::run_event_loop_until_quit();
        closer.join().unwrap();
        assert!(win.get_favorited_only());
        assert_eq!(win.get_active_filter(), 2);
        assert_eq!(win.get_active_text_kind(), 2);
        assert_eq!(win.get_rows().row_count(), 1);
        assert_eq!(win.get_rows().row_data(0).unwrap().id.as_str(), link_id);
        toggle_favorite(&app, 0);
        assert_eq!(
            win.get_rows().row_count(),
            0,
            "组合收藏视图中取消收藏应立即移除条目"
        );
    }
}

#[cfg(test)]
pub(crate) mod refresh_after_editor_tests {
    //! 编辑后刷新契约（docs/testing.md L2 层）：编辑器内改标签经
    //! refresh_after_editor 落到搜索窗——列表行与标签筛选 chips 都必须
    //! 反映库中最新标签（bug 现场：chips 仅在会话打开时加载，新增标签
    //! 要重开搜索窗才可见）。临时 SQLite 预置数据，不经系统剪贴板。由
    //! l2_headless_ui_contracts 单线程串行调用（独立挂 #[test] 会与其他
    //! UI 测试抢 Slint 平台线程绑定）。

    use super::*;
    use std::sync::Arc;

    use floatpaste_core::repository::sqlite_repository::SqliteRepository;
    use floatpaste_core::services::image_storage::ImageStorage;
    use floatpaste_core::services::normalize_service::NormalizeService;
    use floatpaste_core::services::tag_service::TagService;
    use floatpaste_core::state::CoreState;

    use crate::app_state::SharedState;

    /// 单测试函数串行多段中的一段（见模块注释）
    pub(crate) fn run() {
        let stamp = std::process::id();
        let dir = std::env::temp_dir().join(format!("floatpaste-editor-refresh-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("floatpaste.db");
        let item_id = {
            let repository = SqliteRepository::new(&db_path).unwrap();
            let item = NormalizeService::normalize_text("编辑后刷新用条目", None).unwrap();
            repository.save_text_item(&item).unwrap().id
        };
        let _cleanup = TempDirGuard(dir.clone());

        let win = SearchWindow::new().expect("实例化搜索窗失败");
        win.window().set_size(slint::LogicalSize::new(780.0, 620.0));
        let editor_win = crate::EditorWindow::new().expect("实例化编辑窗失败");
        let repository = SqliteRepository::new(&db_path).unwrap();
        let core = CoreState::new(
            repository,
            ImageStorage::new(dir).unwrap(),
            UserSetting::default(),
        );
        let app = App {
            state: Arc::new(SharedState::new(core)),
            picker: Default::default(),
            tooltip: Default::default(),
            tray_menu: Default::default(),
            search: win.as_weak(),
            editor: editor_win.as_weak(),
            settings: Default::default(),
        };

        // 串行前段（scroll_reset_tests）在 thread_local QUERY 留下筛选
        // 状态；等价新会话打开前的重置，保证本段从干净查询态起步
        reset_session_state(&app);

        // 提示随设置更新，不能继续显示编译时的默认键位。
        let mut settings = app.state.current_settings();
        settings.session_keys.confirm = "Alt+Enter".into();
        settings.session_keys.dismiss = "Ctrl+Escape".into();
        app.core().update_settings(settings).unwrap();
        refresh_copy(&app);
        assert_eq!(win.get_action_keys().confirm, "Alt+Enter");
        assert_eq!(win.get_action_keys().dismiss, "Ctrl+Escape");

        // 前置等价 bug 现场：搜索窗已建而 chips 未加载（编辑器路径不
        // 经过会话打开，无从加载）
        assert_eq!(win.get_tag_chips().row_count(), 0, "前置：chips 为空");

        // 编辑器内给条目加标签（生产路径：commit_tags → refresh_lists）
        TagService::set_item_tags(&app.core(), &item_id, &["新标签".to_string()]).unwrap();
        refresh_after_editor(&app);

        // chips 同步重建，新标签无需重开窗口即可见
        let chips: Vec<String> = win
            .get_tag_chips()
            .iter()
            .map(|chip| chip.name.to_string())
            .collect();
        assert_eq!(
            chips,
            vec!["新标签".to_string()],
            "编辑后新标签应出现在筛选 chips"
        );

        // 泵事件循环消费查询落地：行卡同步带出新标签
        let closer = thread::spawn(|| {
            thread::sleep(Duration::from_millis(1000));
            if let Err(error) = slint::quit_event_loop() {
                eprintln!("quit_event_loop 失败: {error:?}");
            }
        });
        let _ = slint::run_event_loop_until_quit();
        closer.join().unwrap();

        assert_eq!(win.get_rows().row_count(), 1, "列表应重查到该条目");
        let row = win.get_rows().row_data(0).expect("首行应存在");
        assert_eq!(
            row.tags
                .iter()
                .map(|tag| tag.to_string())
                .collect::<Vec<_>>(),
            vec!["新标签".to_string()],
            "行卡应显示新标签"
        );
        crate::editor::load_error_tests::run(&app, &editor_win, &db_path, &item_id);
    }
}

#[cfg(test)]
pub(crate) mod action_menu_tests {
    //! 「更多上屏方式」菜单契约（docs/testing.md L2 层）：行级动作目录
    //! 投影、菜单开合、键盘高亮与跨行 Delete 路由，不执行真实上屏
    //! （系统剪贴板属 L3 串行资源）。由 l2_headless_ui_contracts 单线程串行调用。

    use super::*;
    use std::sync::Arc;

    use floatpaste_core::repository::sqlite_repository::SqliteRepository;
    use floatpaste_core::services::image_storage::ImageStorage;
    use floatpaste_core::state::CoreState;

    use crate::app_state::SharedState;

    fn headless_app() -> (App, SearchWindow) {
        let stamp = std::process::id();
        let dir = std::env::temp_dir().join(format!("floatpaste-menu-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("floatpaste.db");
        let repository = SqliteRepository::new(&db_path).unwrap();
        let core = CoreState::new(
            repository,
            ImageStorage::new(dir).unwrap(),
            UserSetting::default(),
        );
        let win = SearchWindow::new().expect("实例化搜索窗失败");
        win.window().set_size(slint::LogicalSize::new(780.0, 620.0));
        let app = App {
            state: Arc::new(SharedState::new(core)),
            picker: Default::default(),
            tooltip: Default::default(),
            tray_menu: Default::default(),
            search: win.as_weak(),
            editor: Default::default(),
            settings: Default::default(),
        };
        (app, win)
    }

    fn summary(clip_type: &str, has_html: bool) -> ClipItemSummary {
        ClipItemSummary {
            has_html,
            id: format!("{clip_type}-{has_html}"),
            r#type: clip_type.to_string(),
            content_preview: "p".into(),
            source_app: None,
            is_favorited: false,
            file_count: 0,
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

    fn row_labels(row: &SearchRow) -> Vec<String> {
        row.actions
            .iter()
            .map(|action| action.label.to_string())
            .collect()
    }

    /// 单测试函数串行多段中的一段（见模块注释）
    pub(crate) fn run() {
        let keys = UserSetting::default().session_keys;
        let defaults = SecondaryPasteDefaults::default();

        // 行投影：目录按类型展开，普通文本不凑「纯文本」选项，
        // 次级动作提示名跟随默认配置
        let plain = make_row(&summary("text", false), &keys, &defaults);
        assert_eq!(plain.actions.row_count(), 1);
        assert_eq!(row_labels(&plain), vec!["粘贴为文本"]);
        assert_eq!(plain.secondary_label, "", "普通文本无独立次级动作提示");

        let rich = make_row(&summary("text", true), &keys, &defaults);
        assert_eq!(row_labels(&rich), vec!["粘贴为原格式", "粘贴为纯文本"]);
        assert_eq!(rich.secondary_label, "粘贴为纯文本");

        let image = make_row(&summary("image", false), &keys, &defaults);
        assert_eq!(
            row_labels(&image),
            vec!["粘贴为图片", "粘贴为图片路径", "粘贴为图片文件"]
        );
        assert_eq!(image.secondary_label, "粘贴为图片路径");

        let file = make_row(&summary("file", false), &keys, &defaults);
        assert_eq!(
            row_labels(&file),
            vec!["粘贴为文件", "粘贴为完整路径", "粘贴为文件名"]
        );
        assert_eq!(file.secondary_label, "粘贴为完整路径");

        // 改配文件名后：次级动作提示换名，快捷键提示跟随新默认
        let file_named = make_row(
            &summary("file", false),
            &keys,
            &SecondaryPasteDefaults {
                file: PasteMode::FileNameText,
                ..defaults
            },
        );
        assert_eq!(file_named.secondary_label, "粘贴为文件名");
        assert_eq!(
            row_labels(&file_named),
            vec!["粘贴为文件", "粘贴为完整路径", "粘贴为文件名"]
        );
        let shortcuts: Vec<String> = (0..file_named.actions.row_count())
            .filter_map(|index| {
                file_named
                    .actions
                    .row_data(index)
                    .map(|action| action.shortcut.to_string())
            })
            .collect();
        assert_eq!(
            shortcuts,
            vec![
                keys.confirm.clone(),
                String::new(),
                keys.confirm_as_file.clone()
            ],
            "次级键提示应跟随新默认（文件名），主动作恒带主键提示"
        );

        // 配置等同主动作（富文本选原格式）：不重复提示，菜单仍完整
        let rich_original = make_row(
            &summary("text", true),
            &keys,
            &SecondaryPasteDefaults {
                rich_text: PasteMode::Original,
                ..defaults
            },
        );
        assert_eq!(rich_original.secondary_label, "");
        assert_eq!(rich_original.actions.row_count(), 2);

        // 菜单状态流转（不执行真实上屏）：直接注入 summary，
        // 等价一次查询落地后的行缓存
        let (app, win) = headless_app();
        app.state
            .set_search_items(vec![summary("text", false), summary("file", false)]);

        // 普通文本（仅一项动作）：不打开菜单
        open_actions_menu(&app, 0, 10.0, 100.0);
        assert_eq!(win.get_actions_menu_row(), -1, "单项动作不开菜单");

        // 文件条目：打开后高亮回主动作，锚点入属性；键盘循环移动
        set_selected(&app, 0);
        request_delete(&app, 0);
        assert_eq!(win.get_delete_armed_row(), 0);
        open_actions_menu(&app, 1, 400.0, 80.0);
        assert_eq!(win.get_selected(), 1, "菜单应选中发起行");
        assert_eq!(
            SELECTED_ID.with(|slot| slot.borrow().clone()),
            Some(summary("file", false).id)
        );
        assert_eq!(win.get_delete_armed_row(), -1, "打开菜单应解除删除待确认");
        assert!(ARMED_DELETE.with(|slot| slot.borrow().is_none()));
        assert_eq!(win.get_actions_menu_row(), 1);
        assert_eq!(win.get_actions_menu_highlight(), 0);
        assert_eq!(win.get_actions_menu_x(), 400.0);
        move_actions_menu(&app, 1);
        assert_eq!(win.get_actions_menu_highlight(), 1);
        move_actions_menu(&app, 1);
        assert_eq!(win.get_actions_menu_highlight(), 2);
        move_actions_menu(&app, 1);
        assert_eq!(win.get_actions_menu_highlight(), 0, "高亮循环回主动作");
        move_actions_menu(&app, -1);
        assert_eq!(win.get_actions_menu_highlight(), 2, "向上移动循环");

        // 收起
        close_actions_menu(&app);
        assert_eq!(win.get_actions_menu_row(), -1);

        // 贴近窗口底边：菜单锚点向上翻（避免画出窗外）
        open_actions_menu(&app, 1, 10.0, 600.0);
        assert!(
            win.get_actions_menu_y() < 600.0 - 3.0 * 30.0,
            "底边附近的菜单应向上展开"
        );
        close_actions_menu(&app);

        // 从 A 的删除待确认态打开 B 菜单，再按 Delete：只让 B 进入待确认。
        wire(&app);
        set_selected(&app, 0);
        request_delete(&app, 0);
        win.invoke_action_menu_requested(1, 400.0, 80.0);
        win.invoke_blur_input();
        win.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Delete.into(),
            });
        win.window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased {
                text: slint::platform::Key::Delete.into(),
            });
        assert_eq!(win.get_actions_menu_row(), -1, "Delete 应先收菜单");
        assert_eq!(win.get_delete_armed_row(), 1, "Delete 应作用于菜单发起行");
        assert_eq!(
            ARMED_DELETE.with(|slot| slot.borrow().clone()),
            Some(summary("file", false).id)
        );
        assert_eq!(
            app.state.search_items().len(),
            2,
            "首次 Delete 不删除任何行"
        );
        reset_delete_arm(&app);
        pointer_split_button(&app, &win);
    }

    fn pointer_split_button(app: &App, win: &SearchWindow) {
        use slint::platform::{PointerEventButton, WindowEvent};

        let mut item = summary("file", false);
        item.content_preview = "D:\\文档\\项目设计说明.pdf".into();
        app.state
            .set_search_items(vec![summary("text", true), item]);
        build_rows(app);
        win.set_result_count(2);
        win.set_result_total(2);
        set_selected(app, 1);
        let _ = win.window().take_snapshot().unwrap();

        let hint = Rc::new(RefCell::new(None));
        let hint_cb = hint.clone();
        win.on_button_hint(move |text, x, y| {
            *hint_cb.borrow_mut() = Some((text.to_string(), x, y))
        });
        let hovered = Rc::new(Cell::new(-1));
        let hovered_cb = hovered.clone();
        win.on_row_hover(move |row, _, _| hovered_cb.set(row));
        let mut bounds = (i32::MAX, 0);
        for y in 100..380 {
            hovered.set(-1);
            win.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(100.0, y as f32),
            });
            if hovered.get() == 1 {
                bounds.0 = bounds.0.min(y);
                bounds.1 = y;
            }
        }
        assert!(bounds.0 < bounds.1, "应找到文件行");
        let y = (bounds.0 + bounds.1) as f32 / 2.0;
        let (mut main, mut more) = (None, None);
        for x in 400..760 {
            win.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(x as f32, y),
            });
            if let Some((text, x, y)) = hint.borrow_mut().take() {
                if text.starts_with("更多上屏方式") {
                    more = Some(slint::LogicalPosition::new(x + 11.0, y - 17.0));
                } else if text.starts_with("粘贴为文件") {
                    main = Some(slint::LogicalPosition::new(x + 34.0, y - 17.0));
                }
            }
        }

        // 截获发布回调以避免写系统剪贴板，其余菜单路由使用真实 wire。
        let pasted = Rc::new(Cell::new(-1));
        let pasted_cb = pasted.clone();
        win.on_action_paste(move |row| pasted_cb.set(row));
        let clicked = Rc::new(Cell::new(false));
        let clicked_cb = clicked.clone();
        win.on_row_clicked(move |_| clicked_cb.set(true));
        for (position, dropdown) in [
            (main.expect("应找到直接粘贴区"), false),
            (more.expect("应找到下拉区"), true),
        ] {
            pasted.set(-1);
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
            assert!(!clicked.get(), "复合按钮不能触发行点击");
            assert_eq!(pasted.get(), if dropdown { -1 } else { 1 });
            assert_eq!(win.get_actions_menu_row(), if dropdown { 1 } else { -1 });
        }

        if std::env::var_os("FLOATPASTE_UI_SNAPSHOTS").is_some() {
            let dir =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.artifacts/snapshot");
            std::fs::create_dir_all(&dir).unwrap();
            for (name, theme) in [
                ("light", floatpaste_core::theme::ResolvedTheme::Light),
                ("dark", floatpaste_core::theme::ResolvedTheme::Dark),
            ] {
                let tokens = floatpaste_core::theme::derive_tokens("default", "", theme);
                crate::theme_bridge::reapply_theme(app, &tokens);
                for (state, menu) in [("split", false), ("menu", true)] {
                    close_actions_menu(app);
                    let _ = win.window().take_snapshot().unwrap();
                    if menu {
                        slint::platform::update_timers_and_animations();
                        open_actions_menu(app, 1, 500.0, y + 22.0);
                        let _ = win.window().take_snapshot().unwrap();
                        std::thread::sleep(Duration::from_millis(180));
                        slint::platform::update_timers_and_animations();
                    }
                    let pixels = win.window().take_snapshot().unwrap();
                    image::save_buffer(
                        dir.join(format!(
                            "paste-search-{state}-{name}-{}.png",
                            (win.window().scale_factor() * 100.0).round() as u32
                        )),
                        pixels.as_bytes(),
                        pixels.width(),
                        pixels.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
        close_actions_menu(app);
    }
}
