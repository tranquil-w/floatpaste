//! 速贴窗口定位与尺寸恢复：三种定位模式（鼠标 / 光标插入符 / 上次位置），
//! 与原版 `picker_position_service.rs` 同算法同常量；窗口尺寸/位置的
//! 读取与落盘由调用方传入，本模块只做纯计算与仓储读写。

use crate::domain::error::AppError;
use crate::domain::settings::{PickerPositionMode, StoredWindowPosition};
use crate::platform::windows::picker_position::{
    caret_point_for_window, current_cursor_point, work_area_from_point, ScreenPoint, ScreenRect,
};
use crate::repository::sqlite_repository::SqliteRepository;

const PICKER_ANCHOR_GAP_PX: i32 = 12;
const PICKER_TOP_ANCHOR_X_DIVISOR: i32 = 5;
pub const PICKER_DEFAULT_WIDTH: u32 = 360;
pub const PICKER_DEFAULT_HEIGHT: u32 = 420;
pub const PICKER_MIN_WIDTH: u32 = 256;
pub const PICKER_MIN_HEIGHT: u32 = 280;

pub struct PickerPositionService;

/// 窗口几何快照：调用方从实际窗口（Tauri / Slint）读出后传入
#[derive(Debug, Clone, Copy)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PickerPositionService {
    /// 解析下次显示位置；window_width/window_height 为物理像素
    pub fn resolve_window_position(
        repository: &SqliteRepository,
        mode: &PickerPositionMode,
        window_width: i32,
        window_height: i32,
        target_window_hwnd: Option<isize>,
    ) -> Result<Option<ScreenPoint>, AppError> {
        Ok(match mode {
            PickerPositionMode::Mouse => resolve_from_mouse(window_width, window_height),
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

    pub fn resolve_window_size(
        repository: &SqliteRepository,
    ) -> Result<Option<(u32, u32)>, AppError> {
        Ok(repository
            .load_picker_window_state()?
            .and_then(stored_window_size))
    }
}

fn resolve_from_mouse(window_width: i32, window_height: i32) -> Option<ScreenPoint> {
    let point = current_cursor_point().ok()?;
    let work_area = work_area_from_point(point).ok()?;
    Some(place_window_near_point(
        point,
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
    let point = target_window_hwnd
        .and_then(|hwnd| caret_point_for_window(hwnd).ok())
        .or_else(|| current_cursor_point().ok())?;
    let work_area = work_area_from_point(point).ok()?;
    Some(place_window_near_point(
        point,
        work_area,
        window_width,
        window_height,
    ))
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

fn place_window_near_point(
    point: ScreenPoint,
    work_area: ScreenRect,
    window_width: i32,
    window_height: i32,
) -> ScreenPoint {
    let mut y = point.y + PICKER_ANCHOR_GAP_PX;
    if y + window_height > work_area.bottom {
        y = point.y - PICKER_ANCHOR_GAP_PX - window_height;
    }

    clamp_top_left(
        ScreenPoint {
            x: point.x - window_width / PICKER_TOP_ANCHOR_X_DIVISOR,
            y,
        },
        work_area,
        window_width,
        window_height,
    )
}

fn center_in_work_area(
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

fn stored_window_size(position: StoredWindowPosition) -> Option<(u32, u32)> {
    if position.width.is_none() && position.height.is_none() {
        return None;
    }

    let (width, height) = clamp_window_size(
        position.width.unwrap_or(PICKER_DEFAULT_WIDTH),
        position.height.unwrap_or(PICKER_DEFAULT_HEIGHT),
    );
    Some((width, height))
}

fn clamp_window_size(width: u32, height: u32) -> (u32, u32) {
    (width.max(PICKER_MIN_WIDTH), height.max(PICKER_MIN_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::{
        center_in_work_area, clamp_top_left, clamp_window_size, place_window_near_point,
        stored_window_size, ScreenPoint, ScreenRect, StoredWindowPosition, PICKER_DEFAULT_HEIGHT,
        PICKER_DEFAULT_WIDTH,
    };

    #[test]
    fn place_window_below_anchor_when_there_is_space() {
        let point = place_window_near_point(
            ScreenPoint { x: 700, y: 200 },
            ScreenRect {
                left: 0,
                top: 0,
                right: 1600,
                bottom: 900,
            },
            360,
            420,
        );

        assert_eq!(point.x, 628);
        assert_eq!(point.y, 212);
    }

    #[test]
    fn place_window_flips_above_when_bottom_space_is_not_enough() {
        let point = place_window_near_point(
            ScreenPoint { x: 700, y: 860 },
            ScreenRect {
                left: 0,
                top: 0,
                right: 1600,
                bottom: 900,
            },
            360,
            420,
        );

        assert_eq!(point.x, 628);
        assert_eq!(point.y, 428);
    }

    #[test]
    fn clamp_top_left_keeps_window_inside_work_area() {
        let point = clamp_top_left(
            ScreenPoint { x: -120, y: 880 },
            ScreenRect {
                left: 0,
                top: 0,
                right: 1600,
                bottom: 900,
            },
            360,
            420,
        );

        assert_eq!(point.x, 0);
        assert_eq!(point.y, 480);
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
    fn clamp_window_size_respects_minimum_bounds() {
        assert_eq!(clamp_window_size(240, 180), (256, 280));
    }

    #[test]
    fn stored_window_size_uses_defaults_for_missing_dimension() {
        let size = stored_window_size(StoredWindowPosition {
            x: 0,
            y: 0,
            width: Some(540),
            height: None,
        })
        .unwrap();

        assert_eq!(size, (540, PICKER_DEFAULT_HEIGHT));
    }

    #[test]
    fn stored_window_size_returns_none_when_legacy_payload_has_no_size() {
        let size = stored_window_size(StoredWindowPosition {
            x: 0,
            y: 0,
            width: None,
            height: None,
        });

        assert!(size.is_none());
    }

    #[test]
    fn stored_window_size_clamps_saved_dimensions() {
        let size = stored_window_size(StoredWindowPosition {
            x: 0,
            y: 0,
            width: Some(200),
            height: Some(120),
        })
        .unwrap();

        assert_eq!(size, (256, 280));
        assert_ne!(size.0, PICKER_DEFAULT_WIDTH);
    }
}
