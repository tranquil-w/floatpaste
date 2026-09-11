//! 速贴窗口定位：Tauri 窗口类型的薄适配层。
//! 定位算法以 floatpaste-core 的 picker_position_service 为唯一实现，
//! 本文件只做窗口几何读取与类型转换，不得复制常量或算法。

use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::{
    domain::{
        error::AppError,
        settings::{PickerPositionMode, StoredWindowPosition},
    },
    repository::sqlite_repository::SqliteRepository,
};

pub use floatpaste_core::services::picker_position_service::{
    PickerPositionService as CorePickerPositionService, WindowGeometry, PICKER_DEFAULT_HEIGHT,
    PICKER_DEFAULT_WIDTH, PICKER_MIN_HEIGHT, PICKER_MIN_WIDTH,
};

pub struct PickerPositionService;

impl PickerPositionService {
    pub fn resolve_window_position(
        window: &WebviewWindow,
        repository: &SqliteRepository,
        mode: &PickerPositionMode,
        target_window_hwnd: Option<isize>,
    ) -> Result<Option<PhysicalPosition<i32>>, AppError> {
        let size = window.outer_size()?;
        let resolved = CorePickerPositionService::resolve_window_position(
            repository,
            mode,
            size.width as i32,
            size.height as i32,
            target_window_hwnd,
        )?;
        Ok(resolved.map(|point| PhysicalPosition::new(point.x, point.y)))
    }

    pub fn capture_window_position(
        window: &WebviewWindow,
    ) -> Result<Option<StoredWindowPosition>, AppError> {
        let position = window.outer_position()?;
        let size = window.inner_size()?;
        Ok(CorePickerPositionService::capture_window_position(
            WindowGeometry {
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
            },
        ))
    }

    pub fn resolve_window_size(
        repository: &SqliteRepository,
    ) -> Result<Option<PhysicalSize<u32>>, AppError> {
        Ok(CorePickerPositionService::resolve_window_size(repository)?
            .map(|(width, height)| PhysicalSize::new(width, height)))
    }
}
