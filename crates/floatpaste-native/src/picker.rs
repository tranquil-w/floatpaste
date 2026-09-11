//! 速贴面板会话逻辑：显示/隐藏/切换、列表刷新、确认上屏、收藏、导航。
//!
//! 行为逐项对齐原版 WindowCoordinator / ShortcutManager / PickerShell：
//! - 显示不抢焦点（WS_EX_NOACTIVATE + show），并把前台还给目标窗口；
//! - 会话期键盘由 LL 钩子接管（长按连发在 core::session_keyboard）；
//! - 外击关闭经 WH_MOUSE_LL 钩子；
//! - 尺寸/位置持久化与三种定位模式在 core::PickerPositionService。

use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tracing::{info, warn};

use floatpaste_core::domain::clip_item::{ClipItemSummary, PasteOption};
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::{mouse_monitor, session_keyboard, window_control};
use floatpaste_core::services::clip_service::ClipService;
use floatpaste_core::services::picker_position_service::{
    PickerPositionService, WindowGeometry, PICKER_DEFAULT_HEIGHT, PICKER_DEFAULT_WIDTH,
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

/// 打开速贴面板（对齐 WindowCoordinator::activate_picker）
pub fn activate(app: &App) {
    let Some(win) = app.picker.upgrade() else {
        return;
    };

    let settings = app.state.refresh_settings();
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
            apply_window_position(app, &settings, None);
            return;
        }
        warn!("Picker 标志位为激活但窗口实际不可见，重置状态后重新显示");
        app.state.end_picker_activation();
        session_keyboard::end_session();
        mouse_monitor::end_session();
    }

    // 恢复记忆尺寸（无记忆则用默认）
    let (width, height) = PickerPositionService::resolve_window_size(&app.core().repository)
        .ok()
        .flatten()
        .unwrap_or((PICKER_DEFAULT_WIDTH, PICKER_DEFAULT_HEIGHT));
    win.window()
        .set_size(slint::PhysicalSize::new(width, height));
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

    apply_window_position(app, &settings, session.target_window_hwnd);

    // 主题随设置刷新（设置可能在后台被改变）
    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
    theme_bridge::apply_theme(
        &win,
        app.tooltip.upgrade().as_ref(),
        app.search.upgrade().as_ref(),
        &tokens,
    );

    app.state.begin_picker_activation();

    begin_input_session(app, hwnd, settings.picker_digit_shortcuts_enabled);

    // 显示（无激活 + 置顶，对齐原版 always_on_top）。winit show 的异步
    // 样式重置与前台抢占统一交 overlay 公共层处理：立即重挂浮层样式并
    // 把前台还给目标窗口，50ms 后兜底复查（winit 的激活若晚于归还落地，
    // 兜底会把前台请回来）
    let _ = win.window().show();
    let immediate = session
        .target_window_hwnd
        .map_or(ForegroundPolicy::Keep, ForegroundPolicy::Restore);
    let deferred = session
        .target_window_hwnd
        .map_or(ForegroundPolicy::Keep, ForegroundPolicy::RestoreIfStolen);
    overlay::after_show(hwnd, false, immediate, deferred);

    // 搜索窗口正被作为回贴目标（用户在搜索中按了主快捷键）：键盘交给
    // 速贴会话，搜索侧输入框失焦、底栏切换为速贴键位（对齐
    // SEARCH_INPUT_SUSPEND_EVENT 语义）
    if session.target_window_hwnd == Some(app.state.search_hwnd.load(Ordering::SeqCst)) {
        search::suspend_input(app);
    }

    // 会话开始：刷新列表、选中归零、滚回顶部、清空消息
    refresh_list_reset(app, &settings);
}

/// 隐藏（对齐 WindowCoordinator::hide_picker + hide_picker_and_restore_target）
pub fn hide(app: &App, restore_target: bool) {
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

    if win.window().is_visible() {
        let _ = win.window().hide();
    }

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

/// 主快捷键命中：活跃则关闭并恢复目标，否则打开。
/// 搜索窗口活跃时先无还原收起（对齐旧版 toggle_picker_from_shortcut，
/// 避免两窗口争抢焦点导致闪烁）
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

fn begin_input_session(app: &App, hwnd: isize, digit_shortcuts_enabled: bool) {
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
        digit_shortcuts_enabled,
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
        return;
    }
    match action {
        SessionAction::NavigateUp => navigate(app, true),
        SessionAction::NavigateDown => navigate(app, false),
        SessionAction::Confirm => confirm(app, current_index(app), false),
        SessionAction::ConfirmAsFile => confirm(app, current_index(app), true),
        SessionAction::Dismiss => hide(app, true),
        SessionAction::ToggleFavorite => toggle_favorite(app),
        SessionAction::OpenEditor => {
            // 编辑器窗口在后续迁移阶段接入；保持面板可见避免"按键没反应"
            warn!("编辑器窗口尚未迁移，Ctrl+Enter 暂不处理");
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
            app.with_picker(|win| win.set_load_failed(true));
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
    let app_for_cb = app.clone();
    std::thread::spawn(move || {
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
            let items = app_for_cb.state.items();
            app_for_cb.with_picker(|win| build_rows(&win, &items));
        });
    });
}

