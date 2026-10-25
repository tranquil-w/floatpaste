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

/// E2E 隔离钩子：FLOATPASTE_INSTANCE_ID 给互斥量与唤醒事件名追加后缀，
/// 让被测实例与日常实例在同一登录会话内共存。值仅允许字母数字与连字符
/// （防异常字符进入内核对象名），空值或缺省时名称保持不变。
fn instance_suffix() -> Option<String> {
    match std::env::var("FLOATPASTE_INSTANCE_ID") {
        Ok(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
            Some(id)
        }
        _ => None,
    }
}

/// 内核对象名后缀拼接的唯一格式来源：listen 侧与 signal 侧必须拼出
/// 同名，格式漂移会让测试侧打不开事件、收尾退化到强杀兜底
fn with_suffix(base: &str, suffix: &str) -> String {
    format!("{base}.{suffix}")
}

fn namespaced(base: &str) -> String {
    match instance_suffix() {
        Some(id) => with_suffix(base, &id),
        None => base.to_string(),
    }
}

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
    let mutex_name = to_wide(&namespaced(SINGLE_INSTANCE_MUTEX_NAME));
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

/// 等待单实例互斥量被释放（提权重启场景：旧实例在 ShellExecuteW 成功
/// 返回后立即退出，新实例须等它释放后再正常获取，避免被当作二次启动
/// 唤醒旧实例而自杀）。超时返回 false（调用方照常继续，让正常单实例
/// 逻辑兜底）。
pub fn wait_mutex_release(timeout: Duration) -> bool {
    let name = to_wide(&namespaced(SINGLE_INSTANCE_MUTEX_NAME));
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let handle = unsafe { CreateMutexW(None, false, PCWSTR::from_raw(name.as_ptr())) };
        if let Ok(handle) = handle {
            let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            let _ = unsafe { CloseHandle(handle) };
            if !exists {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

/* ───────────────── 命名事件（唤醒 / 测试钩子共用骨架） ───────────────── */

/// 装载 auto-reset 命名事件的监听线程，每次 SetEvent 触发一次 on_signal
/// （监听线程上触发）。HANDLE 裸指针非 Send/Sync：用 newtype 携带
/// （Win32 句柄跨线程等待是安全的）
fn spawn_named_event_listener(
    name: String,
    on_signal: impl Fn() + Send + 'static,
) -> Result<(), AppError> {
    use std::sync::Arc;
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

    let wide = to_wide(&name);
    let event = unsafe { CreateEventW(None, false, false, PCWSTR::from_raw(wide.as_ptr()))? };

    struct SendHandle(windows::Win32::Foundation::HANDLE);
    unsafe impl Send for SendHandle {}
    unsafe impl Sync for SendHandle {}
    let event = Arc::new(SendHandle(event));

    thread::spawn(move || loop {
        let wait = unsafe { WaitForSingleObject(event.0, INFINITE) };
        if wait != windows::Win32::Foundation::WAIT_OBJECT_0 {
            break;
        }
        on_signal();
    });

    Ok(())
}

/// 向命名事件发一次信号（事件不存在返回 false）
fn set_named_event(name: &str) -> bool {
    use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};

    let wide = to_wide(name);
    let Ok(event) =
        (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR::from_raw(wide.as_ptr())) })
    else {
        return false;
    };
    let signaled = unsafe { SetEvent(event) }.is_ok();
    let _ = unsafe { CloseHandle(event) };
    signaled
}

/* ───────────────── 跨进程唤醒事件 ───────────────── */

/// 二次启动时通过命名事件通知首个实例（首个实例据此打开速贴会话，
/// 比跨进程 SetForegroundWindow 多一步：键盘/鼠标会话同步激活）。
const WAKE_EVENT_NAME: &str = "Local\\FloatPaste.Wake";

/// 首个实例：装载唤醒事件并监听，收到信号时调用 on_wake（监听线程上触发）
pub fn listen_wake(on_wake: impl Fn() + Send + 'static) -> Result<(), AppError> {
    spawn_named_event_listener(namespaced(WAKE_EVENT_NAME), on_wake)
}

/// 二次启动实例：向首个实例发送唤醒信号
pub fn signal_wake_event() -> bool {
    set_named_event(&namespaced(WAKE_EVENT_NAME))
}

