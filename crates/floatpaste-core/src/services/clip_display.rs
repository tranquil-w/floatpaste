//! 剪贴条目的展示格式化：类型徽章、文件大小与列表元信息行。
//!
//! 纯文本派生，不依赖 GUI 运行时；速贴、搜索、编辑与预览窗口共用，
//! 保证各窗口对同一条目的文案一致。

use crate::domain::clip_item::{ClipItemSummary, TextKind};
use crate::services::text_kind::http_url_parts;
use crate::services::time_format::format_relative_time_or_unused;

/// 列表摘要最多 120 字符；原文与上屏内容由 full_text 独立保存。
pub fn text_preview(full_text: &str, kind: TextKind) -> String {
    let trimmed = full_text.trim();
    let preview = match kind {
        TextKind::Link => http_url_parts(trimmed)
            .map(|(host, suffix)| {
                if suffix.is_empty() {
                    host.to_string()
                } else {
                    format!("{host} · {suffix}")
                }
            })
            .unwrap_or_else(|| full_text.to_string()),
        TextKind::Path => {
            let mut lines = trimmed.lines();
            let first = lines.next().unwrap_or_default().trim();
            let path = first.trim_end_matches(['\\', '/']);
            let mut summary = if first.len() == 3 && first.as_bytes().get(1) == Some(&b':') {
                first.to_string()
            } else if let Some((parent, name)) = path.rsplit_once(['\\', '/']) {
                let parent = if parent.len() == 2 && parent.as_bytes()[1] == b':' {
                    &path[..3]
                } else {
                    parent
                };
                format!("{name} · {parent}")
            } else {
                first.to_string()
            };
            let remaining = lines.count();
            if remaining > 0 {
                summary.push_str(&format!(" （另 {remaining} 条路径）"));
            }
            summary
        }
        _ => return full_text.chars().take(120).collect(),
    };
    preview.chars().take(120).collect()
}

/// 无结果时优先解释关键词与筛选，再根据监听状态引导下一步。
/// 动作码：0=无，1=清除关键词，2=清除筛选。
pub fn search_empty_state(
    keyword: &str,
    has_filters: bool,
    monitoring_paused: bool,
) -> (String, String, i32) {
    let (title, description, action) = if !keyword.trim().is_empty() {
        ("未找到匹配条目", "尝试调整搜索关键词", 1)
    } else if has_filters {
        ("当前筛选下暂无条目", "尝试切换其他筛选或复制更多内容", 2)
    } else if monitoring_paused {
        (
            "监听已暂停",
            "在托盘菜单中恢复监听后，新复制的内容才会保存。",
            0,
        )
    } else {
        (
            "暂无剪贴板条目",
            "复制文本、图片或文件后，会显示在这里。",
            0,
        )
    };
    (title.into(), description.into(), action)
}

/// 类型徽章文案（对齐旧版 getClipTypeLabel）：文件类按「文件/文件夹」
/// 组成细分，目录数以文件数为上限（脏数据兜底）；文本条目按用途细分
/// 「链接/路径/代码」，用途不改变其文本本体身份。
pub fn clip_type_label(item: &ClipItemSummary) -> String {
    match item.r#type.as_str() {
        "text" => {
            let label = match item.text_kind {
                TextKind::Link => "链接",
                TextKind::Path => "路径",
                TextKind::Code => "代码",
                TextKind::Plain => "文本",
            };
            if item.has_html {
                if item.text_kind == TextKind::Plain {
                    "富文本".into()
                } else {
                    format!("{label} · 富文本")
                }
            } else {
                label.into()
            }
        }
        "image" => "图片".to_string(),
        "file" => {
            let file_count = item.file_count.max(0) as usize;
            let directory_count = (item.directory_count.max(0) as usize).min(file_count);
            if file_count == 0 {
                "文件".to_string()
            } else if directory_count == file_count {
                "文件夹".to_string()
            } else if directory_count > 0 {
                "文件/文件夹".to_string()
            } else {
                "文件".to_string()
            }
        }
        _ => "未知".to_string(),
    }
}

/// 元信息行（对齐旧版 getItemDetailMeta）：来源 • 相对时间 • [尺寸] • [大小] • [文件数]
pub fn build_meta(item: &ClipItemSummary) -> String {
    let mut parts = vec![
        item.source_app.clone().unwrap_or_else(|| "未知来源".into()),
        format_relative_time_or_unused(
            item.last_used_at
                .as_deref()
                .or(Some(item.created_at.as_str())),
        ),
    ];
    if item.r#type == "image" {
        if let (Some(width), Some(height)) = (item.image_width, item.image_height) {
            parts.push(format!("{width} × {height}"));
        }
    }
    if let Some(label) = format_file_size(item.file_size) {
        parts.push(label);
    }
    if item.r#type == "file" {
        if item.file_count > 0 {
            parts.push(format!("{} 个文件", item.file_count));
        }
        if item.directory_count > 0 {
            parts.push(format!("{} 个文件夹", item.directory_count));
        }
    }
    if item.has_html {
        parts.push("含原格式".into());
    }
    parts.join(" · ")
}

