pub mod clips;
pub mod settings;
pub mod windows;

/// 将任意可转为字符串的错误映射为 Tauri 命令所需的 `String` 错误类型。
pub(crate) fn map_error(error: impl ToString) -> String {
    error.to_string()
}
