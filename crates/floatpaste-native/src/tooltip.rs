//! 悬停预览 tooltip：400ms 延迟触发、请求失效、内容构建、排版度量与翻转定位。
//!
//! 对齐原版 useHoverTooltip + tooltip.html + TooltipWindow：
//! - 悬停移动每帧重置计时；离开条目立即取消并隐藏；
//! - 文本条目按需取全文（detail），图片条目解码原图预览（≤560×420）；
//!   静态文案模式（工具栏按钮提示）由调用方给定文案与锚点；
//! - 定位：面板窗口位置 + 锚点×scale，越界时按光标翻转（gap 4）；
//! - 点击穿透 + 置顶不激活显示（winit 样式重置统一交 overlay 公共层处理）。

use std::rc::Rc;
use std::sync::atomic::Ordering;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use floatpaste_core::domain::clip_item::ClipItemSummary;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::picker_position::{
    current_cursor_point, work_area_from_point,
};
use floatpaste_core::platform::windows::window_control;
use floatpaste_core::services::clip_display::clip_type_label;
use floatpaste_core::services::time_format::format_relative_time_or_unused;

use crate::overlay::{self, ForegroundPolicy};
use crate::picker::App;
use crate::thumbnails;
use crate::win32_ext;
use crate::{TooltipMetaBadge, TooltipWindow};

const SHOW_DELAY_MS: u64 = 400;
/// tooltip.slint 的 preferred 尺寸（隐藏复位用，与 ui 保持一致）
const PREFERRED_WIDTH: f32 = 200.0;
const PREFERRED_HEIGHT: f32 = 80.0;
/// 鼠标右下偏移（逻辑像素）
const OFFSET_X: f32 = 12.0;
const OFFSET_Y: f32 = 16.0;
/// 内容与窗口约束（对齐 tooltip.html；卡片 padding/边框读 slint 属性）
const MAX_WIDTH: f32 = 600.0;
const MIN_WIDTH: f32 = 120.0;
const IMAGE_MAX_WIDTH: f32 = 560.0;
const IMAGE_MAX_HEIGHT: f32 = 420.0;
/// 高度安全余量：字体度量与窗口装配存在边缘差异时防止裁掉尾行
const HEIGHT_SAFETY: f32 = 8.0;
/// 静态提示为单行短文案，余量收小避免卡片下方留白
const HEIGHT_SAFETY_STATIC: f32 = 4.0;
/// 静态提示的宽度下限：单行短文案不需要长文预览的阅读下限
const MIN_WIDTH_STATIC: f32 = 40.0;

