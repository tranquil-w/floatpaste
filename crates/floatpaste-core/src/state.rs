//! 核心应用状态：仓储、图片存储、设置缓存与自写保护。
//!
//! GUI 壳（Tauri / Slint）各自持有窗口会话状态，但都共享同一个
//! [`CoreState`]；壳层状态可通过 `Deref` 直接访问核心成员。

use std::{
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
};

use crate::{
    domain::{error::AppError, settings::UserSetting},
    repository::sqlite_repository::SqliteRepository,
    services::{image_storage::ImageStorage, privacy_service::SelfWriteGuard},
};

/// 与 GUI 无关的核心状态，`Clone` 为廉价快照（内部全是 Arc）。
#[derive(Clone)]
pub struct CoreState {
    pub repository: SqliteRepository,
    pub image_storage: ImageStorage,
    settings: Arc<RwLock<UserSetting>>,
    self_write_guard: SelfWriteGuard,
    quitting: Arc<AtomicBool>,
}

impl CoreState {
    pub fn new(
        repository: SqliteRepository,
        image_storage: ImageStorage,
        settings: UserSetting,
    ) -> Self {
        Self {
            repository,
            image_storage,
            settings: Arc::new(RwLock::new(settings)),
            self_write_guard: SelfWriteGuard::default(),
            quitting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn current_settings(&self) -> Result<UserSetting, AppError> {
        Ok(self.settings.read()?.clone())
    }

    pub fn update_settings(&self, next_value: UserSetting) -> Result<UserSetting, AppError> {
        let sanitized = next_value.sanitized();
        self.repository.save_settings(&sanitized)?;
        *self.settings.write()? = sanitized.clone();
        Ok(sanitized)
    }

    pub fn self_write_guard(&self) -> SelfWriteGuard {
        self.self_write_guard.clone()
    }

    pub fn begin_quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }
}
