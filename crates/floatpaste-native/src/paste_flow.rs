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

use std::thread;
use std::time::Duration;

use arboard::Clipboard;

use floatpaste_core::domain::clip_item::PasteOption;
use floatpaste_core::domain::error::AppError;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::{mouse_monitor, session_keyboard};
use floatpaste_core::services::paste_support;

use crate::picker::{self, App};

const RESTORE_DELAY: Duration = Duration::from_millis(90);
const INJECT_DELAY: Duration = Duration::from_millis(60);

pub fn paste_item(app: &App, id: &str, option: PasteOption) -> Result<(), AppError> {
    let detail = app.core().repository.get_item_detail(id)?;
    let previous_clipboard = paste_support::capture_snapshot_if_needed(&option)?;
    let mut clipboard = Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;

    // 剪贴板属主：优先速贴窗口（此刻仍存活/可见）
    let owner_hwnd = {
        let hwnd = app
            .state
            .picker_hwnd
            .load(std::sync::atomic::Ordering::SeqCst);
        (hwnd != 0).then_some(hwnd)
    };

    paste_support::write_item_to_clipboard(
        app.core(),
        &mut clipboard,
        &detail,
        option.as_file,
        owner_hwnd,
    )?;

    let clip_type_label = paste_support::clip_type_label(&detail.r#type);

    if !option.paste_to_target {
        app.core().repository.mark_used(id)?;
        picker::set_outcome_message(
            app,
            true,
            &format!("已将{clip_type_label}写入系统剪贴板，可手动粘贴到目标位置。"),
        );
        return Ok(());
    }

    // 速贴活跃路径：先结束会话再隐藏（原版 unregister → hide_picker）
    let session = app.state.picker_session();
    if app.state.is_picker_active() {
        session_keyboard::end_session();
        mouse_monitor::end_session();
        picker::hide(app, false);
    }

    let core = app.core().clone();
    let app_for_result = app.clone();
    let id = id.to_string();
    thread::spawn(move || {
        let (success, message) = match session.target_window_hwnd {
            Some(target_hwnd) => {
                thread::sleep(RESTORE_DELAY);
                if core.is_quitting() {
                    return;
                }
                if ActiveAppResolver::restore_foreground_window_with_focus(
                    target_hwnd,
                    session.target_focus_hwnd,
                ) {
                    thread::sleep(INJECT_DELAY);
                    if paste_support::trigger_ctrl_v() {
                        (true, format!("已将{clip_type_label}写入系统剪贴板，并回贴到目标窗口。"))
                    } else {
                        (false, format!(
                            "已将{clip_type_label}写入系统剪贴板，但系统按键注入失败。你仍可手动执行 Ctrl+V。"
                        ))
                    }
                } else {
                    (false, format!(
                        "已将{clip_type_label}写入系统剪贴板，但未能恢复到原目标窗口。你仍可手动执行 Ctrl+V。"
                    ))
                }
            }
            None => (false, format!(
                "已将{clip_type_label}写入系统剪贴板，但当前没有可恢复的目标窗口句柄。你仍可手动执行 Ctrl+V。"
            )),
        };

        if let Some(snapshot) = previous_clipboard {
            let _ = paste_support::schedule_clipboard_restore(&core, snapshot, owner_hwnd);
        }

        let _ = core.repository.mark_used(&id);

        let _ = slint::invoke_from_event_loop(move || {
            picker::set_outcome_message(&app_for_result, success, &message);
            picker::notify_pasted(&app_for_result);
        });
    });

    Ok(())
}
