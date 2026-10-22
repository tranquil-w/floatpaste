//! 上屏执行流：对齐原版 PasteExecutor::paste_item 的顺序语义。
//!
//! 关键顺序（不可调整）：
//! 1. 捕获剪贴板快照（若需恢复）；
//! 2. 趁速贴窗口仍存活时写入剪贴板（OpenClipboard 需要存活属主窗口）；
//! 3. 结束会话（卸键盘/鼠标钩子）并隐藏速贴（不恢复目标焦点）；
//! 4. 后台线程 sleep 90ms → 恢复目标窗口前台与焦点 → sleep 60ms → 注入 Ctrl+V；
//! 5. 调度 550ms 后恢复原剪贴板内容；
//! 6. mark_used 让条目在活动排序中置顶。
//!
//! 时序等待（90+60ms）在后台线程执行，不阻塞事件循环（原版由 Tauri
//! 命令线程承担同样的等待）；结果消息经 invoke_from_event_loop 回写。

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use floatpaste_core::domain::clip_item::PasteOption;
use floatpaste_core::domain::error::AppError;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::elevation;
use floatpaste_core::platform::windows::{mouse_monitor, session_keyboard};
use floatpaste_core::services::paste_support;
use tracing::{info, warn};

use crate::picker::{self, App};
use crate::search;

const RESTORE_DELAY: Duration = Duration::from_millis(90);
const INJECT_DELAY: Duration = Duration::from_millis(60);
static NEXT_PASTE_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_paste_operation_id() -> u64 {
    NEXT_PASTE_OPERATION_ID.fetch_add(1, Ordering::Relaxed)
}

/// 回贴目标是否需要提权才能送达：目标窗口以管理员运行且当前进程未提权
/// （UIPI 会把未提权进程注入的按键对提权窗口静默丢弃，自动回贴必然无效）
pub fn paste_target_requires_elevation(target_window_hwnd: Option<isize>) -> bool {
    let Some(target_hwnd) = target_window_hwnd else {
        return false;
    };
    !elevation::is_current_process_elevated() && elevation::is_window_process_elevated(target_hwnd)
}

/// 管理员目标提示节流：托盘气泡每进程只发一次，不重复打扰
pub fn notify_admin_target_once(app: &App, restore_clipboard: bool) {
    if app.state.mark_admin_target_notified() {
        crate::tray::notify_admin_target(restore_clipboard);
    }
}

pub fn paste_item(app: &App, id: &str, option: PasteOption) -> Result<(), AppError> {
    let detail = app.core().repository.get_item_for_paste(id)?;
    let session = app.state.picker_session();

    // 管理员目标：照常写入剪贴板并还原目标焦点（用户可手动 Ctrl+V），
    // 仅跳过必然无效的按键注入，经托盘气泡一次性说明
    let admin_target =
        option.paste_to_target && paste_target_requires_elevation(session.target_window_hwnd);
    let operation_id = next_paste_operation_id();
    let operation_gate = paste_support::lock_paste_operation()?;
    let previous_clipboard = paste_support::capture_snapshot_if_needed(&option)?;

    // 剪贴板属主：优先速贴窗口（此刻仍存活/可见）
    let owner_hwnd = picker_owner_hwnd(app);

    let written_sequence = paste_support::publish_item(
        app.core(),
        &detail,
        option.mode,
        owner_hwnd,
        previous_clipboard.as_ref(),
    )?;
    drop(operation_gate);

    if admin_target {
        notify_admin_target_once(app, option.restore_clipboard_after_paste);
    }
    let failure_hint = paste_support::paste_failure_hint(option.restore_clipboard_after_paste);

    if !option.paste_to_target {
        app.core().repository.mark_used(id)?;
        return Ok(());
    }

    // 速贴活跃路径：先结束会话再隐藏（原版 unregister → hide_picker）
    if app.state.is_picker_active() {
        session_keyboard::end_session();
        mouse_monitor::end_session();
        picker::hide(app, false);
    }

    let core = app.core().clone();
    let app_for_result = app.clone();
    let id = id.to_string();
    let session_id = session.session_id;
    thread::spawn(move || {
        let started = std::time::Instant::now();
        let (success, message) = match session.target_window_hwnd {
            Some(target_hwnd) => {
                thread::sleep(RESTORE_DELAY);
                if core.is_quitting()
                    || floatpaste_core::platform::windows::clipboard_formats::clipboard_sequence()
                        != written_sequence
                {
                    return;
                }
                if ActiveAppResolver::restore_foreground_window_with_focus(
                    target_hwnd,
                    session.target_focus_hwnd,
                ) {
                    // 目标是搜索窗口：把输入焦点还给搜索框（对齐原版
                    // resume_search_input_if_target）
                    if target_hwnd
                        == app_for_result
                            .state
                            .search_hwnd
                            .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        let app_for_resume = app_for_result.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            search::resume_input(&app_for_resume);
                        });
                    }
                    if admin_target {
                        (false, "目标应用以管理员身份运行，无法自动粘贴。")
                    } else {
                        thread::sleep(INJECT_DELAY);
                        if paste_support::trigger_ctrl_v_if_current(
                            written_sequence,
                            session_id,
                            operation_id,
                        ) {
                            (true, "")
                        } else {
                            (false, "未能自动粘贴。")
                        }
                    }
                } else {
                    (false, "未能切回目标窗口。")
                }
            }
            None => (false, "未找到需要粘贴的窗口。"),
        };

        if let Some(snapshot) = previous_clipboard {
            if let Err(error) = paste_support::schedule_clipboard_restore(
                &core,
                snapshot,
                owner_hwnd,
                session_id,
                operation_id,
                written_sequence,
            ) {
                warn!(session_id, operation_id, error = %error, "速贴上屏后安排剪贴板恢复失败");
            }
        }

        if let Err(error) = core.repository.mark_used(&id) {
            warn!(session_id, operation_id, error = %error, "速贴上屏后更新条目使用时间失败");
        }

        if success {
            info!(
                operation_id,
                session_id,
                outcome = "success",
                elapsed_ms = started.elapsed().as_millis(),
                "速贴上屏流程完成"
            );
        } else {
            warn!(
                operation_id,
                session_id,
                outcome = "failed",
                reason = message,
                elapsed_ms = started.elapsed().as_millis(),
                "速贴上屏流程失败"
            );
        }

        if let Err(error) = slint::invoke_from_event_loop(move || {
            if !success && !admin_target {
                crate::tray::notify_failure(&format!("{message}{failure_hint}"));
            }
            picker::notify_pasted(&app_for_result);
        }) {
            warn!(session_id, operation_id, error = %error, "速贴上屏结果回传 UI 失败");
        }
    });

    Ok(())
}

fn picker_owner_hwnd(app: &App) -> Option<isize> {
    let hwnd = app
        .state
        .picker_hwnd
        .load(std::sync::atomic::Ordering::SeqCst);
    (hwnd != 0).then_some(hwnd)
}
