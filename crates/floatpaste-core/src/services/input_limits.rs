//! 采集与公开入库共用的资源上限；在复制载荷和分配像素前检查。
use crate::domain::error::AppError;

pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FORMAT_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_SNAPSHOT_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_FILE_COUNT: usize = 1024;
pub const MAX_PATH_UNITS: usize = 32_767;

pub fn validate_text(text: &str) -> Result<(), AppError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(AppError::InputLimit("文本超过 8 MiB"));
    }
    Ok(())
}

pub fn validate_files(paths: &[String]) -> Result<(), AppError> {
    if paths.len() > MAX_FILE_COUNT
        || paths
            .iter()
            .any(|path| path.encode_utf16().count() > MAX_PATH_UNITS)
        || paths
            .iter()
            .try_fold(0usize, |n, path| n.checked_add(path.len()))
            .is_none_or(|n| n > MAX_TEXT_BYTES)
    {
        return Err(AppError::InputLimit("文件列表超过数量或长度限制"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_input_limits_accept_boundary_and_reject_overflow() {
        assert!(validate_text(&"x".repeat(MAX_TEXT_BYTES)).is_ok());
        assert!(validate_text(&"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
        assert!(validate_files(&vec!["a".into(); MAX_FILE_COUNT]).is_ok());
        assert!(validate_files(&vec!["a".into(); MAX_FILE_COUNT + 1]).is_err());
        assert!(validate_files(&["a".repeat(MAX_PATH_UNITS + 1)]).is_err());
    }
}
