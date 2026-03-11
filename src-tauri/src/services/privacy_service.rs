use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::domain::{
    clip_item::{NormalizedClipFile, NormalizedClipImage, NormalizedClipText},
    error::AppError,
    settings::UserSetting,
};

#[derive(Debug, Clone)]
struct SuppressedHash {
    hash: String,
    expires_at: Instant,
}

#[derive(Clone, Default)]
pub struct SelfWriteGuard {
    inner: Arc<Mutex<Vec<SuppressedHash>>>,
}

impl SelfWriteGuard {
    pub fn suppress_hash(&self, hash: String, ttl: Duration) -> Result<(), AppError> {
        let mut guard = self.inner.lock()?;
        let now = Instant::now();
        guard.retain(|entry| entry.expires_at > now);
        guard.push(SuppressedHash {
            hash,
            expires_at: now + ttl,
        });
        Ok(())
    }

    pub fn is_suppressed(&self, hash: &str) -> Result<bool, AppError> {
        let mut guard = self.inner.lock()?;
        let now = Instant::now();
        guard.retain(|entry| entry.expires_at > now);
        Ok(guard.iter().any(|entry| entry.hash == hash))
    }
}

pub struct PrivacyService;

impl PrivacyService {
    pub fn should_capture(
        settings: &UserSetting,
        normalized: &NormalizedClipText,
        source_app: Option<&str>,
        self_write_guard: &SelfWriteGuard,
    ) -> Result<bool, AppError> {
        Self::should_capture_by_hash(
            settings,
            &normalized.hash,
            source_app,
            self_write_guard,
        )
    }

    pub fn should_capture_image(
        settings: &UserSetting,
        normalized: &NormalizedClipImage,
        source_app: Option<&str>,
        self_write_guard: &SelfWriteGuard,
    ) -> Result<bool, AppError> {
        Self::should_capture_by_hash(
            settings,
            &normalized.hash,
            source_app,
            self_write_guard,
        )
    }

    pub fn should_capture_file(
        settings: &UserSetting,
        normalized: &NormalizedClipFile,
        source_app: Option<&str>,
        self_write_guard: &SelfWriteGuard,
    ) -> Result<bool, AppError> {
        Self::should_capture_by_hash(
            settings,
            &normalized.hash,
            source_app,
            self_write_guard,
        )
    }

    fn should_capture_by_hash(
        settings: &UserSetting,
        hash: &str,
        source_app: Option<&str>,
        self_write_guard: &SelfWriteGuard,
    ) -> Result<bool, AppError> {
        if settings.pause_monitoring {
            return Ok(false);
        }

        if self_write_guard.is_suppressed(hash)? {
            return Ok(false);
        }

        if let Some(app_name) = source_app {
            let excluded = settings
                .excluded_apps
                .iter()
                .any(|item| item.eq_ignore_ascii_case(app_name));
            if excluded {
                return Ok(false);
            }
        }

        Ok(true)
    }
}
