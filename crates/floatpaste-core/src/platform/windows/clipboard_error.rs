use crate::domain::error::AppError;

/// 将任意剪贴板相关错误统一映射为 `AppError::Clipboard`，保留错误信息。
pub fn map_clipboard_error(error: impl ToString) -> AppError {
    AppError::Clipboard(error.to_string())
}
