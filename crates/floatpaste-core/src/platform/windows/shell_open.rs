//! 用系统默认方式打开路径（编辑窗口预览辅助）。
//!
//! ShellExecuteW "open"：文件按扩展名关联的程序打开（图片走系统照片
//! 查看器等），目录走资源管理器；递交系统后立即返回，不等待进程退出。

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::domain::error::AppError;

use super::wide_string::to_wide;

/// ShellExecuteW "open" 递交系统打开目标，>32 表示已受理启动（约定值，
/// 不代表目标程序最终启动成功）。前置合法性由各公开入口自行检查
fn shell_execute_open(target: &str) -> Result<(), AppError> {
    unsafe {
        let verb = to_wide("open");
        let file = to_wide(target);
        let instance = ShellExecuteW(
            None,
            PCWSTR::from_raw(verb.as_ptr()),
            PCWSTR::from_raw(file.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        if (instance.0 as usize) <= 32 {
            return Err(AppError::Message(format!(
                "系统拒绝打开请求（代码 {code}）",
                code = instance.0 as usize
            )));
        }
    }
    Ok(())
}

/// 用系统默认浏览器打开 URL。URL 不是文件系统路径，不做存在性检查。
pub fn open_url(url: &str) -> Result<(), AppError> {
    if url.is_empty() {
        return Err(AppError::Message("URL 为空".into()));
    }
    shell_execute_open(url)
}

/// 用系统默认程序打开路径。返回 >32 表示系统已受理启动（ShellExecuteW
/// 约定），但不代表目标程序最终启动成功。
pub fn open_path(path: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Err(AppError::Message("路径为空".into()));
    }
    if !std::path::Path::new(path).exists() {
        return Err(AppError::Message(format!("路径不存在：{path}")));
    }
    shell_execute_open(path)
}
