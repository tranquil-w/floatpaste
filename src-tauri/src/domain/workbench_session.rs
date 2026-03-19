use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSession {
    /// 回贴目标窗口句柄
    pub target_window_hwnd: Option<isize>,
    /// 来源类型
    pub source: WorkbenchSource,
    /// 当前活动条目 ID（编辑态）
    pub current_item_id: Option<String>,
    /// 是否来自 Picker 跳转
    pub from_picker: bool,
    /// Picker 会话的原始选中索引（用于返回时恢复）
    pub picker_selected_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkbenchSource {
    #[default]
    GlobalShortcut,
    PickerEdit,
    PickerSearch,
}
