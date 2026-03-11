use sha2::{Digest, Sha256};

use crate::domain::clip_item::{
    NewClipFileItem, NewClipImageItem, NewClipTextItem, NormalizedClipFile, NormalizedClipImage,
    NormalizedClipText,
};

pub struct NormalizeService;

impl NormalizeService {
    pub fn normalize_text(text: &str, source_app: Option<String>) -> Option<NewClipTextItem> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        let compact = trimmed
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        let preview = compact.chars().take(120).collect::<String>();
        let hash = format!("{:x}", Sha256::digest(compact.as_bytes()));

        Some(NewClipTextItem {
            normalized: NormalizedClipText {
                full_text: trimmed.to_string(),
                preview_text: preview,
                search_text: compact.to_lowercase(),
                hash,
            },
            source_app,
        })
    }

    pub fn normalize_image(
        image_path: Option<String>,
        width: Option<i32>,
        height: Option<i32>,
        format: Option<String>,
        file_size: Option<i64>,
        content_hash: Option<String>,
        source_app: Option<String>,
    ) -> Option<NewClipImageItem> {
        let Some(content_hash) = content_hash else {
            return None;
        };

        let dimension_text = match (width, height) {
            (Some(width), Some(height)) => format!("{width} x {height}"),
            (Some(width), None) => format!("{width} x ?"),
            (None, Some(height)) => format!("? x {height}"),
            (None, None) => "未知尺寸".to_string(),
        };
        let preview = if let Some(size) = file_size {
            format!("图片 ({dimension_text}, {})", format_bytes(size))
        } else {
            format!("图片 ({dimension_text})")
        };
        let search_text = format!(
            "图片 {} {} {}",
            dimension_text,
            format.as_deref().unwrap_or(""),
            file_size
                .map(format_bytes)
                .unwrap_or_default(),
        )
        .trim()
        .to_lowercase();
        let hash_input = format!(
            "{:?}{:?}{:?}{:?}{:?}",
            content_hash,
            width,
            height,
            format,
            file_size
        );
        let hash = format!("{:x}", Sha256::digest(hash_input.as_bytes()));

        Some(NewClipImageItem {
            normalized: NormalizedClipImage {
                preview_text: preview,
                search_text,
                hash,
                image_path,
                image_width: width,
                image_height: height,
                image_format: format,
                file_size,
            },
            source_app,
        })
    }

    pub fn normalize_files(
        file_paths: Vec<String>,
        total_size: Option<i64>,
        source_app: Option<String>,
    ) -> Option<NewClipFileItem> {
        let file_paths = file_paths
            .into_iter()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        if file_paths.is_empty() {
            return None;
        }

        let file_count = file_paths.len() as i32;
        let preview = if file_count == 1 {
            file_paths
                .first()
                .map(|path| format!("文件: {}", extract_filename(path)))
                .unwrap_or("文件".to_string())
        } else {
            let summary = total_size
                .map(format_bytes)
                .map(|size| format!("{file_count} 个文件 ({size})"))
                .unwrap_or_else(|| format!("{file_count} 个文件"));
            summary
        };

        let search_text = file_paths
            .iter()
            .map(extract_filename)
            .collect::<Vec<_>>()
            .join(" ");

        let hash_input = format!("{:?}{:?}", file_paths, total_size);
        let hash = format!("{:x}", Sha256::digest(hash_input.as_bytes()));

        Some(NewClipFileItem {
            normalized: NormalizedClipFile {
                preview_text: preview,
                search_text,
                hash,
                file_paths,
                file_count,
                total_size,
            },
            source_app,
        })
    }
}

fn format_bytes(bytes: i64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < units.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, units[unit_index])
}

fn extract_filename(path: &str) -> String {
    let path = std::path::Path::new(path);
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path.to_str().unwrap_or(""))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::NormalizeService;

    #[test]
    fn normalize_files_skips_empty_path_list() {
        let item = NormalizeService::normalize_files(vec![" ".to_string()], None, None);
        assert!(item.is_none());
    }

    #[test]
    fn normalize_files_builds_preview_and_count() {
        let item = NormalizeService::normalize_files(
            vec![
                "C:\\Temp\\a.txt".to_string(),
                "C:\\Temp\\b.txt".to_string(),
            ],
            Some(2_048),
            None,
        )
        .unwrap();

        assert_eq!(item.normalized.file_count, 2);
        assert_eq!(item.normalized.preview_text, "2 个文件 (2.0 KB)");
        assert_eq!(item.normalized.search_text, "a.txt b.txt");
    }

    #[test]
    fn normalize_image_builds_readable_preview() {
        let item = NormalizeService::normalize_image(
            None,
            Some(1920),
            Some(1080),
            Some("png".to_string()),
            Some(2_048),
            Some("deadbeef".to_string()),
            None,
        )
        .unwrap();

        assert_eq!(item.normalized.preview_text, "图片 (1920 x 1080, 2.0 KB)");
        assert!(item.normalized.search_text.contains("1920 x 1080"));
        assert!(item.normalized.search_text.contains("png"));
        assert!(item.normalized.hash.len() > 10);
    }
}