thread_local! {
    /// 悬停调度的请求 id（对齐前端 requestIdRef：新请求使旧回调失效）
    static PENDING_TOKEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

enum Payload {
    Text(String),
    /// None = 图片解码失败，按元数据占位显示
    Image(Option<slint::Image>),
}

/// tooltip 宿主上下文：定位基准窗口与会话活跃判定
/// （速贴与搜索窗口共用同一个 tooltip 窗口）
#[derive(Clone, Copy)]
pub struct HoverHost {
    /// 宿主窗口句柄（tooltip 以其原点 + 条目内鼠标坐标定位；缩放由
    /// render 按句柄实时取 DPI）
    pub hwnd: isize,
    /// 到点显示前复查宿主会话是否仍活跃（会话结束则放弃显示）
    pub is_active: fn(&App) -> bool,
}

fn picker_is_active(app: &App) -> bool {
    app.state.is_picker_active()
}

/// 速贴面板悬停调度（条目索引取自速贴列表缓存）
pub fn schedule(app: &App, index: usize, mouse_x: f32, mouse_y: f32) {
    let Some(item) = app.state.item_at(index) else {
        return;
    };
    let host = HoverHost {
        hwnd: app.state.picker_hwnd.load(Ordering::SeqCst),
        is_active: picker_is_active,
    };
    schedule_with(app, host, item, mouse_x, mouse_y);
}

/// 条目内鼠标移动：重置 400ms 计时，到点后构建内容并显示
pub fn schedule_with(
    app: &App,
    host: HoverHost,
    item: ClipItemSummary,
    mouse_x: f32,
    mouse_y: f32,
) {
    let token = PENDING_TOKEN.with(|value| {
        value.set(value.get() + 1);
        value.get()
    });
    let host_hwnd = host.hwnd;
    let host_active = host.is_active;
    let app = app.clone();

    slint::Timer::single_shot(std::time::Duration::from_millis(SHOW_DELAY_MS), move || {
        if PENDING_TOKEN.with(|value| value.get()) != token || !host_active(&app) {
            return;
        }

        if item.r#type == "image" && item.image_path.is_some() {
            // 图片解码可能上百毫秒：放后台线程（只传原始像素），回填走 token 失效校验。
            // 预览缩到显示上限（逻辑 560×420 × 宿主 DPI）：超出的原像素不可见，
            // 全量解码只会制造内存尖峰（4K 截图 RGBA ≈ 33MB，缩放后 ≈ 3MB）
            let dpi = if host_hwnd > 0 {
                win32_ext::window_dpi(host_hwnd) as f32 / 96.0
            } else {
                1.0
            };
            let max_w = (IMAGE_MAX_WIDTH * dpi).ceil().max(1.0) as u32;
            let max_h = (IMAGE_MAX_HEIGHT * dpi).ceil().max(1.0) as u32;
            let core = app.core().clone();
            let app_for_cb = app.clone();
            let item_for_cb = item.clone();
            let path = item.image_path.clone().unwrap_or_default();
            std::thread::spawn(move || {
                let raw = thumbnails::load_preview_image_raw(&core, &path, max_w, max_h);
                let _ = slint::invoke_from_event_loop(move || {
                    if PENDING_TOKEN.with(|value| value.get()) != token || !host_active(&app_for_cb)
                    {
                        return;
                    }
                    let image = raw.map(thumbnails::image_from_rgba);
                    render(
                        &app_for_cb,
                        host_hwnd,
                        &item_for_cb,
                        mouse_x,
                        mouse_y,
                        Payload::Image(image),
                    );
                });
            });
        } else {
            let full_text = app
                .core()
                .repository
                .get_item_detail(&item.id)
                .ok()
                .map(|detail| detail.full_text)
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| item.content_preview.clone());
            // 原版 HTML 的 pre-wrap 不为块尾换行生成行框，尾部空白不可见；
            // Slint 会照排成空行，把元信息行顶出窗口下方，显示前裁掉
            render(
                &app,
                host_hwnd,
                &item,
                mouse_x,
                mouse_y,
                Payload::Text(full_text.trim_end().to_owned()),
            );
        }
    });
}

/// 静态文案提示（工具栏按钮等固定文案）：文案与锚点由调用方给定，
/// anchor 为宿主窗口坐标（通常取按钮下方）。400ms 延迟与请求失效令牌
/// 与条目预览共用，悬停目标切换时互相打断
pub fn schedule_static(app: &App, host: HoverHost, text: String, anchor_x: f32, anchor_y: f32) {
    let token = PENDING_TOKEN.with(|value| {
        value.set(value.get() + 1);
        value.get()
    });
    let host_hwnd = host.hwnd;
    let host_active = host.is_active;
    let app = app.clone();

    slint::Timer::single_shot(std::time::Duration::from_millis(SHOW_DELAY_MS), move || {
        if PENDING_TOKEN.with(|value| value.get()) != token || !host_active(&app) {
            return;
        }
        render_static(&app, host_hwnd, &text, anchor_x, anchor_y);
    });
}

/// 取消挂起的显示并隐藏当前 tooltip（离开条目/会话结束/粘贴前调用）
pub fn cancel(app: &App) {
    PENDING_TOKEN.with(|value| value.set(value.get() + 1));
    if let Some(win) = app.tooltip.upgrade() {
        // 软件渲染按窗口尺寸分配帧缓冲：大图预览会把窗口顶到最大显示
        // 尺寸，隐藏时复位到 preferred，避免小内容期 buffer 顶格常驻
        let _ = win
            .window()
            .set_size(slint::LogicalSize::new(PREFERRED_WIDTH, PREFERRED_HEIGHT));
        let hwnd = app.state.tooltip_hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            // 只用 Win32 SW_HIDE：Slint hide 会翻转 winit 可见标志，
            // 下次显示将退回 SW_SHOW 路径并激活窗口
            let _ = window_control::hide_window(hwnd);
        } else if win.window().is_visible() {
            // 装配失败降级路径：tooltip 由 Slint show 显示，需 Slint hide
            let _ = win.window().hide();
        }
    }
}

