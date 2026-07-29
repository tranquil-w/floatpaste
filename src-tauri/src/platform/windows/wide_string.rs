use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

/// 将 Rust 字符串编码为以 null 结尾的 UTF-16 宽字符序列，供 Win32 API 使用。
pub fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
