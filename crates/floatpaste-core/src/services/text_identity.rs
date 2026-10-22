//! 文本身份：按原文定义的内容指纹，与搜索归一化（`normalize_service`）
//! 各自独立。
//!
//! 哈希输入带版本与基础类型前缀，参与计算的是原始 `full_text` 的 UTF-8
//! 字节——代码缩进、连续空格等原文差异都算不同内容。文本用途不参与
//! 计算，调整识别规则不会制造新条目；普通文本与富文本变体共用
//! 这套版本化编码，通过格式标记与长度边界区分。

use sha2::{Digest, Sha256};

/// 文本身份的域分隔前缀：`floatpaste:text:v2\0`（基础类型 text，编码 v2）。
const TEXT_HASH_DOMAIN: &[u8] = b"floatpaste:text:v2\0";

/// 计算文本条目的内容身份。输入为完整原文；同文必然同身份。
pub fn text_content_hash(full_text: &str) -> String {
    text_formats_hash(full_text, None)
}

pub fn text_formats_hash(full_text: &str, html: Option<&[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(TEXT_HASH_DOMAIN);
    hasher.update((full_text.len() as u64).to_le_bytes());
    hasher.update(full_text.as_bytes());
    hasher.update([u8::from(html.is_some())]);
    if let Some(html) = html {
        hasher.update((html.len() as u64).to_le_bytes());
        hasher.update(html);
    }
    crate::services::digest_to_hex(hasher.finalize().iter().copied())
}

#[cfg(test)]
mod tests {
    use super::text_content_hash;

    #[test]
    fn same_text_yields_same_hash_and_whitespace_variants_differ() {
        let first = text_content_hash("Alpha\nBeta");
        assert_eq!(first, text_content_hash("Alpha\nBeta"));
        // 身份按原文定义：空白折叠后的等义文本是不同内容
        assert_ne!(first, text_content_hash(" Alpha   Beta "));
        assert_ne!(first, text_content_hash("alpha\nbeta"));
    }

    #[test]
    fn empty_and_whitespace_only_texts_have_distinct_identity() {
        assert_ne!(text_content_hash(""), text_content_hash(" "));
        assert_ne!(text_content_hash("a"), text_content_hash("a "));
    }
}