/// 文件大小人类可读（对齐旧版 formatFileSize：1024 进制，按值选小数位）
pub fn format_file_size(bytes: Option<i64>) -> Option<String> {
    let bytes = bytes?;
    if bytes <= 0 {
        return None;
    }
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit_index = 0usize;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    let digits = if unit_index == 0 {
        0
    } else if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    Some(format!("{value:.digits$} {}", UNITS[unit_index]))
}

#[cfg(test)]
mod tests {
    use super::{build_meta, clip_type_label, format_file_size, search_empty_state};
    use crate::domain::clip_item::{ClipItemSummary, TextKind};

    #[test]
    fn text_summaries_emphasize_host_and_filename_but_keep_code_layout() {
        assert_eq!(
            super::text_preview("https://user:pass@example.com/a?x=1#f", TextKind::Link),
            "example.com · /a?x=1#f"
        );
        assert_eq!(
            super::text_preview("https://[::1]:8080/", TextKind::Link),
            "[::1] · /"
        );
        assert_eq!(
            super::text_preview("C:\\Temp\\a.txt\nD:\\b.txt", TextKind::Path),
            "a.txt · C:\\Temp （另 1 条路径）"
        );
        assert_eq!(super::text_preview("C:\\", TextKind::Path), "C:\\");
        let code = "fn main() {\n    println!(\"hi\");\n}";
        assert_eq!(super::text_preview(code, TextKind::Code), code);
        assert_eq!(
            super::text_preview(&"中".repeat(121), TextKind::Plain)
                .chars()
                .count(),
            120
        );
    }

    #[test]
    fn empty_state_prioritizes_keyword_then_filters_before_monitoring() {
        assert_eq!(search_empty_state(" abc ", true, true).2, 1);
        assert_eq!(search_empty_state("", true, true).2, 2);
        let paused = search_empty_state("", false, true);
        assert_eq!(paused.0, "监听已暂停");
        assert_eq!(paused.2, 0);
        let ready = search_empty_state("  ", false, false);
        assert_eq!(ready.0, "暂无剪贴板条目");
        assert_eq!(ready.2, 0);
    }

    fn item_of(kind: &str, file_count: i32, directory_count: i32) -> ClipItemSummary {
        ClipItemSummary {
            has_html: false,
            id: "id".into(),
            r#type: kind.into(),
            content_preview: String::new(),
            source_app: None,
            is_favorited: false,
            file_count,
            directory_count,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            image_path: None,
            image_width: None,
            image_height: None,
            image_format: None,
            file_size: None,
            tags: Vec::new(),
            text_kind: TextKind::Plain,
        }
    }

    #[test]
    fn clip_type_label_maps_kinds_and_file_composition() {
        let label =
            |kind: &str, files: i32, dirs: i32| clip_type_label(&item_of(kind, files, dirs));
        assert_eq!(label("text", 0, 0), "文本");
        assert_eq!(label("image", 0, 0), "图片");
        assert_eq!(label("file", 0, 0), "文件");
        assert_eq!(label("file", 3, 3), "文件夹");
        assert_eq!(label("file", 3, 1), "文件/文件夹");
        assert_eq!(label("file", 3, 0), "文件");
        assert_eq!(label("unknown", 0, 0), "未知");
    }

    #[test]
    fn clip_type_label_refines_text_items_by_kind() {
        let mut link = item_of("text", 0, 0);
        link.text_kind = TextKind::Link;
        assert_eq!(clip_type_label(&link), "链接");
        let mut path = item_of("text", 0, 0);
        path.text_kind = TextKind::Path;
        assert_eq!(clip_type_label(&path), "路径");
        let mut code = item_of("text", 0, 0);
        code.text_kind = TextKind::Code;
        assert_eq!(clip_type_label(&code), "代码");
        // 非文本条目忽略用途列
        let mut file = item_of("file", 1, 0);
        file.text_kind = TextKind::Code;
        assert_eq!(clip_type_label(&file), "文件");
    }

    #[test]
    fn format_file_size_uses_binary_units_and_adaptive_digits() {
        assert_eq!(format_file_size(None), None);
        assert_eq!(format_file_size(Some(0)), None);
        assert_eq!(format_file_size(Some(-5)), None);
        assert_eq!(format_file_size(Some(512)), Some("512 B".into()));
        assert_eq!(format_file_size(Some(2048)), Some("2.00 KB".into()));
        assert_eq!(format_file_size(Some(15 * 1024)), Some("15.0 KB".into()));
        assert_eq!(format_file_size(Some(200 * 1024)), Some("200 KB".into()));
    }

    #[test]
    fn build_meta_joins_available_parts_with_bullet() {
        let mut item = item_of("text", 0, 0);
        item.source_app = Some("记事本".into());
        item.created_at = chrono::Local::now().to_rfc3339();
        item.file_size = Some(2048);
        assert_eq!(build_meta(&item), "记事本 · 刚刚 · 2.00 KB");
    }

    #[test]
    fn build_meta_appends_file_and_directory_counts() {
        let mut item = item_of("file", 2, 1);
        item.source_app = Some("资源管理器".into());
        item.created_at = chrono::Local::now().to_rfc3339();
        assert_eq!(
            build_meta(&item),
            "资源管理器 · 刚刚 · 2 个文件 · 1 个文件夹"
        );
    }
}