fn render(
    app: &App,
    host_hwnd: isize,
    item: &ClipItemSummary,
    mouse_x: f32,
    mouse_y: f32,
    payload: Payload,
) {
    let Some(win) = app.tooltip.upgrade() else {
        return;
    };
    // 条目预览总是带元信息行（静态提示可能刚用过 simple-mode）
    win.set_simple_mode(false);

    let dpi = if host_hwnd > 0 {
        win32_ext::window_dpi(host_hwnd) as f32 / 96.0
    } else {
        1.0
    };

    // ── 内容与度量 ──
    let inset_h = win.get_card_inset_h();
    let chrome_v = win.get_card_chrome_v();
    let (content_width, content_height) = match &payload {
        Payload::Text(text) => {
            // 自然宽度（最宽段落一行放下所需）与高度同走 Slint 排版度量：
            // 与内容 Text 同引擎，卡片宽窄与高度都不会偏离实际渲染
            let natural_width = win.invoke_measure_natural_width(text.as_str().into());
            let card_width = (natural_width + inset_h).clamp(MIN_WIDTH, MAX_WIDTH);
            let text_width = card_width - inset_h;
            let content_height = win.invoke_measure_text(text.as_str().into(), text_width);
            win.set_content_text(text.as_str().into());
            win.set_has_image(false);
            (text_width, content_height)
        }
        Payload::Image(image) => {
            // 元数据等比缩到 560×420 内（逻辑像素）
            let (w, h) = match image {
                Some(image) if image.size().width > 0 => {
                    (image.size().width as f32, image.size().height as f32)
                }
                _ => (
                    item.image_width.unwrap_or(0) as f32,
                    item.image_height.unwrap_or(0) as f32,
                ),
            };
            let (display_w, display_h) = if w > 0.0 && h > 0.0 {
                let ratio = (IMAGE_MAX_WIDTH / w).min(IMAGE_MAX_HEIGHT / h).min(1.0);
                ((w * ratio).round(), (h * ratio).round())
            } else {
                (0.0, 0.0)
            };
            if let Some(image) = image {
                win.set_image(image.clone());
            }
            win.set_has_image(true);
            win.set_image_width(display_w);
            win.set_image_height(display_h);
            (display_w, display_h)
        }
    };

    // ── 元信息 ──
    win.set_badges(build_badges(item));
    win.set_source_app(
        item.source_app
            .clone()
            .unwrap_or_else(|| "未知来源".into())
            .into(),
    );
    win.set_time_text(
        format_relative_time_or_unused(
            item.last_used_at
                .as_deref()
                .or(Some(item.created_at.as_str())),
        )
        .into(),
    );

    // ── 窗口尺寸（chrome 读 slint 属性 + 安全余量）──
    let width = (content_width + inset_h).clamp(MIN_WIDTH, MAX_WIDTH);
    let height = content_height + chrome_v + HEIGHT_SAFETY;
    present(
        app,
        &win,
        host_hwnd,
        dpi,
        width,
        height,
        (mouse_x + OFFSET_X, mouse_y + OFFSET_Y),
    );
}

/// 静态提示渲染：单行短文案（text-xs/medium），无元信息行，锚点取按钮下方
fn render_static(app: &App, host_hwnd: isize, text: &str, anchor_x: f32, anchor_y: f32) {
    let Some(win) = app.tooltip.upgrade() else {
        return;
    };
    let dpi = if host_hwnd > 0 {
        win32_ext::window_dpi(host_hwnd) as f32 / 96.0
    } else {
        1.0
    };

    let natural_width = win.invoke_measure_simple_width(text.into());
    let width = (natural_width + win.get_card_inset_h()).clamp(MIN_WIDTH_STATIC, MAX_WIDTH);
    let height = win.get_simple_line_height()
        + win.get_card_chrome_v_simple()
        + HEIGHT_SAFETY_STATIC;

    win.set_simple_mode(true);
    win.set_has_image(false);
    win.set_content_text(text.into());
    win.set_badges(ModelRc::new(Rc::new(VecModel::from(Vec::new()))));
    present(app, &win, host_hwnd, dpi, width, height, (anchor_x, anchor_y));
}

