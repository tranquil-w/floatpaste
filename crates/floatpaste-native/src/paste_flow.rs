//! 速贴与搜索共用的上屏执行流；窗口模块保留各自的会话与隐藏实现。
//!
//! 顺序：捕获快照 → 趁属主窗口存活发布 → 隐藏来源 → 后台等待 90ms
//! → 恢复目标焦点 → 等待 60ms 并核验序列号后注入 → 调度恢复 → mark_used。
//! 发布失败保留来源窗口；仅写剪贴板时不隐藏、不注入。等待不占串行门。

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use floatpaste_core::domain::clip_item::{ClipItemDetail, PasteMode, PasteOption};
use floatpaste_core::domain::error::AppError;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::clipboard_formats::clipboard_sequence;
use floatpaste_core::platform::windows::elevation;
use floatpaste_core::platform::windows::shell_open;
use floatpaste_core::services::paste_support::{self, InjectionOutcome};
use tracing::{info, warn};

use crate::app::App;
use crate::app_state::TargetSession;
use crate::{picker, search};

const RESTORE_DELAY: Duration = Duration::from_millis(90);
const INJECT_DELAY: Duration = Duration::from_millis(60);
static NEXT_PASTE_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteSource {
    Picker,
    Search,
}

#[derive(Debug, Clone, Copy)]
enum PasteOutcome {
    Injected,
    AdminPublished,
    ClipboardChanged,
    Failed(&'static str),
}

fn paste_target_requires_elevation(target_window_hwnd: Option<isize>) -> bool {
    let Some(target_hwnd) = target_window_hwnd else {
        return false;
    };
    !elevation::is_current_process_elevated() && elevation::is_window_process_elevated(target_hwnd)
}

fn notify_admin_target_once(app: &App, restore_clipboard: bool) {
    if app.state.mark_admin_target_notified() {
        crate::tray::notify_admin_target(restore_clipboard);
    }
}

/// 「打开所在文件夹」：不是剪贴板形态，不经发布与注入。在资源管理器中
/// 定位条目引用的第一个仍存在的路径（多选跨目录没有单一所在目录）；
/// 定位成功才收来源窗口，失败保留窗口并把错误交回调用方提示。资源
/// 管理器接管前台，不还原速贴呼出前的目标窗口。
fn reveal_item_location(
    app: &App,
    detail: &ClipItemDetail,
    source: PasteSource,
) -> Result<(), AppError> {
    let existing = detail
        .file_paths
        .iter()
        .find(|path| std::path::Path::new(path).exists())
        .ok_or_else(|| AppError::Message("文件已被移动或删除".into()))?;
    shell_open::reveal_in_explorer(existing)?;
    match source {
        PasteSource::Picker if app.state.is_picker_active() => picker::hide(app, false),
        PasteSource::Search => search::hide(app, false),
        _ => {}
    }
    Ok(())
}

/// 窗口层失败提示的统一文案：「打开所在文件夹」失败不是上屏失败，
/// 前缀按动作名区分，两窗口共用
pub(crate) fn failure_notice(mode: PasteMode, error: &AppError) -> String {
    let action = if mode == PasteMode::RevealInFolder {
        "打开所在文件夹失败"
    } else {
        "上屏失败"
    };
    format!("{action}：{error}")
}

pub(crate) fn paste_item(
    app: &App,
    id: &str,
    option: PasteOption,
    source: PasteSource,
) -> Result<(), AppError> {
    let detail = app.core().repository.get_item_for_paste(id)?;
    if option.mode == PasteMode::RevealInFolder {
        return reveal_item_location(app, &detail, source);
    }
    let (session, owner_hwnd) = match source {
        PasteSource::Picker => (
            app.state.picker_session(),
            app.state.picker_hwnd.load(Ordering::SeqCst),
        ),
        PasteSource::Search => {
            let session = app.state.search_session();
            (
                TargetSession {
                    session_id: session.session_id,
                    target_window_hwnd: session.target_window_hwnd,
                    target_focus_hwnd: None,
                },
                app.state.search_hwnd.load(Ordering::SeqCst),
            )
        }
    };
    let owner_hwnd = (owner_hwnd != 0).then_some(owner_hwnd);
    let admin_target =
        option.paste_to_target && paste_target_requires_elevation(session.target_window_hwnd);
    let operation_id = NEXT_PASTE_OPERATION_ID.fetch_add(1, Ordering::Relaxed);
    let operation_gate = paste_support::lock_paste_operation()?;
    let previous_clipboard = paste_support::capture_snapshot_if_needed(&option)?;
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
    if !option.paste_to_target {
        app.core().repository.mark_used(id)?;
        return Ok(());
    }

    // 发布成功后才结束来源会话。搜索先恢复目标再停屏，速贴由后台恢复。
    match source {
        PasteSource::Picker if app.state.is_picker_active() => picker::hide(app, false),
        PasteSource::Search => search::hide(app, true),
        _ => {}
    }

    let core = app.core().clone();
    let app_for_result = app.clone();
    let id = id.to_string();
    let session_id = session.session_id;
    let failure_hint = paste_support::paste_failure_hint(option.restore_clipboard_after_paste);
    thread::spawn(move || {
        let started = std::time::Instant::now();
        let outcome = match session.target_window_hwnd {
            Some(target_hwnd) => {
                thread::sleep(RESTORE_DELAY);
                if core.is_quitting() {
                    info!(
                        ?source,
                        session_id,
                        operation_id,
                        outcome = "quitting",
                        "退出时取消上屏"
                    );
                    return;
                }
                if clipboard_sequence() != written_sequence {
                    info!(
                        ?source,
                        session_id,
                        operation_id,
                        outcome = "cancelled",
                        "剪贴板已变化，取消上屏"
                    );
                    return;
                }
                if ActiveAppResolver::restore_foreground_window_with_focus(
                    target_hwnd,
                    session.target_focus_hwnd,
                ) {
                    // 速贴的目标可能是挂起的搜索窗口，须回到事件循环恢复输入。
                    if source == PasteSource::Picker
                        && target_hwnd == app_for_result.state.search_hwnd.load(Ordering::SeqCst)
                    {
                        let app_for_resume = app_for_result.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            search::resume_input(&app_for_resume);
                        });
                    }
                    if admin_target {
                        PasteOutcome::AdminPublished
                    } else {
                        thread::sleep(INJECT_DELAY);
                        if core.is_quitting() {
                            info!(
                                ?source,
                                session_id,
                                operation_id,
                                outcome = "quitting",
                                "退出时取消上屏"
                            );
                            return;
                        }
                        match paste_support::trigger_ctrl_v_if_current(
                            written_sequence,
                            session_id,
                            operation_id,
                        ) {
                            Ok(InjectionOutcome::Injected) => PasteOutcome::Injected,
                            Ok(InjectionOutcome::ClipboardChanged) => {
                                PasteOutcome::ClipboardChanged
                            }
                            Err(error) => {
                                warn!(?source, session_id, operation_id, error = %error, "上屏注入失败");
                                PasteOutcome::Failed("未能自动粘贴。")
                            }
                        }
                    }
                } else {
                    PasteOutcome::Failed("未能切回目标窗口。")
                }
            }
            None => PasteOutcome::Failed("未找到需要粘贴的窗口。"),
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
                warn!(?source, session_id, operation_id, error = %error, "上屏后安排剪贴板恢复失败");
            }
        }