/* ───────────────── E2E 优雅退出事件 ───────────────── */

/// E2E 收尾钩子：仅当 FLOATPASTE_INSTANCE_ID 设置时装载（生产实例不装载）。
/// 测试收尾经命名事件请求被测实例退出，走与托盘退出相同的正常收尾路径。
/// 不用强杀：被测实例若正处于剪贴板打开临界区，进程死亡会把系统剪贴板
/// 锁死（OpenClipboard 持续 ERROR_ACCESS_DENIED，实测无法自愈，只能重启
/// explorer 恢复）。
const TEST_EXIT_EVENT_NAME: &str = "Local\\FloatPaste.TestExit";

/// 被测实例侧：装载退出事件监听（未设置隔离后缀时为空操作）
pub fn listen_test_exit(on_exit: impl Fn() + Send + 'static) -> Result<(), AppError> {
    if instance_suffix().is_none() {
        return Ok(());
    }
    spawn_named_event_listener(namespaced(TEST_EXIT_EVENT_NAME), on_exit)
}

/// 测试侧：向指定实例发送退出信号（instance_id 须与被测实例的隔离后缀一致）
pub fn signal_test_exit(instance_id: &str) -> bool {
    set_named_event(&with_suffix(TEST_EXIT_EVENT_NAME, instance_id))
}

/* ───────────────── E2E 托盘命令事件 ───────────────── */

/// E2E 驱动钩子：仅当 FLOATPASTE_INSTANCE_ID 设置时装载（生产实例不装载）。
/// 托盘菜单是模态 TrackPopupMenu 弹出，点击无法可靠自动化（不使用输入
/// 注入），测试经命名事件请求被测实例执行与菜单「切换监听」相同的处理
/// 路径（翻转设置落库 + 运行时联动），覆盖除模态菜单点击本身外的全链路。
const TEST_TOGGLE_MONITORING_EVENT_NAME: &str = "Local\\FloatPaste.TestToggleMonitoring";

/// 被测实例侧：装载切换监听事件（未设置隔离后缀时为空操作）
pub fn listen_test_toggle_monitoring(
    on_toggle: impl Fn() + Send + 'static,
) -> Result<(), AppError> {
    if instance_suffix().is_none() {
        return Ok(());
    }
    spawn_named_event_listener(namespaced(TEST_TOGGLE_MONITORING_EVENT_NAME), on_toggle)
}

/// 测试侧：向指定实例发送切换监听信号（instance_id 须与被测实例的隔离后缀一致）
pub fn signal_test_toggle_monitoring(instance_id: &str) -> bool {
    set_named_event(&with_suffix(TEST_TOGGLE_MONITORING_EVENT_NAME, instance_id))
}

/// E2E 驱动钩子：仅当 FLOATPASTE_INSTANCE_ID 设置时装载（生产实例不装载）。
/// 搜索窗的打开走全局热键或托盘菜单，两者都无法可靠自动化（不使用输入
/// 注入），测试经命名事件请求被测实例执行与菜单「打开搜索」相同的动作
/// 路径。搜索窗几何（顶边锚定伸缩）依赖真窗口与布局管线，headless 层
/// 覆盖不到，只能在真进程里断言
const TEST_OPEN_SEARCH_EVENT_NAME: &str = "Local\\FloatPaste.TestOpenSearch";

/// 被测实例侧：装载打开搜索事件（未设置隔离后缀时为空操作）
pub fn listen_test_open_search(on_open: impl Fn() + Send + 'static) -> Result<(), AppError> {
    if instance_suffix().is_none() {
        return Ok(());
    }
    spawn_named_event_listener(namespaced(TEST_OPEN_SEARCH_EVENT_NAME), on_open)
}

/// 测试侧：向指定实例发送打开搜索信号（instance_id 须与被测实例的隔离后缀一致）
pub fn signal_test_open_search(instance_id: &str) -> bool {
    set_named_event(&with_suffix(TEST_OPEN_SEARCH_EVENT_NAME, instance_id))
}

