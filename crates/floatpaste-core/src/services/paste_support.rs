//! 上屏支持：把剪辑项安全写入系统剪贴板，以及粘贴后的剪贴板快照恢复。
//!
//! 这里只承载与窗口框架无关的纯流程；"隐藏哪个窗口、恢复哪个目标窗口"
//! 的编排由各 GUI 壳自行实现（壳层在写剪贴板前隐藏自身窗口，再把目标
//! 窗口句柄交给 [`restore_foreground_window_with_focus`]）。

use std::{borrow::Cow, thread, time::Duration};

use arboard::{Clipboard, Error as ClipboardError, ImageData};
use tracing::warn;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

use crate::{
    domain::{
        clip_item::{ClipItemDetail, PasteOption},
        error::AppError,
    },
    platform::windows::{
        clipboard_error::should_retry_clipboard_read,
        file_clipboard::{read_file_paths_from_clipboard, write_file_paths_to_clipboard},
        image_clipboard::{
            read_image_from_clipboard, write_image_to_clipboard, ClipboardImageData,
        },
    },
    services::normalize_service::{analyze_file_paths, NormalizeService},
    state::CoreState,
};

/// 粘贴前捕获的剪贴板快照，用于"粘贴后恢复原剪贴板"。
#[derive(Debug, Clone)]
pub enum ClipboardSnapshot {
    Empty,
    Text(String),
    Image(ClipboardImageData),
    Files(Vec<String>),
}

/// 按设置决定是否捕获快照：仅在"粘贴后恢复剪贴板"开启且需要回贴目标窗口时捕获。
pub fn capture_snapshot_if_needed(
    option: &PasteOption,
) -> Result<Option<ClipboardSnapshot>, AppError> {
    if option.restore_clipboard_after_paste && option.paste_to_target {
        Ok(Some(capture_clipboard_snapshot()?))
    } else {
        Ok(None)
    }
}

pub fn capture_clipboard_snapshot() -> Result<ClipboardSnapshot, AppError> {
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

/// 先把快照登记进自写抑制表，再在后台线程延迟恢复，避免恢复动作被当成
/// 新剪贴内容采集。
///
/// `owner_hwnd`：恢复图片内容时承载 OpenClipboard 的窗口句柄；壳层可传
/// 自己的（已隐藏但未销毁的）窗口，传 None 时图片走 arboard 兜底。
pub fn schedule_clipboard_restore(
    core: &CoreState,
    snapshot: ClipboardSnapshot,
    owner_hwnd: Option<isize>,
) -> Result<(), AppError> {
    suppress_clipboard_snapshot(core, &snapshot)?;

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(550));
        if let Err(error) = restore_clipboard_snapshot(snapshot, owner_hwnd) {
            warn!("恢复剪贴板失败: {error}");
        }
    });

    Ok(())
}

pub fn restore_clipboard_snapshot(
    snapshot: ClipboardSnapshot,
    owner_hwnd: Option<isize>,
) -> Result<(), AppError> {
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
            if let Some(owner_window) = owner_hwnd {
                write_image_to_clipboard(owner_window, &image)?;
            } else {
                let mut clipboard =
                    Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
                clipboard
                    .set_image(ImageData {
                        width: image.width,
                        height: image.height,
                        bytes: Cow::Owned(image.rgba),
                    })
                    .map_err(|error| AppError::Clipboard(error.to_string()))?;
            }
            Ok(())
        }
        ClipboardSnapshot::Files(file_paths) => write_file_paths_to_clipboard(&file_paths),
    }
}

fn suppress_clipboard_snapshot(
    core: &CoreState,
    snapshot: &ClipboardSnapshot,
) -> Result<(), AppError> {
    match snapshot {
        ClipboardSnapshot::Empty => Ok(()),
        ClipboardSnapshot::Text(text) => {
            if let Some(normalized) = NormalizeService::normalize_text(text, None) {
                core.self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }
            Ok(())
        }
        ClipboardSnapshot::Image(image) => {
            let prepared = core.image_storage.prepare_image(
                &image.rgba,
                image.width,
                image.height,
                image.png_bytes.as_deref(),
            )?;
            if let Some(normalized) = NormalizeService::normalize_image(
                None,
                Some(prepared.width),
                Some(prepared.height),
                Some(prepared.image_format),
                Some(prepared.file_size),
                Some(prepared.content_hash),
                None,
            ) {
                core.self_write_guard()
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
                core.self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }
            Ok(())
        }
    }
}

