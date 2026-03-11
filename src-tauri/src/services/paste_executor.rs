use std::{borrow::Cow, mem::size_of, ptr::copy_nonoverlapping, thread, time::Duration};

use arboard::{Clipboard, ImageData};
use tauri::AppHandle;
use tracing::warn;
use windows::Win32::{
    Foundation::{BOOL, POINT},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        },
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT},
    },
    UI::{
        Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
        },
        Shell::DROPFILES,
    },
};

use crate::{
    app_bootstrap::AppState,
    domain::{
        clip_item::{ClipItemDetail, PasteOption, PasteResult},
        error::AppError,
    },
    platform::windows::active_app::ActiveAppResolver,
    services::{
        normalize_service::NormalizeService,
        shortcut_manager::ShortcutManager,
        window_coordinator::WindowCoordinator,
    },
};

const CF_HDROP_FORMAT: u32 = 15;

pub struct PasteExecutor;

impl PasteExecutor {
    pub fn paste_item(
        app: &AppHandle,
        state: &AppState,
        id: &str,
        option: PasteOption,
    ) -> Result<PasteResult, AppError> {
        let detail = state.repository.get_item_detail(id)?;
        let mut clipboard =
            Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
        let previous_text = clipboard.get_text().ok();

        write_item_to_clipboard(state, &mut clipboard, &detail)?;

        ShortcutManager::unregister_picker_session_shortcuts(app);
        WindowCoordinator::hide_picker(app)?;

        let clip_type_label = clip_type_label(&detail.r#type);
        let picker_session = state.picker_session()?;
        let paste_result = if let Some(target_hwnd) = picker_session.target_window_hwnd {
            thread::sleep(Duration::from_millis(90));
            if ActiveAppResolver::restore_foreground_window(target_hwnd) {
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

        if option.restore_clipboard_after_paste {
            if let Some(snapshot) = previous_text {
                if let Some(normalized) = NormalizeService::normalize_text(&snapshot, None) {
                    state
                        .self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(550));
                    if let Ok(mut restore_clipboard) = Clipboard::new() {
                        let _ = restore_clipboard.set_text(snapshot);
                    }
                });
            }
        }

        state.repository.mark_used(id)?;

        Ok(paste_result)
    }
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
                return Err(AppError::Message("图片记录缺少可恢复的文件引用".to_string()));
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

            if let Some(normalized) =
                NormalizeService::normalize_files(detail.file_paths.clone(), detail.total_size, None)
            {
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
    let mut encoded_paths = Vec::new();
    for path in file_paths {
        encoded_paths.extend(path.encode_utf16());
        encoded_paths.push(0);
    }
    encoded_paths.push(0);

    let header_size = size_of::<DROPFILES>();
    let path_bytes_len = encoded_paths.len() * size_of::<u16>();
    let total_size = header_size + path_bytes_len;

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total_size);
        if handle.is_invalid() {
            return Err(AppError::Clipboard("分配文件剪贴板内存失败".to_string()));
        }

        let memory = GlobalLock(handle);
        if memory.is_null() {
            let _ = GlobalUnlock(handle);
            return Err(AppError::Clipboard("锁定文件剪贴板内存失败".to_string()));
        }

        let header = memory.cast::<DROPFILES>();
        (*header).pFiles = header_size as u32;
        (*header).pt = POINT { x: 0, y: 0 };
        (*header).fNC = BOOL(0);
        (*header).fWide = BOOL(1);

        let path_memory = memory.cast::<u8>().add(header_size).cast::<u16>();
        copy_nonoverlapping(encoded_paths.as_ptr(), path_memory, encoded_paths.len());
        let _ = GlobalUnlock(handle);

        OpenClipboard(None).map_err(|error| AppError::Clipboard(error.to_string()))?;
        let result = (|| {
            EmptyClipboard().map_err(|error| AppError::Clipboard(error.to_string()))?;
            SetClipboardData(CF_HDROP_FORMAT, handle.into())
                .map_err(|error| AppError::Clipboard(error.to_string()))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
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
