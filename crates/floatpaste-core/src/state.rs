//! 核心应用状态：仓储、图片存储、设置缓存与自写保护。
//!
//! GUI 壳（Tauri / Slint）各自持有窗口会话状态，但都共享同一个
//! [`CoreState`]；壳层状态可通过 `Deref` 直接访问核心成员。

use std::{
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
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
    privacy: Arc<RwLock<PrivacyState>>,
}

/// 锁屏、暂停及清除使旧采集/上屏失效；读守卫覆盖最终副作用，写守卫建立边界。
#[derive(Default)]
pub struct PrivacyState {
    pub generation: u64,
    pub session_locked: bool,
    pub ignored_clipboard_sequence: Option<u32>,
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
            privacy: Arc::default(),
        }
    }

    pub fn current_settings(&self) -> Result<UserSetting, AppError> {
        Ok(self.settings.read()?.clone())
    }

    pub fn update_settings(&self, next_value: UserSetting) -> Result<UserSetting, AppError> {
        let sanitized = next_value.sanitized();
        let mut privacy = self.privacy_write()?;
        let was_paused = self.settings.read()?.pause_monitoring;
        self.repository.save_settings(&sanitized)?;
        *self.settings.write()? = sanitized.clone();
        if was_paused != sanitized.pause_monitoring {
            privacy.generation += 1;
            privacy.ignored_clipboard_sequence =
                Some(crate::platform::windows::clipboard_formats::clipboard_sequence());
        }
        Ok(sanitized)
    }

    pub fn privacy_read(&self) -> Result<RwLockReadGuard<'_, PrivacyState>, AppError> {
        Ok(self.privacy.read()?)
    }

    pub fn privacy_write(&self) -> Result<RwLockWriteGuard<'_, PrivacyState>, AppError> {
        Ok(self.privacy.write()?)
    }

    pub fn privacy_generation(&self) -> u64 {
        self.privacy
            .read()
            .map(|state| state.generation)
            .unwrap_or(u64::MAX)
    }

    pub fn session_locked(&self) -> bool {
        self.privacy
            .read()
            .map(|state| state.session_locked)
            .unwrap_or(true)
    }

    pub fn set_session_locked(&self, locked: bool) -> Result<(), AppError> {
        let mut state = self.privacy_write()?;
        if state.session_locked != locked {
            state.session_locked = locked;
            state.generation += 1;
            state.ignored_clipboard_sequence =
                Some(crate::platform::windows::clipboard_formats::clipboard_sequence());
        }
        Ok(())
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
