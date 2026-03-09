use chrono::{Duration, Utc};

use crate::{domain::error::AppError, repository::sqlite_repository::SqliteRepository};

/// 去重决策结果
pub enum DedupDecision {
    /// 全新内容，应插入新记录
    StoreNew,
    /// 内容已存在，刷新已有记录（返回已有记录的 id）
    BumpExisting(String),
    /// 短期内重复，跳过
    Skip,
}

pub struct DedupService {
    window: Duration,
}

impl Default for DedupService {
    fn default() -> Self {
        Self {
            window: Duration::seconds(8),
        }
    }
}

impl DedupService {
    pub fn decide(
        &self,
        repository: &SqliteRepository,
        hash: &str,
    ) -> Result<DedupDecision, AppError> {
        let cutoff = Utc::now() - self.window;

        // 8 秒窗口内有相同 hash → 直接跳过
        if repository.has_recent_hash(hash, cutoff.timestamp_millis())? {
            return Ok(DedupDecision::Skip);
        }

        // 检查是否存在（未删除的）旧记录
        if let Some(existing_id) = repository.find_existing_by_hash(hash)? {
            return Ok(DedupDecision::BumpExisting(existing_id));
        }

        Ok(DedupDecision::StoreNew)
    }
}
