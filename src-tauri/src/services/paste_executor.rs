use std::{borrow::Cow, fs, thread, time::Duration};

use arboard::{Clipboard, Error as ClipboardError, ImageData};
use tauri::AppHandle;
use tracing::warn;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

use crate::{
    app_bootstrap::AppState,
    domain::{
        clip_item::{ClipItemDetail, PasteOption, PasteResult},
        error::AppError,
    },
    platform::windows::{
        active_app::ActiveAppResolver,
        file_clipboard::read_file_paths_from_clipboard,
        file_clipboard::write_file_paths_to_clipboard as write_file_paths_to_clipboard_impl,
        image_clipboard::{read_image_from_clipboard, ClipboardImageData},
    },
    services::{
        normalize_service::NormalizeService, shortcut_manager::ShortcutManager,
        window_coordinator::WindowCoordinator,
    },
};

pub struct PasteExecutor;

impl PasteExecutor {
    pub fn paste_item(
        app: &AppHandle,
        state: &AppState,
        id: &str,
        option: PasteOption,
    ) -> Result<PasteResult, AppError> {
        let detail = state.repository.get_item_detail(id)?;
        let previous_clipboard = if option.restore_clipboard_after_paste && option.paste_to_target {
            Some(capture_clipboard_snapshot()?)
        } else {
            None
        };
        let mut clipboard =
            Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;

        write_item_to_clipboard(state, &mut clipboard, &detail)?;

        let clip_type_label = clip_type_label(&detail.r#type);

        // 资料库窗口调用时仅写入剪贴板，不恢复目标窗口、不发送 Ctrl+V
        if !option.paste_to_target {
            state.repository.mark_used(id)?;
            return Ok(PasteResult {
                success: true,
                code: format!("{}_clipboard_only", detail.r#type),
                message: format!("已将{clip_type_label}写入系统剪贴板，可手动粘贴到目标位置。"),
            });
        }

        let (target_hwnd, target_focus_hwnd) = if state.is_picker_active() {
            let session = state.picker_session()?;
            ShortcutManager::unregister_picker_session_shortcuts(app);
            WindowCoordinator::hide_picker(app)?;
            (session.target_window_hwnd, session.target_focus_hwnd)
        } else if state.is_search_active() {
            let hwnd = state
                .search_session()?
                .and_then(|session| session.target_window_hwnd);
            WindowCoordinator::hide_search_and_restore_target(app, state)?;
            (hwnd, None)
        } else {
            let session = state.picker_session()?;
            (session.target_window_hwnd, session.target_focus_hwnd)
        };

        let paste_result = if let Some(target_hwnd) = target_hwnd {
            thread::sleep(Duration::from_millis(90));
            if ActiveAppResolver::restore_foreground_window_with_focus(
                target_hwnd,
                target_focus_hwnd,
            ) {
                WindowCoordinator::resume_search_input_if_target(app, Some(target_hwnd));
                thread::sleep(Duration::from_millis(60));
                if trigger_ctrl_v() {
                    PasteResult {
                        success: true,
                        code: format!("{}_paste_injected", detail.r#type),
                        message: format!("已将{clip_type_label}写入系统剪贴板，并回贴到目标窗口。"),
                    }
                } else {
                    PasteResult {
                        success: false,
                        code: format!("{}_paste_injection_failed", detail.r#type),
                        message: format!(
                            "已将{clip_type_label}写入系统剪贴板，但系统按键注入失败。你仍可手动执行 Ctrl+V。"
                        ),
                    }
                }
            } else {
                PasteResult {
                    success: false,
                    code: format!("{}_target_window_restore_failed", detail.r#type),
                    message: format!(
                        "已将{clip_type_label}写入系统剪贴板，但未能恢复到原目标窗口。你仍可手动执行 Ctrl+V。"
                    ),
                }
            }
        } else {
            PasteResult {
                success: false,
                code: format!("{}_target_window_missing", detail.r#type),
                message: format!(
                    "已将{clip_type_label}写入系统剪贴板，但当前没有可恢复的目标窗口句柄。你仍可手动执行 Ctrl+V。"
                ),
            }
        };

        if let Some(snapshot) = previous_clipboard {
            schedule_clipboard_restore(state.clone(), snapshot)?;
        }

        state.repository.mark_used(id)?;

        Ok(paste_result)
    }
}

#[derive(Debug, Clone)]
enum ClipboardSnapshot {
    Empty,
    Text(String),
    Image(ClipboardImageData),
    Files(Vec<String>),
}

fn capture_clipboard_snapshot() -> Result<ClipboardSnapshot, AppError> {
    if let Some(file_paths) = read_file_paths_from_clipboard()? {
        return Ok(ClipboardSnapshot::Files(file_paths));
    }

    if let Some(image) = read_image_from_clipboard()? {
        return Ok(ClipboardSnapshot::Image(image));
    }

    let mut clipboard = Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
    match clipboard.get_text() {
        Ok(text) => Ok(ClipboardSnapshot::Text(text)),
        Err(ClipboardError::ContentNotAvailable) => Ok(ClipboardSnapshot::Empty),
        Err(error) if should_retry_clipboard_read(&error) => {
            Err(AppError::Clipboard(error.to_string()))
        }
        Err(_) => Ok(ClipboardSnapshot::Empty),
    }
}

fn schedule_clipboard_restore(
    state: AppState,
    snapshot: ClipboardSnapshot,
) -> Result<(), AppError> {
    suppress_clipboard_snapshot(&state, &snapshot)?;

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(550));
        if let Err(error) = restore_clipboard_snapshot(snapshot) {
            warn!("恢复剪贴板失败: {error}");
        }
    });

    Ok(())
}

fn suppress_clipboard_snapshot(
    state: &AppState,
    snapshot: &ClipboardSnapshot,
) -> Result<(), AppError> {
    match snapshot {
        ClipboardSnapshot::Empty => Ok(()),
        ClipboardSnapshot::Text(text) => {
            if let Some(normalized) = NormalizeService::normalize_text(text, None) {
                state
                    .self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }
            Ok(())
        }
        ClipboardSnapshot::Image(image) => {
            let prepared =
                state
                    .image_storage
                    .prepare_image(&image.rgba, image.width, image.height)?;
            if let Some(normalized) = NormalizeService::normalize_image(
                None,
                Some(prepared.width),
                Some(prepared.height),
                Some(prepared.image_format),
                Some(prepared.file_size),
                Some(prepared.content_hash),
                None,
            ) {
                state
                    .self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }
            Ok(())
        }
        ClipboardSnapshot::Files(file_paths) => {
            let stats = analyze_file_paths(file_paths);
            if let Some(normalized) = NormalizeService::normalize_files(
                file_paths.clone(),
                stats.directory_count,
                stats.total_size,
                None,
            ) {
                state
                    .self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }
            Ok(())
        }
    }
}

fn restore_clipboard_snapshot(snapshot: ClipboardSnapshot) -> Result<(), AppError> {
    match snapshot {
        ClipboardSnapshot::Empty => {
            let mut clipboard =
                Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
            clipboard
                .clear()
                .map_err(|error| AppError::Clipboard(error.to_string()))
        }
        ClipboardSnapshot::Text(text) => {
            let mut clipboard =
                Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
            clipboard
                .set_text(text)
                .map_err(|error| AppError::Clipboard(error.to_string()))
        }
        ClipboardSnapshot::Image(image) => {
            let mut clipboard =
                Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
            clipboard
                .set_image(ImageData {
                    width: image.width,
                    height: image.height,
                    bytes: Cow::Owned(image.rgba),
                })
                .map_err(|error| AppError::Clipboard(error.to_string()))
        }
        ClipboardSnapshot::Files(file_paths) => write_file_paths_to_clipboard(&file_paths),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileSelectionStats {
    directory_count: i32,
    total_size: Option<i64>,
}

fn analyze_file_paths(file_paths: &[String]) -> FileSelectionStats {
    let mut total_size = 0i64;
    let mut directory_count = 0i32;
    let mut size_available = true;

    for path in file_paths {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => {
                size_available = false;
                continue;
            }
        };

        if metadata.is_dir() {
            directory_count += 1;
            size_available = false;
            continue;
        }

        let Ok(file_size) = i64::try_from(metadata.len()) else {
            size_available = false;
            continue;
        };
        total_size = match total_size.checked_add(file_size) {
            Some(total_size) => total_size,
            None => {
                size_available = false;
                continue;
            }
        };
    }

    FileSelectionStats {
        directory_count,
        total_size: if size_available {
            Some(total_size)
        } else {
            None
        },
    }
}

fn should_retry_clipboard_read(error: &ClipboardError) -> bool {
    matches!(error, ClipboardError::ClipboardOccupied)
}

fn write_item_to_clipboard(
    state: &AppState,
    clipboard: &mut Clipboard,
    detail: &ClipItemDetail,
) -> Result<(), AppError> {
    match detail.r#type.as_str() {
        "text" => {
            if let Some(normalized) = NormalizeService::normalize_text(&detail.full_text, None) {
                state
                    .self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }

            clipboard
                .set_text(detail.full_text.clone())
                .map_err(|error| AppError::Clipboard(error.to_string()))
        }
        "image" => {
            let Some(image_path) = detail.image_path.as_deref() else {
                return Err(AppError::Message(
                    "图片记录缺少可恢复的文件引用".to_string(),
                ));
            };
            let decoded = state.image_storage.load_image(image_path)?;

            state
                .self_write_guard()
                .suppress_hash(detail.hash.clone(), Duration::from_secs(3))?;

            clipboard
                .set_image(ImageData {
                    width: decoded.width,
                    height: decoded.height,
                    bytes: Cow::Owned(decoded.rgba),
                })
                .map_err(|error| AppError::Clipboard(error.to_string()))
        }
        "file" => {
            if detail.file_paths.is_empty() {
                return Err(AppError::Message("文件记录缺少文件路径".to_string()));
            }

            if let Some(normalized) = NormalizeService::normalize_files(
                detail.file_paths.clone(),
                detail.directory_count,
                detail.total_size,
                None,
            ) {
                state
                    .self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }

            write_file_paths_to_clipboard(&detail.file_paths)
        }
        other => Err(AppError::Message(format!("暂不支持 {other} 类型的写回"))),
    }
}

fn write_file_paths_to_clipboard(file_paths: &[String]) -> Result<(), AppError> {
    write_file_paths_to_clipboard_impl(file_paths)
}

fn clip_type_label(value: &str) -> &'static str {
    match value {
        "text" => "文本内容",
        "image" => "图片内容",
        "file" => "文件列表",
        _ => "剪贴内容",
    }
}

fn trigger_ctrl_v() -> bool {
    unsafe {
        let mut inputs: [INPUT; 4] = std::mem::zeroed();
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki.wVk = VK_CONTROL;
        inputs[0].Anonymous.ki.dwFlags = KEYBD_EVENT_FLAGS(0);

        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki.wVk = VK_V;
        inputs[1].Anonymous.ki.dwFlags = KEYBD_EVENT_FLAGS(0);

        inputs[2].r#type = INPUT_KEYBOARD;
        inputs[2].Anonymous.ki.wVk = VK_V;
        inputs[2].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        inputs[3].r#type = INPUT_KEYBOARD;
        inputs[3].Anonymous.ki.wVk = VK_CONTROL;
        inputs[3].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent != 4 {
            warn!("SendInput 只发送了 {sent} 个键盘事件");
        }
        sent == 4
    }
}
