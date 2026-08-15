use std::collections::HashSet;

use crate::{
    app_bootstrap::AppState,
    domain::{clip_item::ClipItemDetail, error::AppError},
};

/// 标签业务服务：归一化、去重、数量上限与 FTS 同步的规则收敛层。
pub struct TagService;

const MAX_TAG_NAME_LEN: usize = 32;
const MAX_TAGS_PER_ITEM: usize = 20;

impl TagService {
    /// 全量替换条目标签。归一化后按 NOCASE 去重，并把名字对齐到既有标签的规范写法。
    pub fn set_item_tags(
        state: &AppState,
        id: &str,
        tag_names: &[String],
    ) -> Result<ClipItemDetail, AppError> {
        let normalized = Self::normalize_tag_names(tag_names)?;
        if normalized.len() > MAX_TAGS_PER_ITEM {
            return Err(AppError::Message(format!(
                "单条目标签数不能超过 {MAX_TAGS_PER_ITEM} 个"
            )));
        }
        let canonical = Self::canonicalize(state, &normalized)?;
        state.repository.set_item_tags(id, &canonical)
    }

    /// 重命名标签；新名已存在（NOCASE 比较）时由仓储层合并。
    pub fn rename_tag(state: &AppState, old_name: &str, new_name: &str) -> Result<(), AppError> {
        let old_name = normalize_single_tag_name(old_name)?;
        let new_name = normalize_single_tag_name(new_name)?;
        state.repository.rename_tag(&old_name, &new_name)
    }

    pub fn delete_tag(state: &AppState, name: &str) -> Result<(), AppError> {
        let name = normalize_single_tag_name(name)?;
        state.repository.delete_tag(&name)
    }

    fn normalize_tag_names(tag_names: &[String]) -> Result<Vec<String>, AppError> {
        let mut seen = HashSet::new();
        let mut normalized = Vec::new();
        for name in tag_names {
            let name = normalize_single_tag_name(name)?;
            // 前端芯片集合可能混入大小写变体，按小写形态去重
            if seen.insert(name.to_lowercase()) {
                normalized.push(name);
            }
        }
        Ok(normalized)
    }

    /// 把每个名字对齐到 `tags` 表既有行的规范写法，避免 NOCASE 主键下
    /// 以变体写法新建关联行与既有行冲突（INSERT OR IGNORE 会静默丢弃关联）。
    fn canonicalize(state: &AppState, tag_names: &[String]) -> Result<Vec<String>, AppError> {
        let existing = state.repository.list_tags()?;
        let mut canonical = Vec::with_capacity(tag_names.len());
        for name in tag_names {
            let matched = existing
                .iter()
                .find(|tag| tag.name.eq_ignore_ascii_case(name))
                .map(|tag| tag.name.clone())
                .unwrap_or_else(|| name.clone());
            canonical.push(matched);
        }
        Ok(canonical)
    }
}

/// 单个标签名归一化：trim、折叠连续空白、长度校验。
fn normalize_single_tag_name(name: &str) -> Result<String, AppError> {
    let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(AppError::Message("标签名不能为空".to_string()));
    }
    if normalized.chars().count() > MAX_TAG_NAME_LEN {
        return Err(AppError::Message(format!(
            "标签名不能超过 {MAX_TAG_NAME_LEN} 个字符"
        )));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{normalize_single_tag_name, TagService};

    #[test]
    fn normalize_single_tag_name_trims_and_collapses_whitespace() {
        assert_eq!(
            normalize_single_tag_name("  工作   资料 \t").unwrap(),
            "工作 资料"
        );
    }

    #[test]
    fn normalize_single_tag_name_rejects_empty_and_overlong() {
        assert!(normalize_single_tag_name("   ").is_err());
        let overlong = "a".repeat(33);
        assert!(normalize_single_tag_name(&overlong).is_err());
    }

    #[test]
    fn normalize_tag_names_dedupes_case_variants() {
        let result =
            TagService::normalize_tag_names(&["Work".to_string(), "work".to_string(), "个人".to_string()])
                .unwrap();
        assert_eq!(result, vec!["Work".to_string(), "个人".to_string()]);
    }
}
