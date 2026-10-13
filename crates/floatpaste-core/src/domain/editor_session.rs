use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSession {
    pub item_id: String,
    pub source: EditorSource,
    pub return_to: EditorReturnTarget,
    pub target_window_hwnd: Option<isize>,
    pub target_focus_hwnd: Option<isize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorSource {
    Picker,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorReturnTarget {
    Picker,
    Search,
}
