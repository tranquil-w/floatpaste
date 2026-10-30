use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItemSummary {
    #[serde(default)]
    pub has_html: bool,
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
    pub has_html: bool,
    /// 仅上屏详情读取载荷；普通详情和列表不读取 HTML。
    pub html_bytes: Option<Vec<u8>>,
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
            has_html: self.has_html,
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
    #[serde(default)]
    pub mode: PasteMode,
}

/// 上屏动作：窗口与菜单传显式选择，发布链路按模式写入剪贴板。
///
/// `Original` 按条目本体发布（富文本同时发布 HTML 与纯文本、图片发布
/// 位图、文件发布文件列表）；其余为变体：`PlainText` 只发布文字，
/// `PathText` 发布路径文本（图片保存路径 / 文件逐行完整路径），
/// `FileNameText` 发布逐行文件名，`ImageFile` 把图片以文件列表发布
/// （CF_HDROP，目标应用按文件附件接收）。`TextFile` 需填写文件名，
/// 把文本生成 UTF-8 文件后以 CF_HDROP 发布。`RevealInFolder` 定位文件或
/// 路径文本，`OpenInBrowser` 用默认浏览器打开 HTTP(S) 链接；两者不是
/// 剪贴板形态，发布链路拒绝，由上屏流程前置分流执行。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMode {
    #[default]
    Original,
    PlainText,
    PathText,
    FileNameText,
    ImageFile,
    TextFile,
    RevealInFolder,
    OpenInBrowser,
}

/// 未知持久化值回退主动作：不让手改/旧库残留阻断设置读取（与
/// PickerPositionMode 等设置枚举的降级策略一致）。
impl<'de> Deserialize<'de> for PasteMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        Ok(match value.as_str() {
            "plain_text" => Self::PlainText,
            "path_text" => Self::PathText,
            "file_name_text" => Self::FileNameText,
            "image_file" => Self::ImageFile,
            "text_file" => Self::TextFile,
            "reveal_in_folder" => Self::RevealInFolder,
            "open_in_browser" => Self::OpenInBrowser,
            _ => Self::Original,
        })
    }
}

/// 富文本的默认次级动作目录；需要填写名称的 TextFile 仅出现在菜单。
pub const RICH_TEXT_PASTE_MODES: &[PasteMode] = &[PasteMode::Original, PasteMode::PlainText];
/// 图片的可用动作：图片、图片路径、图片文件。
pub const IMAGE_PASTE_MODES: &[PasteMode] = &[
    PasteMode::Original,
    PasteMode::PathText,
    PasteMode::ImageFile,
];
/// 文件的可用动作：文件、完整路径、文件名、打开所在文件夹（非上屏，
/// 资源管理器定位，不经发布链路）。
pub const FILE_PASTE_MODES: &[PasteMode] = &[
    PasteMode::Original,
    PasteMode::PathText,
    PasteMode::FileNameText,
    PasteMode::RevealInFolder,
];
/// 普通文本不重复提供「纯文本」；文件动作需输入文件名。
pub const PLAIN_TEXT_PASTE_MODES: &[PasteMode] = &[PasteMode::Original, PasteMode::TextFile];

/// 条目的可用上屏动作，首项为主动作；菜单、按钮与次级解析共用此目录。
pub fn available_paste_modes(item: &ClipItemSummary) -> &'static [PasteMode] {
    match item.r#type.as_str() {
        "image" => IMAGE_PASTE_MODES,
        "file" => FILE_PASTE_MODES,
        "text" if item.text_kind == TextKind::Path && item.has_html => &[
            PasteMode::Original,
            PasteMode::PlainText,
            PasteMode::TextFile,
            PasteMode::RevealInFolder,
        ],
        "text" if item.text_kind == TextKind::Path => &[
            PasteMode::Original,
            PasteMode::TextFile,
            PasteMode::RevealInFolder,
        ],
        "text" if item.text_kind == TextKind::Link && item.has_html => &[
            PasteMode::Original,
            PasteMode::PlainText,
            PasteMode::TextFile,
            PasteMode::OpenInBrowser,
        ],
        "text" if item.text_kind == TextKind::Link => &[
            PasteMode::Original,
            PasteMode::TextFile,
            PasteMode::OpenInBrowser,
        ],
        "text" if item.has_html => &[
            PasteMode::Original,
            PasteMode::PlainText,
            PasteMode::TextFile,
        ],
        "text" => PLAIN_TEXT_PASTE_MODES,
        _ => &[PasteMode::Original],
    }
}