/* ───────────────── 会话动作 ───────────────── */

/// 确认上屏（对齐 PickerShell.confirmSelection：Shift+Enter 仅对图片以文件形式）
pub fn confirm(app: &App, index: usize, as_file: bool) {
    let Some(item) = app.state.item_at(index) else {
        return;
    };

    tooltip::cancel(app);

    let settings = app.state.current_settings();
    let is_image = item.r#type == "image";
    let option = PasteOption {
        restore_clipboard_after_paste: settings.restore_clipboard_after_paste,
        paste_to_target: true,
        as_file: as_file && is_image,
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
    let next = if up {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    };
    set_selected(app, next);
}

fn set_message(app: &App, text: &str, tone: i32) {
    app.with_picker(|win| {
        win.set_message_text(text.into());
        win.set_message_tone(tone);
    });
}

/* ───────────────── 定位 ───────────────── */

fn apply_window_position(
    app: &App,
    settings: &floatpaste_core::domain::settings::UserSetting,
    target_window_hwnd: Option<isize>,
) {
    let Some(win) = app.picker.upgrade() else {
        return;
    };
    let size = win.window().size();
    let scale = win.window().scale_factor();
    let physical = slint::PhysicalSize::new(
        (size.width as f32 * scale) as u32,
        (size.height as f32 * scale) as u32,
    );

    let mode = settings.picker_position_mode.clone();
    let position = PickerPositionService::resolve_window_position(
        &app.core().repository,
        &mode,
        physical.width as i32,
        physical.height as i32,
        target_window_hwnd,
    )
    .ok()
    .flatten();

    if let Some(point) = position {
        win.window()
            .set_position(slint::PhysicalPosition::new(point.x, point.y));
    }
}

/* ───────────────── 类型徽章（对齐 getClipTypeLabel）───────────────── */

pub fn clip_type_label(item: &ClipItemSummary) -> String {
    match item.r#type.as_str() {
        "text" => "文本".to_string(),
        "image" => "图片".to_string(),
        "file" => {
            let file_count = item.file_count.max(0) as usize;
            let directory_count = (item.directory_count.max(0) as usize).min(file_count);
            if file_count == 0 {
                "文件".to_string()
            } else if directory_count == file_count {
                "文件夹".to_string()
            } else if directory_count > 0 {
                "文件/文件夹".to_string()
            } else {
                "文件".to_string()
            }
        }
        _ => "未知".to_string(),
    }
}

/// 粘贴完成后由 paste_flow 调用：刷新列表顺序（mark_used 改变活动排序）
pub fn notify_pasted(app: &App) {
    refresh_list_changed(app);
}

#[cfg(test)]
mod tests {
    use super::{clamp_preview_with, clip_type_label};
    use floatpaste_core::domain::clip_item::ClipItemSummary;

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

    fn item_of(kind: &str, file_count: i32, directory_count: i32) -> ClipItemSummary {
        ClipItemSummary {
            id: "id".into(),
            r#type: kind.into(),
            content_preview: String::new(),
            source_app: None,
            is_favorited: false,
            file_count,
            directory_count,
            created_at: String::new(),
            updated_at: String::new(),
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

    #[test]
    fn clip_type_label_maps_kinds_and_file_composition() {
        let label =
            |kind: &str, files: i32, dirs: i32| clip_type_label(&item_of(kind, files, dirs));
        assert_eq!(label("text", 0, 0), "文本");
        assert_eq!(label("image", 0, 0), "图片");
        assert_eq!(label("file", 0, 0), "文件");
        assert_eq!(label("file", 3, 3), "文件夹");
        assert_eq!(label("file", 3, 1), "文件/文件夹");
        assert_eq!(label("file", 3, 0), "文件");
        assert_eq!(label("unknown", 0, 0), "未知");
    }
}
