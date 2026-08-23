use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use tauri::{AppHandle, Emitter};
use tracing::{debug, error, info, warn};
use windows::core::{w, Error as WindowsError, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, KillTimer, PostThreadMessageW, RegisterClassW, SetTimer, SetWindowLongPtrW,
    TranslateMessage, UnregisterClassW, CREATESTRUCTW, GWLP_USERDATA, HWND_MESSAGE, MSG,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_DESTROY, WM_NCCREATE, WM_QUIT, WM_TIMER,
    WNDCLASSW,
};

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::{ClipsChangedPayload, CLIPS_CHANGED_EVENT},
    },
    platform::windows::{
        active_app::ActiveAppResolver,
        clipboard_error::{map_clipboard_error, should_retry_clipboard_read},
        file_clipboard::read_file_paths_from_clipboard,
        image_clipboard::read_image_from_clipboard,
    },
    services::{history_service::HistoryService, normalize_service::analyze_file_paths},
};

/// message-only 窗口类名：不显示、不占任务栏，仅接收 `WM_CLIPBOARDUPDATE`。
const CLIPBOARD_MONITOR_CLASS_NAME: PCWSTR = w!("FloatPasteClipboardMonitor");
/// 剪贴板被其他程序占用等临时失败时的重试间隔，对齐原轮询节奏。
const CLIPBOARD_RETRY_INTERVAL_MS: u32 = 800;
/// 单次剪贴板变化的全部尝试次数（首次 + 重试），超过后放弃，等待下一次复制。
const CLIPBOARD_MAX_ATTEMPTS: u32 = 10;
const CLIPBOARD_RETRY_TIMER_ID: usize = 1;

static MONITOR_THREAD_ID: AtomicU32 = AtomicU32::new(0);

struct MonitorContext {
    app: AppHandle,
    state: AppState,
    retry_count: u32,
}

pub struct ClipboardMonitor;

impl ClipboardMonitor {
    pub fn start(app: AppHandle, state: AppState) -> Result<(), AppError> {
        let (ready_tx, ready_rx) = mpsc::channel();

        // 独立线程承载 message-only 窗口与消息循环：AddClipboardFormatListener 让系统在
        // 剪贴板变化时投递 WM_CLIPBOARDUPDATE，取代固定间隔轮询 GetClipboardSequenceNumber
        // 的做法（复制到入屏零延迟，空闲零唤醒）。
        thread::spawn(move || unsafe {
            let (hwnd, hinstance) = match setup_monitor_window(&app, &state) {
                Ok(handles) => handles,
                Err(error) => {
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
    app: &AppHandle,
    state: &AppState,
) -> Result<(HWND, windows::Win32::Foundation::HMODULE), WindowsError> {
    let hinstance = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(monitor_wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: CLIPBOARD_MONITOR_CLASS_NAME,
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        return Err(WindowsError::from_win32());
    }

    let context = Box::into_raw(Box::new(MonitorContext {
        app: app.clone(),
        state: state.clone(),
        retry_count: 0,
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

    // 退出窗口期不再采集，避免访问正被销毁的 webview
    if context.state.is_quitting() {
        return;
    }

    let settings = match context.state.current_settings() {
        Ok(settings) => settings,
        Err(error) => {
            warn!("读取设置失败: {error}");
            schedule_retry(hwnd, context);
            return;
        }
    };
    if settings.pause_monitoring {
        // 暂停期间的变化直接丢弃（含遗留的重试）；恢复监控后不补采暂停期间的内容
        context.retry_count = 0;
        let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
        return;
    }

    let source_app = ActiveAppResolver::current_foreground_process_name();
    match process_clipboard_change(&context.app, &context.state, source_app) {
        Ok(()) => {
            context.retry_count = 0;
            let _ = KillTimer(Some(hwnd), CLIPBOARD_RETRY_TIMER_ID);
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
    app: &AppHandle,
    state: &AppState,
    source_app: Option<String>,
) -> Result<(), AppError> {
    if let Some(file_paths) = read_file_paths_from_clipboard()? {
        let file_selection = analyze_file_paths(&file_paths);
        if let Some(detail) = HistoryService::ingest_files(
            state,
            file_paths,
            file_selection.directory_count,
            file_selection.total_size,
            source_app.clone(),
        )? {
            if let Err(error) =
                app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail))
            {
                debug!("广播文件剪贴记录变更失败: {error}");
            }
        }
        return Ok(());
    }

    if let Some(image) = read_image_from_clipboard()? {
        let prepared = state.image_storage.prepare_image(
            &image.rgba,
            image.width,
            image.height,
            image.png_bytes.as_deref(),
        )?;
        if let Some(detail) = HistoryService::ingest_image(state, prepared, source_app.clone())? {
            if let Err(error) =
                app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail))
            {
                debug!("广播图片剪贴记录变更失败: {error}");
            }
        }
        return Ok(());
    }

    let mut clipboard = Clipboard::new().map_err(map_clipboard_error)?;

    match clipboard.get_text() {
        Ok(text) => {
            if let Some(detail) = HistoryService::ingest_text(state, &text, source_app)? {
                if let Err(error) =
                    app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail))
                {
                    debug!("广播文本剪贴记录变更失败: {error}");
                }
            }
        }
        Err(error) if should_retry_clipboard_read(&error) => {
            return Err(map_clipboard_error(error));
        }
        Err(_) => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use arboard::Error as ClipboardError;
    use uuid::Uuid;

    use super::super::clipboard_error::should_retry_clipboard_read;
    use super::analyze_file_paths;
    use super::{retry_decision, RetryDecision, CLIPBOARD_MAX_ATTEMPTS};
    use crate::services::normalize_service::FileSelectionStats;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("floatpaste-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn retries_when_clipboard_is_temporarily_occupied() {
        assert!(should_retry_clipboard_read(
            &ClipboardError::ClipboardOccupied
        ));
    }

    #[test]
    fn does_not_retry_for_unsupported_or_missing_content() {
        assert!(!should_retry_clipboard_read(
            &ClipboardError::ContentNotAvailable
        ));
        assert!(!should_retry_clipboard_read(
            &ClipboardError::ConversionFailure
        ));
        assert!(!should_retry_clipboard_read(&ClipboardError::Unknown {
            description: "test".to_string(),
        }));
    }

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

    #[test]
    fn analyze_file_paths_skips_total_size_when_directories_exist() {
        let dir = temp_path("dir");
        let file = temp_path("file.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&file, b"hello").unwrap();

        let stats = analyze_file_paths(&[
            dir.to_string_lossy().to_string(),
            file.to_string_lossy().to_string(),
        ]);

        assert_eq!(
            stats,
            FileSelectionStats {
                directory_count: 1,
                total_size: None,
            }
        );

        fs::remove_file(file).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn analyze_file_paths_sums_regular_file_sizes() {
        let file_a = temp_path("a.txt");
        let file_b = temp_path("b.txt");
        fs::write(&file_a, b"hello").unwrap();
        fs::write(&file_b, b"world!").unwrap();

        let stats = analyze_file_paths(&[
            file_a.to_string_lossy().to_string(),
            file_b.to_string_lossy().to_string(),
        ]);

        assert_eq!(
            stats,
            FileSelectionStats {
                directory_count: 0,
                total_size: Some(11),
            }
        );

        fs::remove_file(file_a).unwrap();
        fs::remove_file(file_b).unwrap();
    }
}
