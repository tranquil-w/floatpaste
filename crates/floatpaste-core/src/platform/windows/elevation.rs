//! 进程提权检测与提权启动（管理员上屏）。
//!
//! UIPI 语义：普通（中等完整性）进程注入的按键（SendInput）会被以管理员
//! 权限运行的目标窗口静默丢弃，因此速贴/搜索向此类目标回贴必然无效。
//! 这里提供目标进程提权检测（TokenElevation：仅识别「用户主动以管理员
//! 运行」的进程，SYSTEM 窗口不误报）与 runas 提权启动。

use std::mem::size_of;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, ShellExecuteW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, SW_SHOWNORMAL};

use crate::domain::error::AppError;

use super::wide_string::to_wide;

/// UAC 确认框被用户取消（ShellExecuteEx 的 GetLastError）
const ERROR_CANCELLED: u32 = 1223;

/// 当前进程是否以管理员权限运行
pub fn is_current_process_elevated() -> bool {
    unsafe { process_token_is_elevated(GetCurrentProcess()) }
}

/// 窗口所属进程是否以管理员权限运行（探测不到时一律返回 false，不提示）
pub fn is_window_process_elevated(hwnd: isize) -> bool {
    if hwnd <= 0 {
        return false;
    }
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let elevated = process_token_is_elevated(process);
        let _ = CloseHandle(process);
        elevated
    }
}

unsafe fn process_token_is_elevated(process: HANDLE) -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// 以管理员权限启动当前 exe（ShellExecuteW runas，UAC 确认后立即返回，
/// 不等待新进程）。返回 >32 表示已成功递交系统启动。
pub fn relaunch_elevated(arguments: &str) -> Result<(), AppError> {
    let exe = std::env::current_exe()?;
    let exe = exe.as_os_str().to_string_lossy().to_string();
    unsafe {
        // to_wide 产物自带 NUL 终止符，可直接作 PCWSTR
        let verb = to_wide("runas");
        let file = to_wide(&exe);
        let params = to_wide(arguments);
        let instance = ShellExecuteW(
            None,
            PCWSTR::from_raw(verb.as_ptr()),
            PCWSTR::from_raw(file.as_ptr()),
            PCWSTR::from_raw(params.as_ptr()),
            None,
            SW_SHOWNORMAL,
        );
        if (instance.0 as usize) <= 32 {
            return Err(AppError::Message(format!(
                "系统拒绝了提权启动请求（代码 {code}）",
                code = instance.0 as usize
            )));
        }
    }
    Ok(())
}

/// 提权运行外部程序并等待其退出，返回退出码。用于设置项经 UAC 重入
/// 自身完成计划任务注册（同步等待，调用方应放后台线程）。
pub fn run_elevated_and_wait(executable: &str, arguments: &str) -> Result<i32, AppError> {
    unsafe {
        let verb = to_wide("runas");
        let file = to_wide(executable);
        let params = to_wide(arguments);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR::from_raw(verb.as_ptr()),
            lpFile: PCWSTR::from_raw(file.as_ptr()),
            lpParameters: PCWSTR::from_raw(params.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        if let Err(error) = ShellExecuteExW(&mut info) {
            return Err(cancelled_or_error(error));
        }
        let process = info.hProcess;
        if process.is_invalid() {
            return Err(AppError::Message("提权启动未返回进程句柄".to_string()));
        }
        let _ = windows::Win32::System::Threading::WaitForSingleObject(process, u32::MAX);
        let mut code: u32 = 0;
        let result = windows::Win32::System::Threading::GetExitCodeProcess(process, &mut code);
        let _ = CloseHandle(process);
        result.map(|_| code as i32)?;
        Ok(code as i32)
    }
}

/// UAC 取消转成更友好的错误文案
fn cancelled_or_error(error: windows::core::Error) -> AppError {
    if error.code().0 as u32 & 0xFFFF == ERROR_CANCELLED {
        AppError::Message("已在 UAC 确认框中取消".to_string())
    } else {
        AppError::from(error)
    }
}

#[cfg(test)]
mod tests {
    use super::is_window_process_elevated;

    #[test]
    fn invalid_hwnd_is_not_elevated() {
        assert!(!is_window_process_elevated(0));
        assert!(!is_window_process_elevated(-1));
    }
}
