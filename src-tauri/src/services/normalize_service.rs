use sha2::{Digest, Sha256};

use crate::domain::clip_item::{NewClipTextItem, NormalizedClipText};

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
}
