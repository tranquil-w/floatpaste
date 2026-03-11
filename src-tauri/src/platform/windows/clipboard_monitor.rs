use std::{fs, thread, time::Duration};

use arboard::Clipboard;
use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};
use windows::{
    core::PWSTR,
    Win32::{
        System::DataExchange::{
            CloseClipboard, GetClipboardData, GetClipboardSequenceNumber,
            IsClipboardFormatAvailable, OpenClipboard,
        },
        UI::Shell::{DragQueryFileW, HDROP},
    },
};

use crate::{
    app_bootstrap::AppState,
    domain::{error::AppError, events::CLIPS_CHANGED_EVENT},
    platform::windows::active_app::ActiveAppResolver,
    services::history_service::HistoryService,
};

const CLIPBOARD_POLL_INTERVAL_MS: u64 = 800;
const CF_HDROP_FORMAT: u32 = 15;

pub struct ClipboardMonitor;

impl ClipboardMonitor {
    pub fn start(app: AppHandle, state: AppState) -> Result<(), crate::domain::error::AppError> {
        thread::spawn(move || {
            let mut last_sequence_number = 0;

            loop {
                thread::sleep(Duration::from_millis(CLIPBOARD_POLL_INTERVAL_MS));

                let settings = match state.current_settings() {
                    Ok(settings) => settings,
                    Err(error) => {
                        warn!("读取设置失败: {error}");
                        continue;
                    }
                };

                if settings.pause_monitoring {
                    continue;
                }

                let sequence_number = unsafe { GetClipboardSequenceNumber() };
                if sequence_number == 0 || sequence_number == last_sequence_number {
                    continue;
                }
                last_sequence_number = sequence_number;

                let source_app = ActiveAppResolver::current_foreground_process_name();

                match read_file_paths_from_clipboard() {
                    Ok(Some(file_paths)) => {
                        let total_size = sum_file_sizes(&file_paths);
                        match HistoryService::ingest_files(
                            &state,
                            file_paths,
                            total_size,
                            source_app.clone(),
                        ) {
                            Ok(Some(detail)) => {
                                let _ = app.emit(CLIPS_CHANGED_EVENT, &detail.id);
                            }
                            Ok(None) => {}
                            Err(error) => warn!("处理剪贴板文件失败: {error}"),
                        }
                        continue;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        debug!("读取剪贴板文件列表失败: {error}");
                    }
                }

                let mut clipboard = match Clipboard::new() {
                    Ok(clipboard) => clipboard,
                    Err(error) => {
                        debug!("创建剪贴板句柄失败: {error}");
                        continue;
                    }
                };

                match clipboard.get_image() {
                    Ok(image) => {
                        if image.bytes.is_empty() {
                            continue;
                        }
                        let prepared = match state.image_storage.prepare_image(
                            image.bytes.as_ref(),
                            image.width,
                            image.height,
                        ) {
                            Ok(payload) => payload,
                            Err(error) => {
                                warn!("编码剪贴板图片失败: {error}");
                                continue;
                            }
                        };

                        match HistoryService::ingest_image(
                            &state,
                            prepared,
                            source_app.clone(),
                        ) {
                            Ok(Some(detail)) => {
                                let _ = app.emit(CLIPS_CHANGED_EVENT, &detail.id);
                            }
                            Ok(None) => {}
                            Err(error) => warn!("处理剪贴板图片失败: {error}"),
                        }
                        continue;
                    }
                    Err(_) => {}
                }

                let text = match clipboard.get_text() {
                    Ok(value) => value,
                    Err(_) => continue,
                };

                match HistoryService::ingest_text(&state, &text, source_app) {
                    Ok(Some(detail)) => {
                        let _ = app.emit(CLIPS_CHANGED_EVENT, &detail.id);
                    }
                    Ok(None) => {}
                    Err(error) => warn!("处理剪贴板文本失败: {error}"),
                }
            }
        });

        Ok(())
    }
}

fn read_file_paths_from_clipboard() -> Result<Option<Vec<String>>, AppError> {
    unsafe {
        if !IsClipboardFormatAvailable(CF_HDROP_FORMAT).as_bool() {
            return Ok(None);
        }

        OpenClipboard(None).map_err(|error| AppError::Clipboard(error.to_string()))?;
        let result = (|| {
            let handle = GetClipboardData(CF_HDROP_FORMAT)
                .map_err(|error| AppError::Clipboard(error.to_string()))?;
            let hdrop = HDROP(handle.0);
            let file_count = DragQueryFileW(hdrop, u32::MAX, None, 0);
            if file_count == 0 {
                return Ok(None);
            }

            let mut file_paths = Vec::with_capacity(file_count as usize);
            for index in 0..file_count {
                let required_len = DragQueryFileW(hdrop, index, None, 0);
                if required_len == 0 {
                    continue;
                }

                let mut buffer = vec![0u16; required_len as usize + 1];
                let copied_len = DragQueryFileW(
                    hdrop,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    buffer.len() as u32,
                );
                if copied_len == 0 {
                    continue;
                }

                let path = String::from_utf16_lossy(&buffer[..copied_len as usize]);
                if !path.is_empty() {
                    file_paths.push(path);
                }
            }

            if file_paths.is_empty() {
                Ok(None)
            } else {
                Ok(Some(file_paths))
            }
        })();
        let _ = CloseClipboard();
        result
    }
}

fn sum_file_sizes(file_paths: &[String]) -> Option<i64> {
    let mut total_size = 0i64;

    for path in file_paths {
        let metadata = fs::metadata(path).ok()?;
        let file_size = i64::try_from(metadata.len()).ok()?;
        total_size = total_size.checked_add(file_size)?;
    }

    Some(total_size)
}
