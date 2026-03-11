use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItemSummary {
    pub id: String,
    pub r#type: String,
    pub content_preview: String,
    pub source_app: Option<String>,
    pub is_favorited: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItemDetail {
    pub id: String,
    pub r#type: String,
    pub content_preview: String,
    pub full_text: String,
    pub search_text: String,
    pub source_app: Option<String>,
    pub is_favorited: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
    pub hash: String,
    // Image-specific fields
    pub image_path: Option<String>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    pub image_format: Option<String>,
    pub file_size: Option<i64>,
    // File-specific fields
    pub file_paths: Vec<String>,
    pub file_count: i32,
    pub total_size: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewClipImageItem {
    pub normalized: NormalizedClipImage,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NormalizedClipImage {
    pub preview_text: String,
    pub search_text: String,
    pub hash: String,
    pub image_path: Option<String>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    pub image_format: Option<String>,
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewClipFileItem {
    pub normalized: NormalizedClipFile,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NormalizedClipFile {
    pub preview_text: String,
    pub search_text: String,
    pub hash: String,
    pub file_paths: Vec<String>,
    pub file_count: i32,
    pub total_size: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    RelevanceDesc,
    RecentDesc,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilters {
    pub favorited_only: Option<bool>,
    pub source_app: Option<String>,
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub keyword: String,
    pub filters: SearchFilters,
    pub offset: u32,
    pub limit: u32,
    pub sort: SearchSort,
}

impl SearchQuery {
    pub fn normalized(mut self) -> Self {
        self.limit = self.limit.clamp(1, 10000);
        if self.keyword.trim().is_empty() {
            self.keyword.clear();
            self.sort = SearchSort::RecentDesc;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub items: Vec<ClipItemSummary>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteOption {
    pub restore_clipboard_after_paste: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteResult {
    pub success: bool,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct NormalizedClipText {
    pub full_text: String,
    pub preview_text: String,
    pub search_text: String,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct NewClipTextItem {
    pub normalized: NormalizedClipText,
    pub source_app: Option<String>,
}
