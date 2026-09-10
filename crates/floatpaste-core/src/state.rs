//! 核心应用状态：仓储、图片存储、设置缓存与自写保护。
//!
//! GUI 壳（Tauri / Slint）各自持有窗口会话状态，但都共享同一个
//! [`CoreState`]；壳层状态可通过 `Deref` 直接访问核心成员。

use std::{
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
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
    /// 托盘"切换监听"的最近一次受理时间。Windows 菜单存在一次点击触发两次
    /// 事件的上游缺陷，toggle 两次会相互抵消导致状态看似不变，用时间窗去抖。
    monitoring_toggle_gate: Arc<::std::sync::Mutex<DebounceGate>>,
}

/// 时间窗去抖闸门：窗口期内重复请求被拒绝。
/// 用于过滤 Windows 托盘菜单一次点击触发两次事件的上游缺陷。
#[derive(Default)]
pub struct DebounceGate {
    last_accepted: Option<Instant>,
}

impl DebounceGate {
    pub fn try_accept(&mut self, window: Duration) -> bool {
        match self.last_accepted {
            Some(at) if at.elapsed() < window => false,
            _ => {
                self.last_accepted = Some(Instant::now());
                true
            }
        }
    }
}

/// 托盘"切换监听"的去抖窗口：双触发的第二次事件通常在首次后约 1 秒内到达，
/// 1.5 秒窗口可覆盖，同时不影响人工连续点击（重新打开菜单已超过该间隔）。
const MONITORING_TOGGLE_DEBOUNCE: Duration = Duration::from_millis(1500);

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
            monitoring_toggle_gate: Arc::default(),
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

    /// 距上次受理不足去抖窗口的"切换监听"事件视为重复（一次菜单点击的双触发），忽略。
    pub fn should_accept_monitoring_toggle(&self) -> bool {
        let Ok(mut gate) = self.monitoring_toggle_gate.lock() else {
            return false;
        };
        gate.try_accept(MONITORING_TOGGLE_DEBOUNCE)
    }
}

#[cfg(test)]
mod tests {
    use super::{DebounceGate, MONITORING_TOGGLE_DEBOUNCE};
    use std::time::Duration;

    #[test]
    fn debounce_gate_rejects_rapid_repeat_and_accepts_after_window() {
        let mut gate = DebounceGate::default();

        // 首次事件受理（对应一次菜单点击的第一次触发）
        assert!(gate.try_accept(MONITORING_TOGGLE_DEBOUNCE));
        // 窗口期内的第二次事件被拒绝（对应同一点击的重复触发）
        assert!(!gate.try_accept(MONITORING_TOGGLE_DEBOUNCE));
        assert!(!gate.try_accept(MONITORING_TOGGLE_DEBOUNCE));
        // 窗口过期后重新受理（零窗口等价于立即过期）
        assert!(gate.try_accept(Duration::ZERO));
    }
}
