// 迁移垫层：纯业务服务已下沉 floatpaste-core，经 glob 再导出保持旧路径可用。
pub use floatpaste_core::services::*;

pub mod paste_executor;
pub mod picker_position_service;
pub mod settings_service;
pub mod shortcut_manager;
pub mod tooltip_window;
pub mod tray_service;
pub mod window_coordinator;
