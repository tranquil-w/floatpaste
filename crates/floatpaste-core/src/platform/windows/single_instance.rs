use std::{thread, time::Duration};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE},
        System::Threading::CreateMutexW,
    },
};

use crate::{domain::error::AppError, launch_mode::LaunchMode};

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

/// 获取单实例互斥量。已有实例时调用 `focus_existing` 唤醒既有窗口
/// （各 GUI 壳自行决定唤醒哪个窗口）；`launch_mode` 为静默启动时跳过唤醒。
/// 返回 `Ok(None)` 表示已有实例且当前进程应退出。
pub fn acquire_or_focus_existing(
    launch_mode: LaunchMode,
    focus_existing: impl Fn() -> bool,
) -> Result<Option<SingleInstanceGuard>, AppError> {
    let mutex_name = to_wide(SINGLE_INSTANCE_MUTEX_NAME);
    let handle = unsafe { CreateMutexW(None, false, PCWSTR::from_raw(mutex_name.as_ptr())) }?;

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(handle) };
        if !launch_mode.is_silent() && !focus_existing() {
            return Err(AppError::Message(
                "检测到已有实例，但唤醒现有窗口失败".to_string(),
            ));
        }
        return Ok(None);
    }

    Ok(Some(SingleInstanceGuard { handle }))
}

/// 反复尝试唤醒（窗口可能仍在创建中）；全部失败返回 false。
pub fn retry_focus_existing(try_once: impl Fn() -> bool) -> bool {
    for _ in 0..10 {
        if try_once() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }

    false
}

/* ───────────────── 跨进程唤醒事件 ───────────────── */

/// 二次启动时通过命名事件通知首个实例（首个实例据此打开速贴会话，
/// 比跨进程 SetForegroundWindow 多一步：键盘/鼠标会话同步激活）。
const WAKE_EVENT_NAME: &str = "Local\\FloatPaste.Wake";

/// 首个实例：装载唤醒事件并监听，收到信号时调用 on_wake（监听线程上触发）
pub fn listen_wake(on_wake: impl Fn() + Send + 'static) -> Result<(), AppError> {
    use std::sync::Arc;
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

    let name = to_wide(WAKE_EVENT_NAME);
    let event = unsafe { CreateEventW(None, false, false, PCWSTR::from_raw(name.as_ptr()))? };

    // HANDLE 裸指针非 Send/Sync：用 newtype 携带（Win32 句柄跨线程等待是安全的）
    struct SendHandle(windows::Win32::Foundation::HANDLE);
    unsafe impl Send for SendHandle {}
    unsafe impl Sync for SendHandle {}
    let event = Arc::new(SendHandle(event));

    thread::spawn(move || loop {
        let wait = unsafe { WaitForSingleObject(event.0, INFINITE) };
        if wait != windows::Win32::Foundation::WAIT_OBJECT_0 {
            break;
        }
        on_wake();
    });

    Ok(())
}

/// 二次启动实例：向首个实例发送唤醒信号
pub fn signal_wake_event() -> bool {
    use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};

    let name = to_wide(WAKE_EVENT_NAME);
    let Ok(event) =
        (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR::from_raw(name.as_ptr())) })
    else {
        return false;
    };
    let signaled = unsafe { SetEvent(event) }.is_ok();
    let _ = unsafe { CloseHandle(event) };
    signaled
}
