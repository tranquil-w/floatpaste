//! 速贴窗口定位与尺寸恢复：三种定位模式（鼠标 / 光标插入符 / 上次位置）；
//! 开始菜单开着时不论模式统一改贴其边缘同屏共存（速贴压不过 shell 表面，
//! 见 `platform::windows::start_menu`）；窗口尺寸/位置的读取与落盘由调用方
//! 传入，本模块只做纯计算与仓储读写。

use crate::domain::error::AppError;
use crate::domain::settings::{PickerPositionMode, StoredWindowPosition};
use crate::platform::windows::active_app::is_desktop_window;
use crate::platform::windows::picker_position::{
    caret_point_for_window, current_cursor_point, work_area_from_point, Anchor, ScreenPoint,
    ScreenRect,
};
use crate::platform::windows::start_menu;
use crate::repository::sqlite_repository::SqliteRepository;

const PICKER_ANCHOR_GAP_PX: i32 = 12;
/// 设计尺寸（**逻辑像素**，与 `ui/picker.slint` 的 preferred-width/height 同值）。
/// 不对外导出：直接当物理尺寸用就是这个模块踩过的坑，落窗口前必须过
/// [`default_window_size`] 折算
const PICKER_DEFAULT_WIDTH: u32 = 360;
const PICKER_DEFAULT_HEIGHT: u32 = 420;
/// 最小尺寸（逻辑像素），与窗口自设的最小尺寸同单位
pub const PICKER_MIN_WIDTH: u32 = 256;
pub const PICKER_MIN_HEIGHT: u32 = 280;

pub struct PickerPositionService;

/// 窗口几何快照：调用方从实际窗口（Slint）读出后传入
#[derive(Debug, Clone, Copy)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PickerPositionService {
    /// 解析下次显示位置；window_width/window_height 为**窗口真实物理尺寸**，
    /// 三种模式都会把它与工作区边界一起纳入约束，返回的左上角保证
    /// `width×height` 整块落在锚点所在显示器的工作区内（工作区比窗口还小
    /// 的极端情况退化为工作区左上角）
    pub fn resolve_window_position(
        repository: &SqliteRepository,
        mode: &PickerPositionMode,
        window_width: i32,
        window_height: i32,
        target_window_hwnd: Option<isize>,
    ) -> Result<Option<ScreenPoint>, AppError> {
        // 开始菜单开着时任何模式都逃不出它的矩形：插入符/回贴目标是 shell
        // 自己的元素，鼠标多半正指在开始菜单里，而速贴的 TOPMOST 压不过
        // shell 表面所在的更高 z-order 带——统一改贴开始菜单边缘放置
        if let Some(start) = start_menu::foreground_start_menu_rect() {
            if let Some(point) = resolve_beside_start_menu(start, window_width, window_height) {
                return Ok(Some(point));
            }
        }
        Ok(match mode {
            PickerPositionMode::Mouse => resolve_near_cursor(window_width, window_height),
            PickerPositionMode::Caret => {
                resolve_from_caret(target_window_hwnd, window_width, window_height)
            }
            PickerPositionMode::LastPosition => {
                resolve_from_last_position(repository, window_width, window_height)?
            }
        })
    }

    /// 关闭时持久化位置与尺寸（尺寸钳到最小值）
    pub fn capture_window_position(geometry: WindowGeometry) -> Option<StoredWindowPosition> {
        let (width, height) = clamp_window_size(geometry.width, geometry.height);
        Some(StoredWindowPosition {
            x: geometry.x,
            y: geometry.y,
            width: Some(width),
            height: Some(height),
        })
    }

    /// 记忆的窗口尺寸（**物理像素**）；无记忆返回 None，调用方按
    /// [`default_window_size`] 补默认。`scale_factor` 只服务旧数据只存了
    /// 单边时的补齐——设计尺寸是逻辑像素，补齐结果仍必须是物理像素
    pub fn resolve_window_size(
        repository: &SqliteRepository,
        scale_factor: f32,
    ) -> Result<Option<(u32, u32)>, AppError> {
        Ok(repository
            .load_picker_window_state()?
            .and_then(|position| stored_window_size(position, scale_factor)))
    }
}

