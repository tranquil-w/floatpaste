use crate::{
    app_bootstrap::AppState,
    domain::{clip_item::ClipItemDetail, error::AppError},
    services::{
        dedup_service::DedupService, normalize_service::NormalizeService,
        privacy_service::PrivacyService,
    },
};

pub struct HistoryService;

impl HistoryService {
    pub fn ingest_text(
        state: &AppState,
        text: &str,
        source_app: Option<String>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        let settings = state.current_settings()?;
        let Some(normalized) = NormalizeService::normalize_text(text, source_app.clone()) else {
            return Ok(None);
        };

        if !PrivacyService::should_capture(
            &settings,
            &normalized.normalized,
            source_app.as_deref(),
            &state.self_write_guard(),
        )? {
            return Ok(None);
        }

        if !DedupService::default().should_store(&state.repository, &normalized.normalized.hash)? {
            return Ok(None);
        }

        state.repository.save_text_item(&normalized).map(Some)
    }
}
