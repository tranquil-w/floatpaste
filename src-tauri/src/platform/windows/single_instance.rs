use std::{thread, time::Duration};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{
            FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
        },
    },
};

use crate::{
    domain::error::AppError, launch_mode::LaunchMode,
    services::window_coordinator::SETTINGS_WINDOW_TITLE,
};

use super::wide_string::to_wide;

const SINGLE_INSTANCE_MUTEX_NAME: &str = "Local\\FloatPaste.SingleInstance";

pub struct SingleInstanceGuard {
    handle: HANDLE,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

pub fn acquire_or_focus_existing(
    launch_mode: LaunchMode,
) -> Result<Option<SingleInstanceGuard>, AppError> {
    let mutex_name = to_wide(SINGLE_INSTANCE_MUTEX_NAME);
    let handle = unsafe { CreateMutexW(None, false, PCWSTR::from_raw(mutex_name.as_ptr())) }?;

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(handle) };
        if !launch_mode.is_silent() {
            if !focus_existing_settings_window() {
                return Err(AppError::Message(
                    "检测到已有实例，但唤醒现有设置窗口失败".to_string(),
                ));
            }
        }
        return Ok(None);
    }

    Ok(Some(SingleInstanceGuard { handle }))
}

fn focus_existing_settings_window() -> bool {
    for _ in 0..10 {
        if try_focus_existing_settings_window() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }

    false
}

fn try_focus_existing_settings_window() -> bool {
    let title = to_wide(SETTINGS_WINDOW_TITLE);
    let hwnd = match unsafe { FindWindowW(None, PCWSTR::from_raw(title.as_ptr())) } {
        Ok(hwnd) => hwnd,
        Err(_) => return false,
    };
    if hwnd.0.is_null() {
        return false;
    }

    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        SetForegroundWindow(hwnd).as_bool()
    }
}
