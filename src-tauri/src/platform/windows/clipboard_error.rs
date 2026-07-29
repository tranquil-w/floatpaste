use arboard::Error as ClipboardError;
use crate::domain::error::AppError;

/// 将任意剪贴板相关错误统一映射为 `AppError::Clipboard`，保留错误信息。
pub fn map_clipboard_error(error: impl ToString) -> AppError {
    AppError::Clipboard(error.to_string())
}

/// 判断剪贴板读取错误是否值得重试（仅当剪贴板被占用时）。
pub fn should_retry_clipboard_read(error: &ClipboardError) -> bool {
    matches!(error, ClipboardError::ClipboardOccupied)
}
