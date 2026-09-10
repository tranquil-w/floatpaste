use crate::{
    domain::{clip_item::ClipItemDetail, error::AppError},
    repository::sqlite_repository::SqliteRepository,
    services::{
        dedup_service::{DedupDecision, DedupService},
        image_storage::PreparedImage,
        normalize_service::NormalizeService,
        privacy_service::PrivacyService,
    },
    state::CoreState,
};

/// 欢迎指引的正文与标签：数据库为空时作为首条记录写入并收藏。
const WELCOME_TEXT: &str = "欢迎使用 FloatPaste 👋  [↑↓] 导航记录 · [Enter] 快速粘贴 · [1~9] 数字键直达 · [Tab] 打开完整资料库 · [Esc] 随时退出";
const WELCOME_TITLE: &str = "使用指引";

pub struct HistoryService;

impl HistoryService {
    /// 数据库为空时写入欢迎指引记录并收藏；已有数据时不做任何事。
    /// 返回新记录详情供壳层广播变更事件，未写入时返回 `None`。
    pub fn ensure_welcome_item(
        repository: &SqliteRepository,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        if !repository.list_recent(1)?.is_empty() {
            return Ok(None);
        }

        let Some(text_item) =
            NormalizeService::normalize_text(WELCOME_TEXT, Some(WELCOME_TITLE.to_string()))
        else {
            return Ok(None);
        };

        let detail = repository.save_text_item(&text_item)?;
        repository.set_favorited(&detail.id, true)?;
        Ok(Some(detail))
    }

    pub fn ingest_text(
        state: &CoreState,
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
            DedupDecision::BumpExisting(existing_id) => {
                state.repository.bump_item(&existing_id).map(Some)
            }
            DedupDecision::StoreNew => state.repository.save_text_item(&normalized).map(Some),
        }
    }

    pub fn ingest_image(
        state: &CoreState,
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
            DedupDecision::BumpExisting(existing_id) => {
                state.repository.bump_item(&existing_id).map(Some)
            }
            DedupDecision::StoreNew => {
                let stored = state.image_storage.store_prepared_image(&prepared_image)?;
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
        state: &CoreState,
        file_paths: Vec<String>,
        directory_count: i32,
        total_size: Option<i64>,
        source_app: Option<String>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        let settings = state.current_settings()?;
        let Some(normalized) = NormalizeService::normalize_files(
            file_paths,
            directory_count,
            total_size,
            source_app.clone(),
        ) else {
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
            DedupDecision::BumpExisting(existing_id) => {
                state.repository.bump_item(&existing_id).map(Some)
            }
            DedupDecision::StoreNew => state.repository.save_file_item(&normalized).map(Some),
        }
    }
}
