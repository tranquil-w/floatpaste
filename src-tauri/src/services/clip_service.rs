use tracing::warn;

use crate::{
    app_bootstrap::AppState,
    domain::{clip_item::ClipItemDetail, error::AppError},
    services::normalize_service::NormalizeService,
};

/// 剪辑项的读取外变更服务（编辑、删除、收藏）。
///
/// 与专注录入的 [`crate::services::history_service::HistoryService`] 互补，
/// 将原本散落在命令层中的业务规则（类型校验、归一化、关联资源清理）
/// 收敛到 service 层。跨进程事件广播仍由命令层负责。
pub struct ClipService;

impl ClipService {
    /// 编辑文本类型的剪辑项：校验类型、归一化后落库，返回最新详情。
    pub fn update_text(
        state: &AppState,
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

        let normalized = NormalizeService::normalize_text(text, None)
            .ok_or_else(|| AppError::Message("更新内容不能为空".to_string()))?;
        state.repository.update_text(id, &normalized)
    }

    /// 删除剪辑项：先移除数据库记录，若是图片类型再尽力清理已存储的图片文件。
    pub fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
        let detail = state.repository.get_item_detail(id)?;
        state.repository.delete_item(id)?;

        if detail.r#type == "image" {
            if let Some(image_path) = detail.image_path.as_deref() {
                if let Err(error) = state.image_storage.delete_image(image_path) {
                    warn!("删除图片文件失败: {image_path}, error={error}");
                }
            }
        }
        Ok(())
    }

    /// 设置剪辑项的收藏状态。
    pub fn set_favorited(state: &AppState, id: &str, value: bool) -> Result<(), AppError> {
        state.repository.set_favorited(id, value)
    }
}