/// 动作在界面上的名称（「更多动作」菜单、按钮提示与设置页共用）。
pub fn paste_mode_label(mode: PasteMode, clip_type: &str, has_html: bool) -> &'static str {
    match mode {
        PasteMode::Original => match clip_type {
            "image" => "粘贴为图片",
            "file" => "粘贴为文件",
            "text" if has_html => "粘贴为原格式",
            _ => "粘贴为文本",
        },
        PasteMode::PlainText => "粘贴为纯文本",
        PasteMode::PathText if clip_type == "image" => "粘贴为图片路径",
        PasteMode::PathText => "粘贴为完整路径",
        PasteMode::FileNameText => "粘贴为文件名",
        PasteMode::ImageFile => "粘贴为图片文件",
        PasteMode::TextFile => "粘贴为文件…",
        PasteMode::RevealInFolder => "打开所在文件夹",
        PasteMode::OpenInBrowser => "在浏览器中打开",
    }
}

/// 解析条目的默认次级动作：取该类别配置值，不在可用目录（旧库或手改
/// 配置残留）时回退该类别内置默认——富文本纯文本、图片与文件路径
/// 文本，即次级动作可配置之前的既有行为。普通文本次级等同主动作。
pub fn resolve_secondary_mode(
    item: &ClipItemSummary,
    defaults: &super::settings::SecondaryPasteDefaults,
) -> PasteMode {
    let (configured, fallback) = match item.r#type.as_str() {
        "image" => (defaults.image, PasteMode::PathText),
        "file" => (defaults.file, PasteMode::PathText),
        "text" if item.has_html => (defaults.rich_text, PasteMode::PlainText),
        _ => return PasteMode::Original,
    };
    let allowed = if item.r#type == "text" {
        RICH_TEXT_PASTE_MODES
    } else {
        available_paste_modes(item)
    };
    if allowed.contains(&configured) {
        configured
    } else {
        fallback
    }
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
    pub html_bytes: Option<Vec<u8>>,
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

#[cfg(test)]
mod tests {
    use super::{
        available_paste_modes, paste_mode_label, resolve_secondary_mode, ClipItemSummary, PasteMode,
    };
    use crate::domain::settings::SecondaryPasteDefaults;

    fn item(clip_type: &str, has_html: bool) -> ClipItemSummary {
        ClipItemSummary {
            has_html,
            id: String::new(),
            r#type: clip_type.to_string(),
            content_preview: String::new(),
            source_app: None,
            is_favorited: false,
            file_count: 0,
            directory_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            image_path: None,
            image_width: None,
            image_height: None,
            image_format: None,
            file_size: None,
            tags: vec![],
            text_kind: Default::default(),
        }
    }

    #[test]
    fn catalog_lists_primary_first_per_type() {
        assert_eq!(
            available_paste_modes(&item("text", false)),
            &[PasteMode::Original, PasteMode::TextFile]
        );
        assert_eq!(
            available_paste_modes(&item("text", true)),
            &[
                PasteMode::Original,
                PasteMode::PlainText,
                PasteMode::TextFile
            ]
        );
        assert_eq!(
            available_paste_modes(&item("image", false)),
            &[
                PasteMode::Original,
                PasteMode::PathText,
                PasteMode::ImageFile
            ]
        );
        assert_eq!(
            available_paste_modes(&item("file", false)),
            &[
                PasteMode::Original,
                PasteMode::PathText,
                PasteMode::FileNameText,
                PasteMode::RevealInFolder
            ]
        );
    }

