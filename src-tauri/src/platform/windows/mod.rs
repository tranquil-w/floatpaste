// 迁移垫层：已下沉 floatpaste-core 的平台模块在此显式列举，仅服务旧壳
// 代码的旧路径引用；新代码一律直接依赖 floatpaste-core，
// 不再往垫层里加条目，清零即删除本块。壳层自有模块见下方 mod 声明。
pub use floatpaste_core::platform::windows::{
    active_app, clipboard_monitor, picker_position, wide_string,
};

pub mod app_icon;
pub mod picker_mouse_monitor;
pub mod window_utils;
