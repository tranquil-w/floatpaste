use std::fs;

use sha2::{Digest, Sha256};
use tracing::debug;

use crate::domain::clip_item::{
    NewClipFileItem, NewClipImageItem, NewClipTextItem, NormalizedClipFile, NormalizedClipImage,
    NormalizedClipText,
};

pub struct NormalizeService;

impl NormalizeService {
    pub fn normalize_text(text: &str, source_app: Option<String>) -> Option<NewClipTextItem> {
        if text.trim().is_empty() {
            return None;
        }

        let normalized_text = normalize_text_for_indexing(text);
        let preview = text.chars().take(120).collect::<String>();
        let hash = format!("{:x}", Sha256::digest(normalized_text.as_bytes()));

        Some(NewClipTextItem {
            normalized: NormalizedClipText {
                full_text: text.to_string(),
                preview_text: preview,
                search_text: normalized_text.to_lowercase(),
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
            file_size.map(format_bytes).unwrap_or_default(),
        )
        .trim()
        .to_lowercase();
        let hash_input = format!(
            "{:?}{:?}{:?}{:?}{:?}",
            content_hash, width, height, format, file_size
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
        directory_count: i32,
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
        let directory_count = directory_count.clamp(0, file_count);
        let preview = build_file_preview(&file_paths, file_count, directory_count, total_size);

        let search_text = file_paths
            .iter()
            .map(|path| extract_filename(path))
            .collect::<Vec<_>>()
            .join(" ");

        let hash_input = format!("{:?}{:?}{:?}", file_paths, directory_count, total_size);
        let hash = format!("{:x}", Sha256::digest(hash_input.as_bytes()));

        Some(NewClipFileItem {
            normalized: NormalizedClipFile {
                preview_text: preview,
                search_text,
                hash,
                file_paths,
                file_count,
                directory_count,
                total_size,
            },
            source_app,
        })
    }
}

pub fn build_file_preview(
    file_paths: &[String],
    file_count: i32,
    directory_count: i32,
    total_size: Option<i64>,
) -> String {
    if file_count <= 1 {
        let label = if directory_count > 0 {
            "文件夹"
        } else {
            "文件"
        };
        return file_paths
            .first()
            .map(|path| format!("{label}: {}", extract_filename(path)))
            .unwrap_or_else(|| label.to_string());
    }

    if directory_count == file_count {
        return format!("{file_count} 个文件夹");
    }

    if directory_count > 0 {
        let file_only_count = file_count - directory_count;
        return format!("{file_only_count} 个文件，{directory_count} 个文件夹");
    }

    total_size
        .map(format_bytes)
        .map(|size| format!("{file_count} 个文件 ({size})"))
        .unwrap_or_else(|| format!("{file_count} 个文件"))
}

/// 文件选择集合的统计结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileSelectionStats {
    pub directory_count: i32,
    pub total_size: Option<i64>,
}

/// 遍历文件路径，统计其中的目录数与所有普通文件的总大小。
///
/// - 任一路径元数据读取失败、文件大小超出 i64 范围或总大小累计溢出时，
///   `total_size` 退化为 `None`（视为"大小不可用"），但不影响 `directory_count`。
/// - 只要存在目录，文件总大小即视为不可用（`total_size = None`）。
pub fn analyze_file_paths(file_paths: &[String]) -> FileSelectionStats {
    let mut total_size = 0i64;
    let mut directory_count = 0i32;
    let mut size_available = true;

    for path in file_paths {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                debug!("读取文件元数据失败，无法统计文件总大小: {path}, {error}");
                size_available = false;
                continue;
            }
        };

        if metadata.is_dir() {
            directory_count += 1;
            size_available = false;
            continue;
        }

        let file_size = match i64::try_from(metadata.len()) {
            Ok(file_size) => file_size,
            Err(_) => {
                debug!("文件大小超出 i64 支持范围，无法统计文件总大小: {path}");
                size_available = false;
                continue;
            }
        };
        total_size = match total_size.checked_add(file_size) {
            Some(total_size) => total_size,
            None => {
                debug!("文件总大小累计溢出，无法统计文件总大小");
                size_available = false;
                continue;
            }
        };
    }

    FileSelectionStats {
        directory_count,
        total_size: if size_available {
            Some(total_size)
        } else {
            None
        },
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

fn normalize_text_for_indexing(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::NormalizeService;

    #[test]
    fn normalize_text_keeps_raw_text_but_normalizes_search_and_hash() {
        let item = NormalizeService::normalize_text("  Hello\n  WORLD \t ", None).unwrap();

        assert_eq!(item.normalized.full_text, "  Hello\n  WORLD \t ");
        assert_eq!(item.normalized.search_text, "hello world");
        assert_eq!(
            item.normalized.hash,
            format!("{:x}", Sha256::digest("Hello WORLD".as_bytes()))
        );
    }

    #[test]
    fn normalize_text_treats_whitespace_only_variants_as_same_hash() {
        let first = NormalizeService::normalize_text("Alpha\nBeta", None).unwrap();
        let second = NormalizeService::normalize_text(" Alpha   Beta ", None).unwrap();

        assert_eq!(first.normalized.hash, second.normalized.hash);
        assert_eq!(first.normalized.search_text, second.normalized.search_text);
    }

    #[test]
    fn normalize_files_skips_empty_path_list() {
        let item = NormalizeService::normalize_files(vec![" ".to_string()], 0, None, None);
        assert!(item.is_none());
    }

    #[test]
    fn normalize_files_builds_preview_and_count() {
        let item = NormalizeService::normalize_files(
            vec!["C:\\Temp\\a.txt".to_string(), "C:\\Temp\\b.txt".to_string()],
            0,
            Some(2_048),
            None,
        )
        .unwrap();

        assert_eq!(item.normalized.file_count, 2);
        assert_eq!(item.normalized.directory_count, 0);
        assert_eq!(item.normalized.preview_text, "2 个文件 (2.0 KB)");
        assert_eq!(item.normalized.search_text, "a.txt b.txt");
    }

    #[test]
    fn normalize_files_marks_single_directory_preview() {
        let item = NormalizeService::normalize_files(
            vec!["C:\\Temp\\项目资料".to_string()],
            1,
            None,
            None,
        )
        .unwrap();

        assert_eq!(item.normalized.preview_text, "文件夹: 项目资料");
        assert_eq!(item.normalized.directory_count, 1);
    }

    #[test]
    fn normalize_files_marks_mixed_entries_preview() {
        let item = NormalizeService::normalize_files(
            vec![
                "C:\\Temp\\a.txt".to_string(),
                "C:\\Temp\\设计稿".to_string(),
            ],
            1,
            None,
            None,
        )
        .unwrap();

        assert_eq!(item.normalized.preview_text, "1 个文件，1 个文件夹");
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
