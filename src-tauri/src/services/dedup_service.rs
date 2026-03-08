use chrono::{Duration, Utc};

use crate::{domain::error::AppError, repository::sqlite_repository::SqliteRepository};

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
    pub fn should_store(
        &self,
        repository: &SqliteRepository,
        hash: &str,
    ) -> Result<bool, AppError> {
        let cutoff = Utc::now() - self.window;
        Ok(!repository.has_recent_hash(hash, cutoff.timestamp_millis())?)
    }
}
