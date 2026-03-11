use std::{fs, thread, time::Duration};

use arboard::{Clipboard, Error as ClipboardError};
use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

use crate::{
    app_bootstrap::AppState,
    domain::{error::AppError, events::CLIPS_CHANGED_EVENT},
    platform::windows::{
        active_app::ActiveAppResolver, file_clipboard::read_file_paths_from_clipboard,
        image_clipboard::read_image_from_clipboard,
    },
    services::history_service::HistoryService,
};

const CLIPBOARD_POLL_INTERVAL_MS: u64 = 800;

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

                let source_app = ActiveAppResolver::current_foreground_process_name();

                match process_clipboard_change(&app, &state, source_app) {
                    Ok(()) => {
                        last_sequence_number = sequence_number;
                    }
                    Err(error) => {
                        debug!("处理剪贴板变更失败，将在下轮重试: {error}");
                    }
                }
            }
        });

        Ok(())
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
            if let Err(error) = app.emit(CLIPS_CHANGED_EVENT, &detail.id) {
                debug!("广播文件剪贴记录变更失败: {error}");
            }
        }
        return Ok(());
    }

    if let Some(image) = read_image_from_clipboard()? {
        let prepared = state
            .image_storage
            .prepare_image(&image.rgba, image.width, image.height)?;
        if let Some(detail) = HistoryService::ingest_image(state, prepared, source_app.clone())? {
            if let Err(error) = app.emit(CLIPS_CHANGED_EVENT, &detail.id) {
                debug!("广播图片剪贴记录变更失败: {error}");
            }
        }
        return Ok(());
    }

    let mut clipboard = Clipboard::new().map_err(map_clipboard_error)?;

    match clipboard.get_text() {
        Ok(text) => {
            if let Some(detail) = HistoryService::ingest_text(state, &text, source_app)? {
                if let Err(error) = app.emit(CLIPS_CHANGED_EVENT, &detail.id) {
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

fn should_retry_clipboard_read(error: &ClipboardError) -> bool {
    matches!(error, ClipboardError::ClipboardOccupied)
}

fn map_clipboard_error(error: ClipboardError) -> AppError {
    AppError::Clipboard(error.to_string())
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
            Err(error) => {
                debug!("读取文件元数据失败，无法统计文件总大小: {path}, {error}");
                size_available = false;
                continue;
            }
        };

        if metadata.is_dir() {
            directory_count += 1;
            size_available = false;
            continue;
        }

        let file_size = match i64::try_from(metadata.len()) {
            Ok(file_size) => file_size,
            Err(_) => {
                debug!("文件大小超出 i64 支持范围，无法统计文件总大小: {path}");
                size_available = false;
                continue;
            }
        };
        total_size = match total_size.checked_add(file_size) {
            Some(total_size) => total_size,
            None => {
                debug!("文件总大小累计溢出，无法统计文件总大小");
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

#[cfg(test)]
mod tests {
    use std::fs;

    use arboard::Error as ClipboardError;
    use uuid::Uuid;

    use super::{analyze_file_paths, should_retry_clipboard_read, FileSelectionStats};

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
