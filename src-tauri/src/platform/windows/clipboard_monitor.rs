use std::{thread, time::Duration};

use arboard::Clipboard;
use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};

use crate::{
    app_bootstrap::AppState,
    domain::events::CLIPS_CHANGED_EVENT,
    platform::windows::active_app::ActiveAppResolver,
    services::history_service::HistoryService,
};

pub struct ClipboardMonitor;

impl ClipboardMonitor {
    pub fn start(
        app: AppHandle,
        state: AppState,
    ) -> Result<(), crate::domain::error::AppError> {
        thread::spawn(move || {
            let mut last_observed_text = String::new();

            loop {
                thread::sleep(Duration::from_millis(800));

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

                let mut clipboard = match Clipboard::new() {
                    Ok(clipboard) => clipboard,
                    Err(error) => {
                        debug!("创建剪贴板句柄失败: {error}");
                        continue;
                    }
                };

                let text = match clipboard.get_text() {
                    Ok(value) => value,
                    Err(_) => continue,
                };

                if text == last_observed_text {
                    continue;
                }

                last_observed_text = text.clone();
                let source_app = ActiveAppResolver::current_foreground_process_name();
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
