use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use tracing::{debug, error, info, warn};
use windows::core::{w, Error as WindowsError, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, KillTimer, PostThreadMessageW, RegisterClassW, SetTimer, SetWindowLongPtrW,
    TranslateMessage, UnregisterClassW, CREATESTRUCTW, GWLP_USERDATA, HWND_MESSAGE, MSG,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_DESTROY, WM_NCCREATE, WM_QUIT, WM_TIMER,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT,
    WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

use crate::{
    domain::{clip_item::ClipItemDetail, error::AppError},
    platform::windows::{
        active_app::ActiveAppResolver,
        clipboard_formats::{clipboard_sequence, ClipboardSnapshot},
    },
    services::{history_service::HistoryService, normalize_service::analyze_file_paths},
    state::CoreState,
};

/// message-only 窗口类名：不显示、不占任务栏，仅接收 `WM_CLIPBOARDUPDATE`。
const CLIPBOARD_MONITOR_CLASS_NAME: PCWSTR = w!("FloatPasteClipboardMonitor");
/// 剪贴板被其他程序占用等临时失败时的重试间隔，对齐原轮询节奏。
const CLIPBOARD_RETRY_INTERVAL_MS: u32 = 800;
/// 单次剪贴板变化的全部尝试次数（首次 + 重试），超过后放弃，等待下一次复制。
const CLIPBOARD_MAX_ATTEMPTS: u32 = 10;
const CLIPBOARD_RETRY_TIMER_ID: usize = 1;

static MONITOR_THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// 剪贴板录入回调：每次成功入库（新增或置顶）后以最新详情调用。
/// 在监听线程上执行，实现必须自身保证线程安全并尽快返回。
pub type ClipUpsertSink = Arc<dyn Fn(ClipItemDetail) + Send + Sync>;

struct MonitorContext {
    core: CoreState,
    on_upsert: ClipUpsertSink,
    retry_count: u32,
    on_session_changed: Arc<dyn Fn(bool) + Send + Sync>,
}

pub struct ClipboardMonitor;

impl ClipboardMonitor {
    pub fn start(
        core: CoreState,
        on_upsert: ClipUpsertSink,
        on_session_changed: Arc<dyn Fn(bool) + Send + Sync>,
    ) -> Result<(), AppError> {
        let (ready_tx, ready_rx) = mpsc::channel();

        // 独立线程承载 message-only 窗口与消息循环：AddClipboardFormatListener 让系统在
        // 剪贴板变化时投递 WM_CLIPBOARDUPDATE，取代固定间隔轮询 GetClipboardSequenceNumber
        // 的做法（复制到入屏零延迟，空闲零唤醒）。
        thread::spawn(move || unsafe {
            let (hwnd, hinstance) =
                match setup_monitor_window(&core, &on_upsert, on_session_changed) {
                    Ok(handles) => handles,
                    Err(error) => {
                        let _ = core.set_session_locked(true);
                        error!("初始化剪贴板监听窗口失败: {error}");
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };

            MONITOR_THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
            let _ = ready_tx.send(Ok(()));
            info!("剪贴板监听已就绪（事件驱动）");

            // 注册后主动核对一次当前剪贴板，保持轮询版"启动即采集现状"的行为；
            // 与历史重复的内容由去重服务拦截。
            handle_clipboard_update(hwnd);

            let mut message = MSG::default();
            loop {
                let result = GetMessageW(&mut message, None, 0, 0);
                if result.0 <= 0 {
                    // 0 = 收到 WM_QUIT 正常退出；-1 = 消息循环自身出错
                    if result.0 == -1 {
                        error!("剪贴板监听消息循环异常退出");
                    }
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            // 收尾：移除监听并销毁窗口（WM_DESTROY 内回收上下文），再注销窗口类，
            // 避免残留已注册的窗口类。
            let _ = RemoveClipboardFormatListener(hwnd);
            let _ = WTSUnRegisterSessionNotification(hwnd);
            let _ = DestroyWindow(hwnd);
            let _ = UnregisterClassW(CLIPBOARD_MONITOR_CLASS_NAME, Some(hinstance.into()));
            MONITOR_THREAD_ID.store(0, Ordering::SeqCst);
        });

        // 同步等待初始化结果，失败尽早暴露给启动流程
        let init_result = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| AppError::Message("等待剪贴板监听初始化超时".to_string()))?;
        init_result.map_err(|error| AppError::Message(error.to_string()))
    }

    /// 请求监听线程退出并释放窗口，幂等；供退出清理路径调用。
    pub fn stop() {
        let thread_id = MONITOR_THREAD_ID.swap(0, Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// 创建 message-only 窗口并注册剪贴板监听；失败时回收上下文与窗口类。
unsafe fn setup_monitor_window(
    core: &CoreState,
    on_upsert: &ClipUpsertSink,
    on_session_changed: Arc<dyn Fn(bool) + Send + Sync>,
) -> Result<(HWND, windows::Win32::Foundation::HMODULE), WindowsError> {
    let hinstance = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(monitor_wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: CLIPBOARD_MONITOR_CLASS_NAME,
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        return Err(WindowsError::from_thread());
    }

    let context = Box::into_raw(Box::new(MonitorContext {
        core: core.clone(),
        on_upsert: Arc::clone(on_upsert),
        retry_count: 0,
        on_session_changed,
    }));
    let hwnd = match CreateWindowExW(
        WINDOW_EX_STYLE(0),
        CLIPBOARD_MONITOR_CLASS_NAME,
        w!(""),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(hinstance.into()),
        Some(context.cast()),
    ) {
        Ok(hwnd) => hwnd,
        Err(error) => {
            // 创建失败时系统不会消费 lpParam，这里自行回收，避免泄漏
            drop(Box::from_raw(context));
            let _ = UnregisterClassW(CLIPBOARD_MONITOR_CLASS_NAME, Some(hinstance.into()));
            return Err(error);
        }
    };

    if let Err(error) = AddClipboardFormatListener(hwnd) {
        let _ = RemoveClipboardFormatListener(hwnd);
        let _ = DestroyWindow(hwnd);
        let _ = UnregisterClassW(CLIPBOARD_MONITOR_CLASS_NAME, Some(hinstance.into()));
        return Err(error);
    }

    if let Err(error) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
        let _ = core.set_session_locked(true);
        let _ = RemoveClipboardFormatListener(hwnd);
        let _ = DestroyWindow(hwnd);
        let _ = UnregisterClassW(CLIPBOARD_MONITOR_CLASS_NAME, Some(hinstance.into()));
        return Err(error);
    }
    let locked = super::session_privacy::current_session_locked().unwrap_or_else(|error| {
        warn!(error = %error, "无法确认会话状态，暂停采集和上屏直到解锁通知");
        true
    });
    let _ = core.set_session_locked(locked);

    Ok((hwnd, hinstance))
}

unsafe extern "system" fn monitor_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // 窗口创建的第一条消息：把创建参数携带的上下文挂到窗口，供后续消息取用
        WM_NCCREATE => {
            let create_struct = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create_struct.lpCreateParams as isize);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_CLIPBOARDUPDATE => {
            handle_clipboard_update(hwnd);
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            let locked = match wparam.0 as u32 {
                WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => Some(true),
                WTS_SESSION_UNLOCK => Some(false),
                WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                    Some(super::session_privacy::current_session_locked().unwrap_or(true))
                }
                _ => None,
            };
            if let Some(locked) = locked {
                let context = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut MonitorContext);
                match context.core.set_session_locked(locked) {
                    Ok(()) => (context.on_session_changed)(locked),
                    Err(error) => warn!(error = %error, "更新会话隐私状态失败"),
                }
                context.retry_count = 0;
                let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
            }
            LRESULT(0)
        }
        // SetTimer 为周期触发，入口先杀掉，是否重设由处理结果决定
        WM_TIMER if wparam.0 as usize == CLIPBOARD_RETRY_TIMER_ID => {
            let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
            handle_clipboard_update(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            release_window_context(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// 处理一次剪贴板变化：暂停监控时丢弃，临时失败时安排延迟重试。
unsafe fn handle_clipboard_update(hwnd: HWND) {
    let context_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut MonitorContext;
    if context_ptr.is_null() {
        return;
    }
    let context = &mut *context_ptr;

    // 退出期不再采集，避免访问正被销毁的资源
    if context.core.is_quitting() {
        return;
    }

    let settings = match context.core.current_settings() {
        Ok(settings) => settings,
        Err(error) => {
            warn!("读取设置失败: {error}");
            schedule_retry(hwnd, context);
            return;
        }
    };
    if settings.pause_monitoring || context.core.session_locked() {
        // 暂停期间的变化直接丢弃（含遗留的重试）；恢复监控后不补采暂停期间的内容
        context.retry_count = 0;
        let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
        return;
    }

    let source_app = ActiveAppResolver::current_foreground_process_name();
    match process_clipboard_change(&context.core, &context.on_upsert, source_app) {
        Ok(()) => {
            context.retry_count = 0;
            let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
        }
        Err(AppError::InputLimit(reason)) => {
            context.retry_count = 0;
            let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
            info!(reason, "跳过超限剪贴板内容");
        }
        Err(error) => {
            debug!("处理剪贴板变更失败，将延迟重试: {error}");
            schedule_retry(hwnd, context);
        }
    }
}

/// 安排下一轮重试；连续失败达到上限后放弃本次内容，等待下一次复制。
unsafe fn schedule_retry(hwnd: HWND, context: &mut MonitorContext) {
    context.retry_count += 1;
    match retry_decision(context.retry_count) {
        RetryDecision::Schedule => {
            let _ = SetTimer(
                Some(hwnd),
                CLIPBOARD_RETRY_TIMER_ID,
                CLIPBOARD_RETRY_INTERVAL_MS,
                None,
            );
        }
        RetryDecision::Abandon => {
            let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
            warn!("剪贴板变更连续尝试 {CLIPBOARD_MAX_ATTEMPTS} 次失败，放弃并等待下一次复制");
            context.retry_count = 0;
        }
    }
}

enum RetryDecision {
    Schedule,
    Abandon,
}

/// 已失败 `failed_attempts` 次后是否继续重试。
fn retry_decision(failed_attempts: u32) -> RetryDecision {
    if failed_attempts < CLIPBOARD_MAX_ATTEMPTS {
        RetryDecision::Schedule
    } else {
        RetryDecision::Abandon
    }
}

unsafe fn release_window_context(hwnd: HWND) {
    // 取回并清零窗口上下文，二次销毁时不会重复回收
    let previous = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    if previous != 0 {
        drop(Box::from_raw(previous as *mut MonitorContext));
    }
}

fn process_clipboard_change(
    core: &CoreState,
    on_upsert: &ClipUpsertSink,
    source_app: Option<String>,
) -> Result<(), AppError> {
    let settings = core.current_settings()?;
    let generation = core.privacy_generation();
    // 排除规则本体在 privacy_service（入库前仍会生效）；此处前置短路，
    // 避免为排除来源白白捕获多格式快照。暂停监测由调用方先行丢弃。
    if source_app.as_deref().is_some_and(|app| {
        settings
            .excluded_apps
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(app))
    }) {
        return Ok(());
    }
    let snapshot = ClipboardSnapshot::capture_for_history(&settings.excluded_apps)?;
    if snapshot.excluded_from_history {
        return Ok(());
    }
    {
        let privacy = core.privacy_read()?;
        if privacy.session_locked
            || privacy.generation != generation
            || privacy.ignored_clipboard_sequence == Some(snapshot.sequence)
        {
            return Ok(());
        }
    }
    let source_app = snapshot.source_app.clone().or(source_app);
    if let Some(file_paths) = snapshot.files.as_ref() {
        let file_selection = analyze_file_paths(file_paths);
        if clipboard_sequence() != snapshot.sequence {
            return Ok(());
        }
        if let Some(detail) = HistoryService::ingest_files_at_generation(
            core,
            file_paths.clone(),
            file_selection.directory_count,
            file_selection.total_size,
            source_app,
            Some(generation),
        )? {
            on_upsert(detail);
        }
        return Ok(());
    }
    let html_result = snapshot
        .html()
        .map(crate::services::html_format::parse_html_text);
    let html_failed = snapshot.html_available && !matches!(html_result, Some(Ok(_)));
    let html_text = html_result.and_then(Result::ok).flatten();
    let plain = snapshot.text();
    if let Some(extracted) = html_text {
        let text = plain
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or(&extracted);
        if clipboard_sequence() != snapshot.sequence {
            return Ok(());
        }
        if let Some(detail) = HistoryService::ingest_text_formats_at_generation(
            core,
            text,
            snapshot.html().map(<[u8]>::to_vec),
            source_app,
            Some(generation),
        )? {
            on_upsert(detail);
        }
        return Ok(());
    }
    if html_failed {
        if let Some(text) = plain.as_deref().filter(|text| !text.trim().is_empty()) {
            if clipboard_sequence() != snapshot.sequence {
                return Ok(());
            }
            if let Some(detail) = HistoryService::ingest_text_formats_at_generation(
                core,
                text,
                None,
                source_app,
                Some(generation),
            )? {
                on_upsert(detail);
            }
            return Ok(());
        }
    }
    if let Some(image) = snapshot.image() {
        let prepared = core.image_storage.prepare_image(
            &image.rgba,
            image.width,
            image.height,
            image.png_bytes.as_deref(),
        )?;
        if clipboard_sequence() != snapshot.sequence {
            return Ok(());
        }
        if let Some(detail) = HistoryService::ingest_image_at_generation(
            core,
            prepared,
            source_app,
            Some(generation),
        )? {
            on_upsert(detail);
        }
        return Ok(());
    }
    if let Some(text) = plain {
        if clipboard_sequence() != snapshot.sequence {
            return Ok(());
        }
        if let Some(detail) = HistoryService::ingest_text_formats_at_generation(
            core,
            &text,
            None,
            source_app,
            Some(generation),
        )? {
            on_upsert(detail);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{retry_decision, RetryDecision, CLIPBOARD_MAX_ATTEMPTS};

    #[test]
    fn schedules_retry_below_attempt_limit() {
        assert!(matches!(retry_decision(0), RetryDecision::Schedule));
        assert!(matches!(
            retry_decision(CLIPBOARD_MAX_ATTEMPTS - 1),
            RetryDecision::Schedule
        ));
    }

    #[test]
    fn abandons_retry_at_and_beyond_attempt_limit() {
        assert!(matches!(
            retry_decision(CLIPBOARD_MAX_ATTEMPTS),
            RetryDecision::Abandon
        ));
        assert!(matches!(
            retry_decision(CLIPBOARD_MAX_ATTEMPTS + 1),
            RetryDecision::Abandon
        ));
    }
}
