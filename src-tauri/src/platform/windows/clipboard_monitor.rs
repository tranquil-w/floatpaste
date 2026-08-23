use std::{thread, time::Duration};

use arboard::Clipboard;
use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

use crate::{
    app_bootstrap::AppState,
    domain::{
        error::AppError,
        events::{ClipsChangedPayload, CLIPS_CHANGED_EVENT},
    },
    platform::windows::{
        active_app::ActiveAppResolver, clipboard_error::{map_clipboard_error, should_retry_clipboard_read},
        file_clipboard::read_file_paths_from_clipboard,
        image_clipboard::read_image_from_clipboard,
    },
    services::{
        history_service::HistoryService,
        normalize_service::analyze_file_paths,
    },
};

const CLIPBOARD_POLL_INTERVAL_MS: u64 = 800;

pub struct ClipboardMonitor;

impl ClipboardMonitor {
    pub fn start(app: AppHandle, state: AppState) -> Result<(), crate::domain::error::AppError> {
        thread::spawn(move || {
            let mut last_sequence_number = 0;

            loop {
                thread::sleep(Duration::from_millis(CLIPBOARD_POLL_INTERVAL_MS));

                // 退出后停止轮询，避免继续访问 AppHandle / emit 事件到已销毁的窗口。
                if state.is_quitting() {
                    break;
                }

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
            if let Err(error) = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail)) {
                debug!("广播文件剪贴记录变更失败: {error}");
            }
        }
        return Ok(());
    }

    if let Some(image) = read_image_from_clipboard()? {
        let prepared = state
            .image_storage
            .prepare_image(&image.rgba, image.width, image.height, image.png_bytes.as_deref())?;
        if let Some(detail) = HistoryService::ingest_image(state, prepared, source_app.clone())? {
            if let Err(error) = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail)) {
                debug!("广播图片剪贴记录变更失败: {error}");
            }
        }
        return Ok(());
    }

    let mut clipboard = Clipboard::new().map_err(map_clipboard_error)?;

    match clipboard.get_text() {
        Ok(text) => {
            if let Some(detail) = HistoryService::ingest_text(state, &text, source_app)? {
                if let Err(error) = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail)) {
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

    use super::analyze_file_paths;
    use super::super::clipboard_error::should_retry_clipboard_read;
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