        // 保留原有活动时间契约：恢复目标前取消不更新；进入后续尝试则更新，
        // 即使目标恢复/注入失败或在注入前最后一次核验时发现新复制。
        if let Err(error) = core.repository.mark_used(&id) {
            warn!(?source, session_id, operation_id, error = %error, "上屏后更新条目使用时间失败");
        }

        let (status, failure) = match outcome {
            PasteOutcome::Injected => ("injected", None),
            PasteOutcome::AdminPublished => ("published_admin", None),
            PasteOutcome::ClipboardChanged => ("cancelled", None),
            PasteOutcome::Failed(reason) => ("failed", Some(reason)),
        };
        if let Some(reason) = failure {
            warn!(
                ?source,
                session_id,
                operation_id,
                outcome = status,
                reason,
                elapsed_ms = started.elapsed().as_millis(),
                "上屏流程失败"
            );
        } else {
            info!(
                ?source,
                session_id,
                operation_id,
                outcome = status,
                elapsed_ms = started.elapsed().as_millis(),
                "上屏流程完成"
            );
        }
        if let Err(error) = slint::invoke_from_event_loop(move || {
            if app_for_result.core().is_quitting() {
                return;
            }
            if let Some(message) = failure.filter(|_| !admin_target) {
                crate::tray::notify_failure(&format!("{message}{failure_hint}"));
            }
            if source == PasteSource::Picker {
                picker::notify_pasted(&app_for_result);
            }
        }) {
            warn!(?source, session_id, operation_id, error = %error, "上屏结果回传 UI 失败");
        }
    });

    Ok(())
}
