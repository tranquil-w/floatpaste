// 迁移垫层：纯业务服务已下沉 floatpaste-core，这里只显式列举旧壳代码
// 仍经旧路径引用的模块；新代码一律直接依赖 floatpaste_core，
// 不再往垫层里加条目，清零即删除本块。
pub use floatpaste_core::services::{
    clip_service, history_service, image_storage, paste_support, retention_service, search_service,
    startup_service, tag_service,
};

pub mod paste_executor;
pub mod picker_position_service;
pub mod settings_service;
pub mod shortcut_manager;
pub mod tooltip_window;
pub mod tray_service;
pub mod window_coordinator;
