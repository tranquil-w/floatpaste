use crate::{
    app_bootstrap::AppState,
    domain::{clip_item::ClipItemDetail, error::AppError},
    services::{
        dedup_service::{DedupDecision, DedupService},
        image_storage::PreparedImage,
        normalize_service::NormalizeService,
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

        match DedupService::default().decide(&state.repository, &normalized.normalized.hash)? {
            DedupDecision::Skip => Ok(None),
            DedupDecision::BumpExisting(existing_id) => {
                state.repository.bump_item(&existing_id).map(Some)
            }
            DedupDecision::StoreNew => state.repository.save_text_item(&normalized).map(Some),
        }
    }

    pub fn ingest_image(
        state: &AppState,
        prepared_image: PreparedImage,
        source_app: Option<String>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        let settings = state.current_settings()?;
        let Some(mut normalized) = NormalizeService::normalize_image(
            None,
            Some(prepared_image.width),
            Some(prepared_image.height),
            Some(prepared_image.image_format.clone()),
            Some(prepared_image.file_size),
            Some(prepared_image.content_hash.clone()),
            source_app.clone(),
        ) else {
            return Ok(None);
        };

        if !PrivacyService::should_capture_image(
            &settings,
            &normalized.normalized,
            source_app.as_deref(),
            &state.self_write_guard(),
        )? {
            return Ok(None);
        }

        match DedupService::default().decide(&state.repository, &normalized.normalized.hash)? {
            DedupDecision::Skip => Ok(None),
            DedupDecision::BumpExisting(existing_id) => {
                state.repository.bump_item(&existing_id).map(Some)
            }
            DedupDecision::StoreNew => {
                let stored = state
                    .image_storage
                    .store_prepared_image(&prepared_image)?;
                normalized.normalized.image_path = Some(stored.image_path.clone());

                match state.repository.save_image_item(&normalized) {
                    Ok(detail) => Ok(Some(detail)),
                    Err(error) => {
                        let _ = state.image_storage.delete_image(&stored.image_path);
                        Err(error)
                    }
                }
            }
        }
    }

    pub fn ingest_files(
        state: &AppState,
        file_paths: Vec<String>,
        total_size: Option<i64>,
        source_app: Option<String>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        let settings = state.current_settings()?;
        let Some(normalized) =
            NormalizeService::normalize_files(file_paths, total_size, source_app.clone())
        else {
            return Ok(None);
        };

        if !PrivacyService::should_capture_file(
            &settings,
            &normalized.normalized,
            source_app.as_deref(),
            &state.self_write_guard(),
        )? {
            return Ok(None);
        }

        match DedupService::default().decide(&state.repository, &normalized.normalized.hash)? {
            DedupDecision::Skip => Ok(None),
            DedupDecision::BumpExisting(existing_id) => {
                state.repository.bump_item(&existing_id).map(Some)
            }
            DedupDecision::StoreNew => state.repository.save_file_item(&normalized).map(Some),
        }
    }
}
