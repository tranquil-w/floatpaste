use serde::Serialize;

use crate::domain::clip_item::{ClipItemDetail, ClipItemSummary};

pub const CLIPS_CHANGED_EVENT: &str = "clips://changed";
pub const SETTINGS_CHANGED_EVENT: &str = "settings://changed";
pub const TAGS_CHANGED_EVENT: &str = "tags://changed";
pub const PICKER_SESSION_START_EVENT: &str = "picker://session-start";
pub const PICKER_SESSION_END_EVENT: &str = "picker://session-end";
pub const PICKER_NAVIGATE_EVENT: &str = "picker://navigate";
pub const PICKER_CONFIRM_EVENT: &str = "picker://confirm";
pub const PICKER_CONFIRM_AS_FILE_EVENT: &str = "picker://confirm-as-file";
pub const PICKER_SELECT_INDEX_EVENT: &str = "picker://select-index";
pub const PICKER_OPEN_EDITOR_EVENT: &str = "picker://open-editor";
pub const PICKER_FAVORITE_EVENT: &str = "picker://favorite";

// Search 相关事件
pub const SEARCH_SESSION_START_EVENT: &str = "search://session-start";
pub const SEARCH_SESSION_END_EVENT: &str = "search://session-end";
#[allow(dead_code)] // Search 改为前端本地键盘处理后，Rust 侧暂不直接发编辑事件
pub const SEARCH_EDIT_ITEM_EVENT: &str = "search://edit-item";
#[allow(dead_code)] // 预留给未来 Search 搜索操作使用
pub const SEARCH_SEARCH_EVENT: &str = "search://search";
#[allow(dead_code)] // 预留给未来 Search 回贴操作使用
pub const SEARCH_PASTE_EVENT: &str = "search://paste";
#[allow(dead_code)] // Search 改为前端本地键盘处理后，Rust 侧暂不直接发导航事件
pub const SEARCH_NAVIGATE_EVENT: &str = "search://navigate";
pub const SEARCH_INPUT_SUSPEND_EVENT: &str = "search://input-suspend";
pub const SEARCH_INPUT_RESUME_EVENT: &str = "search://input-resume";

// Editor 相关事件
pub const EDITOR_SESSION_START_EVENT: &str = "editor://session-start";
pub const EDITOR_SESSION_END_EVENT: &str = "editor://session-end";

/// `clips://changed` 的载荷。前端依据 `kind` 对列表缓存做精准合并，
/// 避免任何单条变更都触发全量重拉。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ClipsChangedPayload {
    /// 单条记录新增，或内容/收藏/标签/活跃时间发生变更，携带最新快照。
    Upserted { item: ClipItemSummary },
    /// 单条记录被删除。
    Deleted { id: String },
    /// 批量或影响范围不确定的变更（如标签重命名、删除标签），前端整体刷新。
    BulkChanged,
}

impl ClipsChangedPayload {
    pub fn upserted(detail: &ClipItemDetail) -> Self {
        Self::Upserted {
            item: detail.to_summary(),
        }
    }
}
