//! 剪贴条目的展示格式化：类型徽章、文件大小与列表元信息行。
//!
//! 纯文本派生，不依赖 GUI 运行时；速贴、搜索、编辑与预览窗口共用，
//! 保证各窗口对同一条目的文案一致。

use crate::domain::clip_item::ClipItemSummary;
use crate::services::time_format::format_relative_time_or_unused;

/// 类型徽章文案（对齐旧版 getClipTypeLabel）：文件类按「文件/文件夹」
/// 组成细分，目录数以文件数为上限（脏数据兜底）。
pub fn clip_type_label(item: &ClipItemSummary) -> String {
    match item.r#type.as_str() {
        "text" => "文本".to_string(),
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
    parts.join(" • ")
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
    use super::{build_meta, clip_type_label, format_file_size};
    use crate::domain::clip_item::ClipItemSummary;

    fn item_of(kind: &str, file_count: i32, directory_count: i32) -> ClipItemSummary {
        ClipItemSummary {
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
        assert_eq!(build_meta(&item), "记事本 • 刚刚 • 2.00 KB");
    }

    #[test]
    fn build_meta_appends_file_and_directory_counts() {
        let mut item = item_of("file", 2, 1);
        item.source_app = Some("资源管理器".into());
        item.created_at = chrono::Local::now().to_rfc3339();
        assert_eq!(build_meta(&item), "资源管理器 • 刚刚 • 2 个文件 • 1 个文件夹");
    }
}