/// 无记忆尺寸时的首开尺寸（**物理像素**）：设计尺寸按显示器缩放折算。
/// 直接把设计尺寸当物理像素用在缩放 >100% 的显示器上会小于自定义的
/// 最小尺寸（`PICKER_MIN_* × scale`），被系统钳到最小后首开窗口偏小
pub fn default_window_size(scale_factor: f32) -> (u32, u32) {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };

    (
        ((PICKER_DEFAULT_WIDTH as f32 * scale).round() as u32).max(PICKER_MIN_WIDTH),
        ((PICKER_DEFAULT_HEIGHT as f32 * scale).round() as u32).max(PICKER_MIN_HEIGHT),
    )
}

/// 鼠标锚点解析：贴近光标但不越界。既是 `mouse` 模式的实现，也是配置模式
/// 解析失败（仓储报错）时的兜底——兜底不按模式走，只保证窗口不留在屏外
/// 停屏位；光标或工作区读不到才返回 None，调用方保持原位
pub fn resolve_near_cursor(window_width: i32, window_height: i32) -> Option<ScreenPoint> {
    let point = current_cursor_point().ok()?;
    let work_area = work_area_from_point(point).ok()?;
    Some(place_window_near_point(
        Anchor::at_point(point),
        work_area,
        window_width,
        window_height,
    ))
}

fn resolve_from_caret(
    target_window_hwnd: Option<isize>,
    window_width: i32,
    window_height: i32,
) -> Option<ScreenPoint> {
    // 桌面前台（无应用聚焦，Progman/WorkerW）没有插入符，而 UIA 对
    // 桌面的解析实测阻塞 1.3s——直接按鼠标位置兜底
    let anchor = target_window_hwnd
        .filter(|hwnd| !is_desktop_window(*hwnd))
        .and_then(|hwnd| caret_point_for_window(hwnd).ok())
        .or_else(|| {
            tracing::debug!("caret 定位失败，回退鼠标位置");
            current_cursor_point().ok().map(Anchor::at_point)
        })?;
    let work_area = work_area_from_point(anchor.point).ok()?;
    let placed = place_window_near_point(anchor, work_area, window_width, window_height);
    tracing::debug!(
        "速贴落位 锚点=({},{}) 行高={} 左上角=({},{})",
        anchor.point.x,
        anchor.point.y,
        anchor.line_height,
        placed.x,
        placed.y
    );
    Some(placed)
}

/// 开始菜单同屏共存的落位：取开始菜单中心所在工作区，贴其边缘放置；
/// 工作区不可得（多屏枚举失败等）返回 None，由调用方走配置模式兜底
fn resolve_beside_start_menu(
    start: ScreenRect,
    window_width: i32,
    window_height: i32,
) -> Option<ScreenPoint> {
    let center = ScreenPoint {
        x: (start.left + start.right) / 2,
        y: (start.top + start.bottom) / 2,
    };
    let work_area = work_area_from_point(center).ok()?;
    let placed = beside_start_menu_point(start, work_area, window_width, window_height);
    tracing::debug!(
        "开始菜单同屏落位 开始菜单=({},{})-({},{}) 左上角=({},{})",
        start.left,
        start.top,
        start.right,
        start.bottom,
        placed.x,
        placed.y
    );
    Some(placed)
}

/// 贴开始菜单边缘放置 width×height 窗口的左上角：优先右侧（阅读序在
/// 开始菜单之后），放不下换左侧，两侧都放不下落更宽的一侧并钳进工作区
/// （部分被盖但可见，不劣于被整块盖住）；顶边对齐开始菜单顶边下方一个
/// 空隙（2026-10-11 拍板，底边对齐曾被否）
fn beside_start_menu_point(
    start: ScreenRect,
    work_area: ScreenRect,
    window_width: i32,
    window_height: i32,
) -> ScreenPoint {
    let gap = PICKER_ANCHOR_GAP_PX;
    let right_x = start.right + gap;
    let left_x = start.left - gap - window_width;
    let x = if right_x + window_width <= work_area.right {
        right_x
    } else if left_x >= work_area.left {
        left_x
    } else if work_area.right - start.right >= start.left - work_area.left {
        right_x
    } else {
        left_x
    };
    let y = start.top + gap;
    clamp_top_left(ScreenPoint { x, y }, work_area, window_width, window_height)
}

