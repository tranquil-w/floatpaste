use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSession {
    /// 回贴目标窗口句柄
    pub target_window_hwnd: Option<isize>,
    /// 来源类型
    pub source: WorkbenchSource,
    /// 当前活动条目 ID（搜索时选中的条目）
    pub current_item_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkbenchSource {
    #[default]
    GlobalShortcut,
}
