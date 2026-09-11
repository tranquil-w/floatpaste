//! 悬停预览 tooltip：400ms 延迟触发、请求失效、内容构建、排版度量与翻转定位。
//!
//! 对齐原版 useHoverTooltip + tooltip.html + TooltipWindow：
//! - 悬停移动每帧重置计时；离开条目立即取消并隐藏；
//! - 文本条目按需取全文（detail），图片条目解码原图预览（≤560×420）；
//! - 定位：面板窗口位置 + (条目内鼠标 + 12,16)×scale，越界时按光标翻转（gap 4）；
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
use floatpaste_core::services::time_format::format_relative_time_or_unused;

use crate::overlay::{self, ForegroundPolicy};
use crate::picker::{self, App};
use crate::thumbnails;
use crate::win32_ext;
use crate::TooltipMetaBadge;

const SHOW_DELAY_MS: u64 = 400;
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
            // 图片解码可能上百毫秒：放后台线程（只传原始像素），回填走 token 失效校验
            let core = app.core().clone();
            let app_for_cb = app.clone();
            let item_for_cb = item.clone();
            let path = item.image_path.clone().unwrap_or_default();
            std::thread::spawn(move || {
                let raw = thumbnails::load_full_image_raw(&core, &path);
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

/// 取消挂起的显示并隐藏当前 tooltip（离开条目/会话结束/粘贴前调用）
pub fn cancel(app: &App) {
    PENDING_TOKEN.with(|value| value.set(value.get() + 1));
    if let Some(win) = app.tooltip.upgrade() {
        let hwnd = app.state.tooltip_hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            let _ = window_control::hide_window(hwnd);
        }
        if win.window().is_visible() {
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

    let physical_w = (width * dpi).round().max(1.0) as u32;
    let physical_h = (height * dpi).round().max(1.0) as u32;
    win.window()
        .set_size(slint::PhysicalSize::new(physical_w, physical_h));

    // ── 定位：宿主窗口原点 + (条目内鼠标 + 偏移)×scale，越界翻转 ──
    let position = resolve_position(host_hwnd, mouse_x, mouse_y, dpi, physical_w, physical_h);
    win.window()
        .set_position(slint::PhysicalPosition::new(position.0, position.1));

    // ── 显示（点击穿透 + 置顶不激活 + 前台若被抢则归还）──
    // winit show 的异步样式重置与前台抢占统一交 overlay 公共层处理
    let prev_foreground = ActiveAppResolver::current_foreground_hwnd();
    let _ = win.window().show();
    let tooltip_hwnd = app.state.tooltip_hwnd.load(Ordering::SeqCst);
    if tooltip_hwnd != 0 {
        let restore =
            prev_foreground.map_or(ForegroundPolicy::Keep, ForegroundPolicy::RestoreIfStolen);
        overlay::after_show(tooltip_hwnd, true, restore, restore);
    }
}

/// 定位与翻转（对齐 TooltipWindow::resolve_clamped_position：
/// 默认右下展开，超出工作区时按当前光标位置翻到对侧，留 4px 间隙）
fn resolve_position(
    picker_hwnd: isize,
    mouse_x: f32,
    mouse_y: f32,
    scale: f32,
    width: u32,
    height: u32,
) -> (i32, i32) {
    let picker_rect = (picker_hwnd > 0)
        .then(|| win32_ext::physical_rect(picker_hwnd))
        .flatten();
    let (mut x, mut y) = match picker_rect {
        Some(rect) => (
            rect.left + ((mouse_x + OFFSET_X) * scale) as i32,
            rect.top + ((mouse_y + OFFSET_Y) * scale) as i32,
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
        text: picker::clip_type_label(item).into(),
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