fn resolve_from_last_position(
    repository: &SqliteRepository,
    window_width: i32,
    window_height: i32,
) -> Result<Option<ScreenPoint>, AppError> {
    if let Some(position) = repository.load_picker_window_state()? {
        let anchor = ScreenPoint {
            x: position.x,
            y: position.y,
        };
        let work_area = work_area_from_point(anchor)
            .or_else(|_| current_cursor_point().and_then(work_area_from_point))
            .ok();

        return Ok(work_area.map(|rect| clamp_top_left(anchor, rect, window_width, window_height)));
    }

    let cursor = current_cursor_point().ok();
    let work_area = cursor.and_then(|point| work_area_from_point(point).ok());
    Ok(work_area.map(|rect| center_in_work_area(rect, window_width, window_height)))
}

/// 贴近锚点放置 width×height 窗口的左上角：优先落在锚点下方，下方放不下
/// 翻到上方，再整体钳进工作区。**尺寸参与约束**——返回的左上角 + 传入的
/// 窗口尺寸必然整块落在工作区内（窗口比工作区还大时退化为工作区左上角），
/// 因此调用方必须传入窗口**真实**的物理尺寸，传小了约束就会失效。
///
/// 翻到上方时让开的是**锚点整行**（`anchor.line_height`）而不只是锚点本身：
/// 只让开空隙会把窗口压在插入符自己那一行上（输入框首行的上半截被切掉）
fn place_window_near_point(
    anchor: Anchor,
    work_area: ScreenRect,
    window_width: i32,
    window_height: i32,
) -> ScreenPoint {
    let point = anchor.point;
    let mut y = point.y + PICKER_ANCHOR_GAP_PX;
    if y + window_height > work_area.bottom {
        y = point.y - anchor.line_height.max(0) - PICKER_ANCHOR_GAP_PX - window_height;
    }

    // 无列信息的锚点（字段框兜底）在锚点上**水平居中**打开：行带中间才是
    // 「贴着这一行」的预期位置；其余锚点（插入符列/鼠标）以窗口左上角对齐
    // 锚点，不横向偏移
    let x = if anchor.centered {
        point.x - window_width / 2
    } else {
        point.x
    };
    clamp_top_left(ScreenPoint { x, y }, work_area, window_width, window_height)
}

/// 工作区内居中放置 width×height 窗口的左上角（物理像素）；窗口大于
/// 工作区时钳回工作区左上（不居中到负偏移）。窗口自身尺寸同步定尺寸的
/// 路径（搜索窗口上屏几何）复用同一算式，避免居中逻辑两处各写一遍
pub fn center_in_work_area(
    work_area: ScreenRect,
    window_width: i32,
    window_height: i32,
) -> ScreenPoint {
    clamp_top_left(
        ScreenPoint {
            x: work_area.left + (work_area.width() - window_width) / 2,
            y: work_area.top + (work_area.height() - window_height) / 2,
        },
        work_area,
        window_width,
        window_height,
    )
}

fn clamp_top_left(
    point: ScreenPoint,
    work_area: ScreenRect,
    window_width: i32,
    window_height: i32,
) -> ScreenPoint {
    let max_x = (work_area.right - window_width).max(work_area.left);
    let max_y = (work_area.bottom - window_height).max(work_area.top);

    ScreenPoint {
        x: point.x.clamp(work_area.left, max_x),
        y: point.y.clamp(work_area.top, max_y),
    }
}

fn stored_window_size(position: StoredWindowPosition, scale_factor: f32) -> Option<(u32, u32)> {
    if position.width.is_none() && position.height.is_none() {
        return None;
    }

    let (default_width, default_height) = default_window_size(scale_factor);
    let (width, height) = clamp_window_size(
        position.width.unwrap_or(default_width),
        position.height.unwrap_or(default_height),
    );
    Some((width, height))
}

