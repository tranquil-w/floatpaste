use crate::{
    domain::{clip_item::ClipItemDetail, error::AppError},
    services::normalize_service::NormalizeService,
    state::CoreState,
};

/// 剪辑项的读取外变更服务（编辑、删除、收藏）。
///
/// 与专注录入的 [`crate::services::history_service::HistoryService`] 互补，
/// 统一类型校验、归一化与软删语义；各窗口的数据刷新由壳层负责。
pub struct ClipService;

pub struct ClearHistoryResult {
    pub pending_images: usize,
    pub pending_text_files: usize,
    pub storage_compacted: bool,
}

impl ClipService {
    /// 用户明确确认后的全部历史清除；不清空系统剪贴板或用户源文件。
    pub fn clear_history(state: &CoreState) -> Result<ClearHistoryResult, AppError> {
        let mut privacy = state.privacy_write()?;
        state.repository.clear_history()?;
        privacy.generation += 1;
        privacy.ignored_clipboard_sequence =
            Some(crate::platform::windows::clipboard_formats::clipboard_sequence());
        let pending_images =
            super::retention_service::RetentionService::retry_image_deletions(state)?;
        let pending_text_files = state.text_file_storage.cleanup(true)?;
        let storage_compacted = match state.repository.vacuum() {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(error = %error, "历史已移除，数据库压缩失败");
                false
            }
        };
        Ok(ClearHistoryResult {
            pending_images,
            pending_text_files,
            storage_compacted,
        })
    }

    /// 编辑文本类型的剪辑项：校验类型、归一化后落库，返回最新详情。
    pub fn update_text(
        state: &CoreState,
        id: &str,
        text: &str,
    ) -> Result<ClipItemDetail, AppError> {
        let existing = state.repository.get_item_detail(id)?;
        if existing.r#type != "text" {
            return Err(AppError::Message(format!(
                "不能编辑 {} 类型的记录",
                existing.r#type
            )));
        }

        if existing.full_text == text {
            return Ok(existing);
        }

        super::input_limits::validate_text(text)?;

        let normalized = NormalizeService::normalize_text(text, None)
            .ok_or_else(|| AppError::Message("更新内容不能为空".to_string()))?;
        state.repository.update_text(id, &normalized)
    }

    /// 软删条目并保留载荷；到期后由 RetentionService 物理回收。
    pub fn delete(state: &CoreState, id: &str) -> Result<(), AppError> {
        // 保留缺失条目的错误契约，不把不存在的 ID 当成删除成功。
        state.repository.get_item_detail(id)?;
        state.repository.delete_item(id)
    }

    /// 设置剪辑项的收藏状态。
    pub fn set_favorited(state: &CoreState, id: &str, value: bool) -> Result<(), AppError> {
        state.repository.set_favorited(id, value)
    }
}