    #[test]
    fn labels_follow_the_paste_as_family() {
        assert_eq!(
            paste_mode_label(PasteMode::Original, "text", true),
            "粘贴为原格式"
        );
        assert_eq!(
            paste_mode_label(PasteMode::Original, "text", false),
            "粘贴为文本"
        );
        // 图片与文件的路径文本同模式不同名，不混淆两个类别
        assert_eq!(
            paste_mode_label(PasteMode::PathText, "image", false),
            "粘贴为图片路径"
        );
        assert_eq!(
            paste_mode_label(PasteMode::PathText, "file", false),
            "粘贴为完整路径"
        );
        assert_eq!(
            paste_mode_label(PasteMode::FileNameText, "file", false),
            "粘贴为文件名"
        );
        assert_eq!(
            paste_mode_label(PasteMode::ImageFile, "image", false),
            "粘贴为图片文件"
        );
        // 非上屏动作不属于「粘贴为」族
        assert_eq!(
            paste_mode_label(PasteMode::RevealInFolder, "file", false),
            "打开所在文件夹"
        );
    }

    #[test]
    fn text_external_actions_follow_purpose_without_changing_secondary_defaults() {
        use super::TextKind;
        for has_html in [false, true] {
            for (kind, action) in [
                (TextKind::Path, PasteMode::RevealInFolder),
                (TextKind::Link, PasteMode::OpenInBrowser),
            ] {
                let mut text = item("text", has_html);
                text.text_kind = kind;
                let mut expected = vec![PasteMode::Original];
                if has_html {
                    expected.push(PasteMode::PlainText);
                }
                expected.extend([PasteMode::TextFile, action]);
                assert_eq!(available_paste_modes(&text), expected);
                let defaults = SecondaryPasteDefaults {
                    rich_text: action,
                    ..Default::default()
                };
                assert_eq!(
                    resolve_secondary_mode(&text, &defaults),
                    if has_html {
                        PasteMode::PlainText
                    } else {
                        PasteMode::Original
                    }
                );
            }
        }
    }

    #[test]
    fn secondary_resolves_configured_value_per_category() {
        let defaults = SecondaryPasteDefaults {
            rich_text: PasteMode::Original,
            image: PasteMode::ImageFile,
            file: PasteMode::RevealInFolder,
        };
        assert_eq!(
            resolve_secondary_mode(&item("text", true), &defaults),
            PasteMode::Original
        );
        assert_eq!(
            resolve_secondary_mode(&item("image", false), &defaults),
            PasteMode::ImageFile
        );
        assert_eq!(
            resolve_secondary_mode(&item("file", false), &defaults),
            PasteMode::RevealInFolder
        );
    }

    #[test]
    fn secondary_falls_back_when_configured_mode_unavailable() {
        // 图片类别不可能出现 PlainText（sanitized 已挡，这里防旧库/手改残留）
        let defaults = SecondaryPasteDefaults {
            image: PasteMode::PlainText,
            file: PasteMode::ImageFile,
            ..SecondaryPasteDefaults::default()
        };
        assert_eq!(
            resolve_secondary_mode(&item("image", false), &defaults),
            PasteMode::PathText
        );
        assert_eq!(
            resolve_secondary_mode(&item("file", false), &defaults),
            PasteMode::PathText
        );
    }

    #[test]
    fn plain_text_secondary_stays_original() {
        assert_eq!(
            resolve_secondary_mode(&item("text", false), &SecondaryPasteDefaults::default()),
            PasteMode::Original
        );
    }

    #[test]
    fn paste_mode_deserialize_falls_back_to_original() {
        let mode: PasteMode = serde_json::from_str("\"no_such_mode\"").unwrap();
        assert_eq!(mode, PasteMode::Original);
        for value in ["\"original\"", "\"plain_text\"", "\"path_text\""] {
            serde_json::from_str::<PasteMode>(value).unwrap();
        }
        // 序列化往返保持值（设置页草稿读取依赖）
        for mode in [
            PasteMode::Original,
            PasteMode::PlainText,
            PasteMode::PathText,
            PasteMode::FileNameText,
            PasteMode::ImageFile,
            PasteMode::TextFile,
            PasteMode::RevealInFolder,
            PasteMode::OpenInBrowser,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(serde_json::from_str::<PasteMode>(&json).unwrap(), mode);
        }
    }
}