/// 把剪辑项内容写入系统剪贴板，并登记自写抑制。
///
/// `owner_hwnd`：承载 OpenClipboard 的窗口句柄（传 None 时图片走 arboard 兜底）。
/// 壳层应在调用前传自己还可见的窗口句柄，隐藏窗口后再回贴目标。
/// `as_path_text`：次级上屏形态——图片写图片文件路径文本、文件写逐行
/// 路径列表文本；文本类型无次级形态，标记被忽略
pub fn write_item_to_clipboard(
    core: &CoreState,
    clipboard: &mut Clipboard,
    detail: &ClipItemDetail,
    as_path_text: bool,
    owner_hwnd: Option<isize>,
) -> Result<(), AppError> {
    match detail.r#type.as_str() {
        "text" => {
            if let Some(normalized) = NormalizeService::normalize_text(&detail.full_text, None) {
                core.self_write_guard()
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

            if as_path_text {
                let absolute_path = core.image_storage.resolve_existing_image_path(image_path)?;
                let path_str = absolute_path.to_string_lossy().to_string();

                if let Some(normalized) = NormalizeService::normalize_text(&path_str, None) {
                    core.self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                clipboard
                    .set_text(path_str)
                    .map_err(|error| AppError::Clipboard(error.to_string()))
            } else {
                let image = core.image_storage.load_image(image_path)?;
                if let Some(owner_window) = owner_hwnd {
                    let crate::services::image_storage::DecodedImage {
                        rgba,
                        width,
                        height,
                        png_bytes,
                    } = image;
                    write_image_to_clipboard(
                        owner_window,
                        &ClipboardImageData {
                            rgba,
                            width,
                            height,
                            png_bytes: Some(png_bytes),
                        },
                    )?;
                } else {
                    clipboard
                        .set_image(ImageData {
                            width: image.width,
                            height: image.height,
                            bytes: Cow::Owned(image.rgba),
                        })
                        .map_err(|error| AppError::Clipboard(error.to_string()))?;
                }

                core.self_write_guard()
                    .suppress_hash(detail.hash.clone(), Duration::from_secs(3))?;

                Ok(())
            }
        }
        "file" => {
            if detail.file_paths.is_empty() {
                return Err(AppError::Message("文件记录缺少文件路径".to_string()));
            }

            // 次级形态「粘贴为文件路径」：路径列表作为文本上屏，不动文件本体
            if as_path_text {
                let listing = file_paths_text(&detail.file_paths);
                if let Some(normalized) = NormalizeService::normalize_text(&listing, None) {
                    core.self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                return clipboard
                    .set_text(listing)
                    .map_err(|error| AppError::Clipboard(error.to_string()));
            }

            if let Some(normalized) = NormalizeService::normalize_files(
                detail.file_paths.clone(),
                detail.directory_count,
                detail.total_size,
                None,
            ) {
                core.self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }

            write_file_paths_to_clipboard(&detail.file_paths)
        }
        other => Err(AppError::Message(format!("暂不支持 {other} 类型的写回"))),
    }
}

/// 文件条目次级上屏的文本形态：逐行路径（CRLF 分隔对齐 Windows 文本
/// 惯例，记事本等按行消费的目标应用才能正确换行）
fn file_paths_text(paths: &[String]) -> String {
    paths.join("\r\n")
}

/// 中文类型标签，用于拼装用户可见的粘贴结果消息。
pub fn clip_type_label(value: &str) -> &'static str {
    match value {
        "text" => "文本内容",
        "image" => "图片内容",
        "file" => "文件列表",
        _ => "剪贴内容",
    }
}

/// 向前台窗口注入一次 Ctrl+V；返回注入是否成功。
pub fn trigger_ctrl_v() -> bool {
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

#[cfg(test)]
mod tests {
    use super::{clip_type_label, file_paths_text};

    #[test]
    fn clip_type_label_maps_known_and_unknown_types() {
        assert_eq!(clip_type_label("text"), "文本内容");
        assert_eq!(clip_type_label("image"), "图片内容");
        assert_eq!(clip_type_label("file"), "文件列表");
        assert_eq!(clip_type_label("other"), "剪贴内容");
    }

    #[test]
    fn file_paths_text_joins_with_crlf_per_line() {
        assert_eq!(
            file_paths_text(&["C:\\a.txt".to_string(), "D:\\dir\\b.txt".to_string()]),
            "C:\\a.txt\r\nD:\\dir\\b.txt"
        );
        assert_eq!(
            file_paths_text(&["C:\\only.txt".to_string()]),
            "C:\\only.txt"
        );
        assert_eq!(file_paths_text(&[]), "");
    }
}