/// E2E 驱动钩子：仅当 FLOATPASTE_INSTANCE_ID 设置时装载（生产实例不装载）。
/// 搜索窗的 Esc 关闭是键盘路径，按键注入不可用（不使用输入注入），测试经
/// 命名事件请求被测实例触发 key-close 回调——与 Esc 按键经键位解析后的
/// 终点相同，仅绕过按键分发层。焦点归属（前台还原给会话目标）依赖真窗口
/// 与真实前台转移，headless 层覆盖不到
const TEST_SEARCH_ESCAPE_EVENT_NAME: &str = "Local\\FloatPaste.TestSearchEscape";

/// 被测实例侧：装载搜索窗 Esc 关闭事件（未设置隔离后缀时为空操作）
pub fn listen_test_search_escape(on_escape: impl Fn() + Send + 'static) -> Result<(), AppError> {
    if instance_suffix().is_none() {
        return Ok(());
    }
    spawn_named_event_listener(namespaced(TEST_SEARCH_ESCAPE_EVENT_NAME), on_escape)
}

/// 测试侧：向指定实例发送搜索窗 Esc 信号（instance_id 须与被测实例的隔离后缀一致）
pub fn signal_test_search_escape(instance_id: &str) -> bool {
    set_named_event(&with_suffix(TEST_SEARCH_ESCAPE_EVENT_NAME, instance_id))
}

/// E2E 驱动钩子：仅当 FLOATPASTE_INSTANCE_ID 设置时装载（生产实例不装载）。
/// 速贴收藏切换是键盘/点击路径（不使用输入注入），测试经命名事件触发
/// toggle_favorite 回调——与会话键（默认 Ctrl+D）处理后的终点相同，
/// 仅绕过按键分发层
const TEST_PICKER_FAVORITE_EVENT_NAME: &str = "Local\\FloatPaste.TestPickerFavorite";

/// 被测实例侧：装载速贴收藏切换事件（未设置隔离后缀时为空操作）
pub fn listen_test_picker_favorite(
    on_favorite: impl Fn() + Send + 'static,
) -> Result<(), AppError> {
    if instance_suffix().is_none() {
        return Ok(());
    }
    spawn_named_event_listener(namespaced(TEST_PICKER_FAVORITE_EVENT_NAME), on_favorite)
}

/// 测试侧：向指定实例发送速贴收藏信号（instance_id 须与被测实例的隔离后缀一致）
pub fn signal_test_picker_favorite(instance_id: &str) -> bool {
    set_named_event(&with_suffix(TEST_PICKER_FAVORITE_EVENT_NAME, instance_id))
}

const TEST_PICKER_DELETE_EVENT_NAME: &str = "Local\\FloatPaste.TestPickerDelete";

/// 被测实例侧：装载速贴删除事件（仅隔离实例装载，绕过按键分发）。
pub fn listen_test_picker_delete(on_delete: impl Fn() + Send + 'static) -> Result<(), AppError> {
    if instance_suffix().is_none() {
        return Ok(());
    }
    spawn_named_event_listener(namespaced(TEST_PICKER_DELETE_EVENT_NAME), on_delete)
}

/// 测试侧：向指定实例发送速贴删除信号。
pub fn signal_test_picker_delete(instance_id: &str) -> bool {
    set_named_event(&with_suffix(TEST_PICKER_DELETE_EVENT_NAME, instance_id))
}

/// E2E 驱动钩子：仅当 FLOATPASTE_INSTANCE_ID 设置时装载（生产实例不装载）。
/// 搜索窗两段式删除是键盘路径（不使用输入注入），测试经命名事件触发
/// request_delete 回调——与删除键处理后的终点相同，仅绕过按键分发层
const TEST_SEARCH_DELETE_EVENT_NAME: &str = "Local\\FloatPaste.TestSearchDelete";

/// 被测实例侧：装载搜索窗删除事件（未设置隔离后缀时为空操作）
pub fn listen_test_search_delete(on_delete: impl Fn() + Send + 'static) -> Result<(), AppError> {
    if instance_suffix().is_none() {
        return Ok(());
    }
    spawn_named_event_listener(namespaced(TEST_SEARCH_DELETE_EVENT_NAME), on_delete)
}

/// 测试侧：向指定实例发送搜索删除信号（instance_id 须与被测实例的隔离后缀一致）
pub fn signal_test_search_delete(instance_id: &str) -> bool {
    set_named_event(&with_suffix(TEST_SEARCH_DELETE_EVENT_NAME, instance_id))
}
