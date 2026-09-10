// 迁移垫层：已下沉 floatpaste-core 的平台模块经 glob 再导出保持旧路径可用；
// 壳层仅剩依赖 Tauri 窗口类型的模块。
pub use floatpaste_core::platform::windows::*;

pub mod app_icon;
pub mod picker_mouse_monitor;
pub mod window_utils;