fn clamp_window_size(width: u32, height: u32) -> (u32, u32) {
    (width.max(PICKER_MIN_WIDTH), height.max(PICKER_MIN_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::{
        beside_start_menu_point, center_in_work_area, default_window_size,
        place_window_near_point, stored_window_size, Anchor, ScreenPoint, ScreenRect,
        StoredWindowPosition, PICKER_DEFAULT_WIDTH, PICKER_MIN_HEIGHT, PICKER_MIN_WIDTH,
    };

    const WORK: ScreenRect = ScreenRect {
        left: 0,
        top: 0,
        right: 1600,
        bottom: 900,
    };

    /// 鼠标式锚点（无行高）
    fn mouse_anchor(x: i32, y: i32) -> Anchor {
        Anchor::at_point(ScreenPoint { x, y })
    }

    /// 断言窗口（左上角 + 尺寸）整块落在工作区内
    fn assert_fits_inside(point: ScreenPoint, work: ScreenRect, width: i32, height: i32) {
        assert!(
            point.x >= work.left && point.y >= work.top,
            "左上角越界: {point:?}"
        );
        assert!(
            point.x + width <= work.right && point.y + height <= work.bottom,
            "右下角越界: {point:?}"
        );
    }

    /// 翻到上方时窗口底边必须让开锚点**整行**：只有空隙（12px）会让窗口压在
    /// 插入符自己那一行上（WorkBuddy 输入框首行被切掉半行就是这个）
    #[test]
    fn place_window_clears_anchor_line_when_flipping_above() {
        // 与真机同款：插入符行 1170..1190（行高 20），窗口 460 高，工作区下沿 1392
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1392,
        };
        let caret = Anchor {
            point: ScreenPoint { x: 1140, y: 1190 },
            line_height: 20,
            centered: false,
        };

        let placed = place_window_near_point(caret, work, 360, 460);

        // 底边 = 1190-20-12 = 1158，让开插入符行上沿 1170
        assert_eq!(placed.y, 1158 - 460);
        assert!(placed.y + 460 < 1170, "窗口压在插入符行上: {placed:?}");
    }

    /// 无列信息的锚点（字段框兜底）窗口**水平居中**于锚点打开；其余锚点
    /// （插入符列/鼠标）窗口左上角对齐锚点
    #[test]
    fn centered_anchor_opens_window_centered_on_point() {
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1392,
        };
        let caret = Anchor {
            point: ScreenPoint { x: 1140, y: 200 },
            line_height: 20,
            centered: true,
        };

        let placed = place_window_near_point(caret, work, 360, 420);
        assert_eq!(placed.x, 1140 - 180);

        let lead = Anchor {
            centered: false,
            ..caret
        };
        let placed = place_window_near_point(lead, work, 360, 420);
        assert_eq!(placed.x, 1140);
    }

    /// 下方放得下时行高不参与（免得平白把窗口往下推）
    #[test]
    fn place_window_below_anchor_ignores_line_height() {
        let caret = Anchor {
            point: ScreenPoint { x: 700, y: 200 },
            line_height: 20,
            centered: false,
        };

        let placed = place_window_near_point(caret, WORK, 360, 420);

        assert_eq!(placed.y, 212);
    }

    /// 开始菜单（居中，Win11 常态）开着：落其右侧，顶边对齐其顶边下方
    /// 一个空隙
    #[test]
    fn beside_start_menu_places_on_right_side_top_aligned() {
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let start = ScreenRect {
            left: 640,
            top: 300,
            right: 1280,
            bottom: 1030,
        };

        let placed = beside_start_menu_point(start, work, 360, 420);

        assert_eq!(placed.x, 1280 + 12);
        assert_eq!(placed.y, 300 + 12);
        assert_fits_inside(placed, work, 360, 420);
    }

    /// 右侧放不下（开始菜单偏右）：换左缘放置
    #[test]
    fn beside_start_menu_falls_back_to_left_side() {
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let start = ScreenRect {
            left: 1400,
            top: 300,
            right: 1900,
            bottom: 1030,
        };

        let placed = beside_start_menu_point(start, work, 360, 420);

        assert_eq!(placed.x, 1400 - 12 - 360);
        assert_fits_inside(placed, work, 360, 420);
    }

    /// 两侧都放不下（开始菜单几乎占满工作区）：落更宽的一侧并整体钳进
    /// 工作区（部分被盖但可见，不越屏）
    #[test]
    fn beside_start_menu_clamps_into_wider_side_when_neither_fits() {
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 700,
        };
        let start = ScreenRect {
            left: 80,
            top: 0,
            right: 920,
            bottom: 690,
        };

        let placed = beside_start_menu_point(start, work, 360, 420);

        // 右侧剩 80、左侧剩 80：右侧胜出，钳进工作区后贴右沿
        assert_eq!(placed.x, 1000 - 360);
        assert_fits_inside(placed, work, 360, 420);
    }

    #[test]
    fn center_in_work_area_uses_visible_center() {
        let point = center_in_work_area(
            ScreenRect {
                left: 100,
                top: 80,
                right: 1700,
                bottom: 980,
            },
            360,
            420,
        );

        assert_eq!(point.x, 720);
        assert_eq!(point.y, 320);
    }

    #[test]
    fn stored_window_size_scales_default_for_missing_dimension() {
        // 旧数据只存了宽：补出来的高同样是物理像素
        let size = stored_window_size(
            StoredWindowPosition {
                x: 0,
                y: 0,
                width: Some(540),
                height: None,
            },
            1.5,
        )
        .unwrap();

        assert_eq!(size, (540, 630));
    }

    #[test]
    fn stored_window_size_returns_none_when_legacy_payload_has_no_size() {
        let size = stored_window_size(
            StoredWindowPosition {
                x: 0,
                y: 0,
                width: None,
                height: None,
            },
            1.0,
        );

        assert!(size.is_none());
    }

    #[test]
    fn stored_window_size_clamps_saved_dimensions() {
        let size = stored_window_size(
            StoredWindowPosition {
                x: 0,
                y: 0,
                width: Some(200),
                height: Some(120),
            },
            1.0,
        )
        .unwrap();

        assert_eq!(size, (256, 280));
        assert_ne!(size.0, PICKER_DEFAULT_WIDTH);
    }

    #[test]
    fn place_window_stays_inside_work_area_at_every_corner() {
        // 光标贴四角时窗口也必须整块可见（越界即失败）；带行高的锚点一并覆盖
        let probes = [
            mouse_anchor(0, 0),
            mouse_anchor(1599, 0),
            mouse_anchor(0, 899),
            mouse_anchor(1599, 899),
            Anchor {
                point: ScreenPoint { x: 1599, y: 899 },
                line_height: 24,
                centered: false,
            },
        ];

        for anchor in probes {
            let placed = place_window_near_point(anchor, WORK, 360, 420);
            assert_fits_inside(placed, WORK, 360, 420);
        }
    }

    #[test]
    fn place_window_keeps_window_inside_on_negative_origin_monitor() {
        // 副屏在工作区左侧/上方时工作区原点是负数
        let work = ScreenRect {
            left: -1920,
            top: -100,
            right: 0,
            bottom: 980,
        };

        let placed = place_window_near_point(mouse_anchor(-1, -100), work, 360, 420);

        assert_fits_inside(placed, work, 360, 420);
    }

    #[test]
    fn place_window_degrades_to_work_area_origin_when_window_is_larger() {
        // 窗口比工作区还大：无处可放，退化为工作区左上角（不允许负偏移）
        let work = ScreenRect {
            left: 100,
            top: 60,
            right: 400,
            bottom: 300,
        };

        let placed = place_window_near_point(mouse_anchor(380, 290), work, 600, 500);

        assert_eq!(placed, ScreenPoint { x: 100, y: 60 });
    }

    #[test]
    fn default_window_size_follows_display_scale() {
        assert_eq!(default_window_size(1.0), (360, 420));
        assert_eq!(default_window_size(1.5), (540, 630));
        assert_eq!(default_window_size(2.0), (720, 840));
    }

    #[test]
    fn default_window_size_never_drops_below_minimum() {
        // 缩放异常（NaN）退回 1.0
        assert_eq!(default_window_size(f32::NAN), (360, 420));
        // 缩放小于 1（异常但可能）：折算后不缩到最小尺寸以下
        assert_eq!(
            default_window_size(0.5),
            (PICKER_MIN_WIDTH, PICKER_MIN_HEIGHT)
        );
    }
}
