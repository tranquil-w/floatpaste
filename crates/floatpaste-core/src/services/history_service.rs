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

/// 欢迎指引的正文与标签：数据库为空时作为首条条目写入并收藏。
/// 键位必须与当前会话键一致（无 Tab 动作；数字键仅 1-9）。
const WELCOME_TEXT: &str =
    "欢迎使用 FloatPaste。[↑↓] 选择条目 · [Enter] 上屏 · [1-9] 数字直达 · [Esc] 关闭";
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
        Self::ingest_text_formats(state, text, None, source_app)
    }

    pub fn ingest_text_formats(
        state: &CoreState,
        text: &str,
        html_bytes: Option<Vec<u8>>,
        source_app: Option<String>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        Self::ingest_text_formats_at_generation(state, text, html_bytes, source_app, None)
    }

    pub(crate) fn ingest_text_formats_at_generation(
        state: &CoreState,
        text: &str,
        html_bytes: Option<Vec<u8>>,
        source_app: Option<String>,
        generation: Option<u64>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        let privacy = state.privacy_read()?;
        if privacy.session_locked
            || generation.is_some_and(|value| value != privacy.generation)
            || state.is_quitting()
        {
            return Ok(None);
        }
        super::input_limits::validate_text(text)?;
        let settings = state.current_settings()?;
        let Some(mut normalized) = NormalizeService::normalize_text(text, source_app.clone())
        else {
            return Ok(None);
        };

        // 公共入口同样校验格式；损坏或纯图片包装只保留可用纯文本。
        normalized.normalized.html_bytes =
            html_bytes.filter(|bytes| super::html_format::extract_html_text(bytes).is_some());
        normalized.normalized.hash = super::text_identity::text_formats_hash(
            text,
            normalized.normalized.html_bytes.as_deref(),
        );

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
        Self::ingest_image_at_generation(state, prepared_image, source_app, None)
    }

    pub(crate) fn ingest_image_at_generation(
        state: &CoreState,
        prepared_image: PreparedImage,
        source_app: Option<String>,
        generation: Option<u64>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        let privacy = state.privacy_read()?;
        if privacy.session_locked
            || generation.is_some_and(|value| value != privacy.generation)
            || state.is_quitting()
        {
            return Ok(None);
        }
        super::image_decode::validate_pixels(
            prepared_image.width as usize,
            prepared_image.height as usize,
        )?;
        if prepared_image.png_bytes.len() > super::input_limits::MAX_FORMAT_BYTES {
            return Err(AppError::InputLimit("PNG 超过 256 MiB"));
        }
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
                        if state
                            .image_storage
                            .delete_image(&stored.image_path)
                            .is_err()
                        {
                            if let Err(cleanup_error) =
                                state.repository.queue_image_deletion(&stored.image_path)
                            {
                                tracing::warn!(error = %cleanup_error, "保存失败后的图片未能登记清理任务");
                            }
                        }
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
        Self::ingest_files_at_generation(
            state,
            file_paths,
            directory_count,
            total_size,
            source_app,
            None,
        )
    }

    pub(crate) fn ingest_files_at_generation(
        state: &CoreState,
        file_paths: Vec<String>,
        directory_count: i32,
        total_size: Option<i64>,
        source_app: Option<String>,
        generation: Option<u64>,
    ) -> Result<Option<ClipItemDetail>, AppError> {
        let privacy = state.privacy_read()?;
        if privacy.session_locked
            || generation.is_some_and(|value| value != privacy.generation)
            || state.is_quitting()
        {
            return Ok(None);
        }
        super::input_limits::validate_files(&file_paths)?;
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
