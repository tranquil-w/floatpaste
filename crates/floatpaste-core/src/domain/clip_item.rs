use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItemSummary {
    pub id: String,
    pub r#type: String,
    pub content_preview: String,
    pub source_app: Option<String>,
    pub is_favorited: bool,
    pub file_count: i32,
    pub directory_count: i32,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    pub image_format: Option<String>,
    pub file_size: Option<i64>,
    pub tags: Vec<String>,
    /// 文本用途；图片、文件恒为 Plain（忽略此列）
    pub text_kind: TextKind,
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
    pub directory_count: i32,
    pub total_size: Option<i64>,
    pub tags: Vec<String>,
    /// 文本用途；图片、文件恒为 Plain（忽略此列）
    pub text_kind: TextKind,
}

impl ClipItemDetail {
    pub fn to_summary(&self) -> ClipItemSummary {
        ClipItemSummary {
            id: self.id.clone(),
            r#type: self.r#type.clone(),
            content_preview: self.content_preview.clone(),
            source_app: self.source_app.clone(),
            is_favorited: self.is_favorited,
            file_count: self.file_count,
            directory_count: self.directory_count,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            last_used_at: self.last_used_at.clone(),
            image_path: self.image_path.clone(),
            image_width: self.image_width,
            image_height: self.image_height,
            image_format: self.image_format.clone(),
            file_size: self.file_size,
            tags: self.tags.clone(),
            text_kind: self.text_kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagInfo {
    pub name: String,
    pub item_count: u32,
    pub created_at: String,
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
    pub directory_count: i32,
    pub total_size: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    RelevanceDesc,
    RecentDesc,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipType {
    Text,
    Image,
    File,
}

impl ClipType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::File => "file",
        }
    }
}

/// 文本用途：文字更像什么，决定徽章、预览与筛选。
///
/// 与基础类型（`ClipType`）和内容身份（hash）互相独立：用途不参与 hash，
/// 调整识别规则不会制造新条目；识别不确定时回到 `Plain`，且任何用途值
/// 都不影响原文上屏。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    #[default]
    Plain,
    Link,
    Path,
    Code,
}

impl TextKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Link => "link",
            Self::Path => "path",
            Self::Code => "code",
        }
    }

    /// 数据库读取的解码入口：未知值一律降级为普通文本，
    /// 不让识别值影响可上屏性。
    pub fn parse(value: &str) -> Self {
        match value {
            "link" => Self::Link,
            "path" => Self::Path,
            "code" => Self::Code,
            _ => Self::Plain,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilters {
    pub favorited_only: Option<bool>,
    pub clip_type: Option<ClipType>,
    pub source_app: Option<String>,
    pub include_deleted: Option<bool>,
    /// 按标签筛选，AND 语义；None 或空数组表示不筛选
    pub tag_names: Option<Vec<String>>,
    /// 按文本用途筛选，AND 语义；None 表示不筛选（"文本"仍包含全部文本用途）
    pub text_kind: Option<TextKind>,
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
    #[serde(default = "default_true")]
    pub paste_to_target: bool,
    /// 次级上屏形态（Shift+Enter）：把路径作为文本写入剪贴板——图片写
    /// 图片文件路径、文件写逐行路径列表。文本类型暂无次级形态，标记被
    /// 忽略（处理类次级形态设计中，见 docs/adr-0004）
    #[serde(default)]
    pub as_path_text: bool,
}

fn default_true() -> bool {
    true
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
    pub text_kind: TextKind,
}

#[derive(Debug, Clone)]
pub struct NewClipTextItem {
    pub normalized: NormalizedClipText,
    pub source_app: Option<String>,
}
