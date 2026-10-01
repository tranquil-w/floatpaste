//! 系统明暗变化监听：专用线程对 Personalize 注册表键（AppsUseLightTheme
//! 所在）登记变更通知，事件等待，键内值变化即触发回调。回调在监听线程上
//! 执行，UI 侧须自行切回主线程（invoke_from_event_loop）。系统切明暗时
//! 键内多个值同翻、只改「透明效果」也会触发，调用方应重读
//! `theme::system_prefers_dark` 按实际结果行动（去重）。
//!
//! 不走 WM_SETTINGCHANGE 广播路线：广播消息只达顶层窗口（message-only
//! 窗口收不到广播），接收需常驻一个隐形顶层窗；注册表通知与
//! single_instance::listen_wake 同构（线程 + 事件等待），还能捕获无广播的
//! 注册表改动。

use std::thread;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{
    RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyW, HKEY, HKEY_CURRENT_USER,
    REG_NOTIFY_CHANGE_LAST_SET,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

use crate::domain::error::AppError;

use super::wide_string::to_wide;

/// AppsUseLightTheme（`theme::system_prefers_dark` 的数据源）所在键
const PERSONALIZE_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";

/// 监听系统明暗变化，触发时调用 `on_change`（监听线程上）。监听随进程
/// 生命周期，无退出机制（与 listen_wake 同策略）；键不存在等启动失败经
/// 返回值暴露给调用方。
pub fn listen_system_theme_change(on_change: impl Fn() + Send + 'static) -> Result<(), AppError> {
    // auto-reset 事件：每轮等待消费信号后，紧接的再登记即下一轮通知。
    // HANDLE 用 newtype+Arc 携带（Win32 句柄跨线程等待是安全的）；不能
    // 裸 move——闭包按字段路径精确捕获，会绕过 newtype 直接抓走裸 HANDLE
    struct SendHandle(HANDLE);
    unsafe impl Send for SendHandle {}
    unsafe impl Sync for SendHandle {}
    let event = unsafe { CreateEventW(None, false, false, PCWSTR::null())? };
    let event = std::sync::Arc::new(SendHandle(event));

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), AppError>>();
    let sub_key = to_wide(PERSONALIZE_KEY);

    thread::spawn(move || unsafe {
        let mut key = HKEY::default();
        // windows crate 的 RegOpenKeyW 返回 WIN32_ERROR 而非 Result
        let open_result = RegOpenKeyW(HKEY_CURRENT_USER, PCWSTR(sub_key.as_ptr()), &mut key);
        if open_result.is_err() {
            let _ = ready_tx.send(Err(AppError::Message(format!(
                "打开系统主题注册表键失败: {}",
                open_result.0
            ))));
            return;
        }
        let _ = ready_tx.send(Ok(()));

        // 登记是一次性的（一次登记一次信号），循环消耗
        while RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, Some(event.0), true)
            .is_ok()
        {
            if WaitForSingleObject(event.0, INFINITE) != WAIT_OBJECT_0 {
                break;
            }
            on_change();
        }
        // 走出循环 = 再登记失败或等待异常：留日志后停听，兜底路径仍在
        // （开窗/保存时惰性重读），不掩盖故障
        tracing::warn!("系统明暗监听退出，跟随系统模式回落为开窗/保存时重读");
        let _ = RegCloseKey(key);
        let _ = CloseHandle(event.0);
    });

    // 同步等待初始化结果，失败尽早暴露给启动流程
    ready_rx
        .recv()
        .map_err(|_| AppError::Message("主题监听线程未响应".to_string()))?
}
