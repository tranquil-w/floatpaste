use crate::{
    domain::error::AppError,
    repository::sqlite_repository::SqliteRepository,
};

/// 去重决策结果
#[derive(Debug)]
pub enum DedupDecision {
    /// 全新内容，应插入新记录
    StoreNew,
    /// 内容已存在，刷新已有记录（返回已有记录的 id）
    BumpExisting(String),
}

pub struct DedupService;

impl Default for DedupService {
    fn default() -> Self {
        Self
    }
}

impl DedupService {
    pub fn decide(
        &self,
        repository: &SqliteRepository,
        hash: &str,
    ) -> Result<DedupDecision, AppError> {
        // 检查是否存在（未删除的）旧记录：命中则置顶已有项，否则插入新记录。
        //
        // 早期版本曾基于 created_at 维护一个 8 秒的 Skip 窗口用于"防抖"，但它与
        // bump_item 刷新 created_at 冲突：连续复制相同内容间隔小于 8 秒时会一直命中
        // Skip，导致该条目永远无法置顶（表现为"有时更新有时不更新"）。
        // 防抖职责现已由剪贴板序列号检测（ClipboardMonitor）和应用自写回过滤
        // （SelfWriteGuard）承担，这里不再需要。
        if let Some(existing_id) = repository.find_existing_by_hash(hash)? {
            return Ok(DedupDecision::BumpExisting(existing_id));
        }

        Ok(DedupDecision::StoreNew)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::{DedupDecision, DedupService};
    use crate::{
        repository::sqlite_repository::SqliteRepository,
        services::normalize_service::NormalizeService,
    };

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!("floatpaste-dedup-{}.db", Uuid::new_v4()))
    }

    /// 通过公共 API 入库一条文本记录，返回其归一化 hash。
    fn seed_text_item(repository: &SqliteRepository, text: &str) -> String {
        let item = NormalizeService::normalize_text(text, None).unwrap();
        repository.save_text_item(&item).unwrap();
        item.normalized.hash
    }

    /// 回归测试：刚入库的记录，立即再次 decide 必须返回 BumpExisting，
    /// 而不是被任何"短期窗口"逻辑跳过。
    ///
    /// 复现条件（修复前）：旧版本基于 created_at 维护 8 秒 Skip 窗口，
    /// seed 后 created_at=now，decide 立刻命中 Skip，导致重新复制相同内容时
    /// 已有项无法置顶，用户表现为"有时更新有时不更新"。
    #[test]
    fn decide_bumps_existing_item_even_just_after_capture() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();

        let hash = seed_text_item(&repository, "shared content");
        let existing_id = repository
            .find_existing_by_hash(&hash)
            .unwrap()
            .expect("已入库的记录应能被找到");

        let first = DedupService::default().decide(&repository, &hash).unwrap();
        assert!(matches!(first, DedupDecision::BumpExisting(_)));

        // 紧接着第二次调用（无任何睡眠）——修复前会返回 Skip
        let second = DedupService::default().decide(&repository, &hash).unwrap();
        match second {
            DedupDecision::BumpExisting(id) => assert_eq!(id, existing_id),
            other => panic!("期望 BumpExisting，实际: {other:?}"),
        }

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn decide_stores_new_when_no_existing_record() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();

        let decision = DedupService::default()
            .decide(&repository, "never-seen-hash")
            .unwrap();
        assert!(matches!(decision, DedupDecision::StoreNew));

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    /// 回归测试：删除一条记录后再复制相同内容，应作为全新记录存储，
    /// 而不是被 `find_existing_by_hash` 命中（它必须忽略软删除行）。
    #[test]
    fn decide_stores_new_after_existing_item_was_deleted() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();

        let hash = seed_text_item(&repository, "to be deleted");
        let existing_id = repository
            .find_existing_by_hash(&hash)
            .unwrap()
            .expect("已入库的记录应能被找到");

        // 删除前的决策：应命中已有项
        assert!(matches!(
            DedupService::default().decide(&repository, &hash).unwrap(),
            DedupDecision::BumpExisting(_)
        ));

        // 软删除后，find_existing_by_hash 不应再返回它
        repository.delete_item(&existing_id).unwrap();
        assert_eq!(
            repository.find_existing_by_hash(&hash).unwrap(),
            None,
            "软删除的记录不应被去重视为已有项"
        );

        // 再次 decide 应返回 StoreNew
        let decision = DedupService::default().decide(&repository, &hash).unwrap();
        assert!(matches!(decision, DedupDecision::StoreNew));

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn decide_treats_whitespace_variants_as_same_content() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();

        // "Alpha\nBeta" 与 " Alpha   Beta " 经 split_whitespace 折叠后 hash 相同
        let first = NormalizeService::normalize_text("Alpha\nBeta", None).unwrap();
        repository
            .save_text_item(&first)
            .unwrap();

        let second = NormalizeService::normalize_text(" Alpha   Beta ", None).unwrap();
        let decision = DedupService::default()
            .decide(&repository, &second.normalized.hash)
            .unwrap();
        assert!(matches!(decision, DedupDecision::BumpExisting(_)));

        drop(repository);
        fs::remove_file(path).unwrap();
    }
}