/// 定尺寸、定位并显示：条目预览与静态提示共用的收尾（anchor 已含偏移）
fn present(
    app: &App,
    win: &TooltipWindow,
    host_hwnd: isize,
    dpi: f32,
    width: f32,
    height: f32,
    anchor: (f32, f32),
) {
    let physical_w = (width * dpi).round().max(1.0) as u32;
    let physical_h = (height * dpi).round().max(1.0) as u32;
    win.window()
        .set_size(slint::PhysicalSize::new(physical_w, physical_h));

    // ── 定位：宿主窗口原点 + 锚点×scale，越界按光标翻转 ──
    let position = resolve_position(host_hwnd, anchor.0, anchor.1, dpi, physical_w, physical_h);
    win.window()
        .set_position(slint::PhysicalPosition::new(position.0, position.1));

    // ── 显示（点击穿透 + 置顶不激活 + 前台若被抢则归还）──
    // 不走 Slint show：winit 对二次 show 固定 SW_SHOW，无视 NOACTIVATE
    // 激活窗口，打断宿主输入框焦点与 IME 组合。启动装配后 winit 可见
    // 标志恒为真，这里直接复用速贴的无激活显示（SW_SHOWNOACTIVATE）；
    // 样式重挂与置顶由 after_show 兜底。
    // RestoreIfStolen 双保险：实测部分激活仍会穿透到达（时序在 show 与
    // 兜底之间），归还后须把 tooltip 重新抬到置顶带顶部——宿主同为置顶，
    // SetForegroundWindow 会把它提到 tooltip 之上盖住内容，两步均不激活
    let tooltip_hwnd = app.state.tooltip_hwnd.load(Ordering::SeqCst);
    let prev_foreground = ActiveAppResolver::current_foreground_hwnd();
    if tooltip_hwnd != 0 {
        win32_ext::apply_overlay_style(tooltip_hwnd, true);
        let _ = window_control::remove_window_system_menu(tooltip_hwnd);
        let _ = window_control::show_window_no_activate(tooltip_hwnd);
        let restore =
            prev_foreground.map_or(ForegroundPolicy::Keep, ForegroundPolicy::RestoreIfStolen);
        overlay::after_show(tooltip_hwnd, true, restore, ForegroundPolicy::Keep);
        let host = host_hwnd;
        slint::Timer::single_shot(std::time::Duration::from_millis(60), move || {
            if ActiveAppResolver::current_foreground_hwnd() == Some(tooltip_hwnd)
                && ActiveAppResolver::restore_foreground_window(host)
            {
                window_control::set_window_topmost_no_activate(tooltip_hwnd);
            }
        });
    } else {
        // 装配失败降级：裸 Slint show，可接受激活副作用
        let _ = win.window().show();
    }
}

/// 定位与翻转（对齐 TooltipWindow::resolve_clamped_position：
/// anchor 为相对宿主窗口原点的逻辑锚点（已含偏移），默认向右下展开，
/// 超出工作区时按当前光标位置翻到对侧，留 4px 间隙）
fn resolve_position(
    picker_hwnd: isize,
    anchor_x: f32,
    anchor_y: f32,
    scale: f32,
    width: u32,
    height: u32,
) -> (i32, i32) {
    let picker_rect = (picker_hwnd > 0)
        .then(|| win32_ext::physical_rect(picker_hwnd))
        .flatten();
    let (mut x, mut y) = match picker_rect {
        Some(rect) => (
            rect.left + (anchor_x * scale) as i32,
            rect.top + (anchor_y * scale) as i32,
        ),
        None => (0, 0),
    };

    if let Ok(cursor) = current_cursor_point() {
        if let Ok(work_area) = work_area_from_point(cursor) {
            const FLIP_GAP: i32 = 4;
            if x + width as i32 > work_area.right {
                x = cursor.x - width as i32 - FLIP_GAP;
            }
            if y + height as i32 > work_area.bottom {
                y = cursor.y - height as i32 - FLIP_GAP;
            }
        }
    }

    (x, y)
}

/// 构建元信息徽章模型（类型徽章 / 图片尺寸 / 图片格式）
fn build_badges(item: &ClipItemSummary) -> ModelRc<TooltipMetaBadge> {
    let mut badges: Vec<TooltipMetaBadge> = Vec::new();
    badges.push(TooltipMetaBadge {
        kind: 0,
        text: clip_type_label(item).into(),
    });
    if item.r#type == "image" {
        if let (Some(width), Some(height)) = (item.image_width, item.image_height) {
            badges.push(TooltipMetaBadge {
                kind: 1,
                text: SharedString::from(format!("{width} × {height}")),
            });
        }
        if let Some(format) = &item.image_format {
            badges.push(TooltipMetaBadge {
                kind: 2,
                text: SharedString::from(format.clone()),
            });
        }
    }
    ModelRc::new(Rc::new(VecModel::from(badges)))
}
