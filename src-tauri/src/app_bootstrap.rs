use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use tauri::{App, AppHandle, Emitter, Manager};
use tracing::{info, warn};

use crate::{
    domain::{
        editor_session::EditorSession, error::AppError,
        events::{ClipsChangedPayload, CLIPS_CHANGED_EVENT}, search_session::SearchSession,
        settings::UserSetting,
    },
    launch_mode::LaunchMode,
    platform::windows::clipboard_monitor::ClipboardMonitor,
    repository::sqlite_repository::SqliteRepository,
    services::{
        image_storage::ImageStorage, privacy_service::SelfWriteGuard,
        retention_service::RetentionService, settings_service::SettingsService,
        tray_service::TrayService, window_coordinator::WindowCoordinator,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub repository: SqliteRepository,
    pub image_storage: ImageStorage,
    settings: Arc<RwLock<UserSetting>>,
    self_write_guard: SelfWriteGuard,
    quitting: Arc<AtomicBool>,
    picker: PickerState,
    search: SearchState,
    editor: EditorState,
    /// 托盘"切换监听"的最近一次受理时间。Windows 菜单存在一次点击触发两次
    /// 事件的上游缺陷，toggle 两次会相互抵消导致状态看似不变，用时间窗去抖。
    monitoring_toggle_gate: Arc<Mutex<DebounceGate>>,
}

#[derive(Debug, Default, Clone)]
pub struct PickerSession {
    pub target_window_hwnd: Option<isize>,
    pub target_focus_hwnd: Option<isize>,
}

/// Picker 速贴面板的会话状态。
#[derive(Clone, Default)]
struct PickerState {
    session: Arc<Mutex<PickerSession>>,
    active: Arc<AtomicBool>,
    session_shortcuts_registered: Arc<AtomicBool>,
}

/// Search 搜索窗口的会话状态。
#[derive(Clone, Default)]
struct SearchState {
    session: Arc<Mutex<Option<SearchSession>>>,
    active: Arc<AtomicBool>,
    session_monitor_token: Arc<AtomicU64>,
    focus_loss_ignore_deadline: Arc<Mutex<Option<Instant>>>,
}

/// Editor 编辑器窗口的会话状态。
#[derive(Clone, Default)]
struct EditorState {
    session: Arc<Mutex<Option<EditorSession>>>,
    active: Arc<AtomicBool>,
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

impl AppState {
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
            picker: PickerState::default(),
            search: SearchState::default(),
            editor: EditorState::default(),
            monitoring_toggle_gate: Arc::default(),
        }
    }

    /// 距上次受理不足去抖窗口的"切换监听"事件视为重复（一次菜单点击的双触发），忽略。
    pub fn should_accept_monitoring_toggle(&self) -> bool {
        let Ok(mut gate) = self.monitoring_toggle_gate.lock() else {
            return false;
        };
        gate.try_accept(MONITORING_TOGGLE_DEBOUNCE)
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

    pub fn set_picker_session(
        &self,
        hwnd: Option<isize>,
        focus_hwnd: Option<isize>,
    ) -> Result<(), AppError> {
        let mut session = self.picker.session.lock()?;
        session.target_window_hwnd = hwnd;
        session.target_focus_hwnd = focus_hwnd;
        Ok(())
    }

    pub fn picker_session(&self) -> Result<PickerSession, AppError> {
        Ok(self.picker.session.lock()?.clone())
    }

    pub fn begin_picker_activation(&self) {
        self.picker.active.store(true, Ordering::SeqCst);
    }

    pub fn end_picker_activation(&self) {
        self.picker.active.store(false, Ordering::SeqCst);
    }

    pub fn is_picker_active(&self) -> bool {
        self.picker.active.load(Ordering::SeqCst)
    }

    pub fn set_picker_session_shortcuts_registered(&self, registered: bool) {
        self.picker
            .session_shortcuts_registered
            .store(registered, Ordering::SeqCst);
    }

    pub fn picker_session_shortcuts_registered(&self) -> bool {
        self.picker
            .session_shortcuts_registered
            .load(Ordering::SeqCst)
    }

    pub fn begin_quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }

    pub fn set_search_session(&self, session: SearchSession) -> Result<(), AppError> {
        let mut current = self.search.session.lock()?;
        *current = Some(session);
        Ok(())
    }

    pub fn search_session(&self) -> Result<Option<SearchSession>, AppError> {
        Ok(self.search.session.lock()?.clone())
    }

    pub fn clear_search_session(&self) -> Result<(), AppError> {
        let mut current = self.search.session.lock()?;
        *current = None;
        Ok(())
    }

    pub fn begin_search_activation(&self) {
        self.search.active.store(true, Ordering::SeqCst);
    }

    pub fn end_search_activation(&self) {
        self.search.active.store(false, Ordering::SeqCst);
    }

    pub fn is_search_active(&self) -> bool {
        self.search.active.load(Ordering::SeqCst)
    }

    pub fn next_search_session_monitor_token(&self) -> u64 {
        self.search
            .session_monitor_token
            .fetch_add(1, Ordering::SeqCst)
            + 1
    }

    pub fn current_search_session_monitor_token(&self) -> u64 {
        self.search.session_monitor_token.load(Ordering::SeqCst)
    }

    pub fn mark_search_focus_loss_ignored_for(&self, duration: Duration) -> Result<(), AppError> {
        let mut deadline = self.search.focus_loss_ignore_deadline.lock()?;
        *deadline = Some(Instant::now() + duration);
        Ok(())
    }

    pub fn should_ignore_search_focus_loss(&self) -> Result<bool, AppError> {
        let mut deadline = self.search.focus_loss_ignore_deadline.lock()?;
        let Some(current_deadline) = *deadline else {
            return Ok(false);
        };

        if Instant::now() <= current_deadline {
            *deadline = None;
            return Ok(true);
        }

        *deadline = None;
        Ok(false)
    }

    pub fn set_editor_session(&self, session: EditorSession) -> Result<(), AppError> {
        let mut current = self.editor.session.lock()?;
        *current = Some(session);
        Ok(())
    }

    pub fn editor_session(&self) -> Result<Option<EditorSession>, AppError> {
        Ok(self.editor.session.lock()?.clone())
    }

    pub fn clear_editor_session(&self) -> Result<(), AppError> {
        let mut current = self.editor.session.lock()?;
        *current = None;
        Ok(())
    }

    pub fn begin_editor_activation(&self) {
        self.editor.active.store(true, Ordering::SeqCst);
    }

    pub fn end_editor_activation(&self) {
        self.editor.active.store(false, Ordering::SeqCst);
    }
}

pub fn bootstrap(app: &mut App, launch_mode: LaunchMode) -> Result<(), AppError> {
    let data_dir = resolve_app_data_dir(app)?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("floatpaste.db");
    let repository = SqliteRepository::new(&db_path)?;
    let image_storage = ImageStorage::new(data_dir.clone())?;
    let settings = repository.load_settings()?;
    let state = AppState::new(repository.clone(), image_storage, settings);

    app.manage(state.clone());
    WindowCoordinator::configure_existing_windows(&app.handle());
    if let Err(error) = SettingsService::apply_runtime_side_effects(&app.handle(), &state) {
        warn!("启动时同步运行设置失败，应用将继续运行，但部分系统能力暂不可用: {error}");
    }
    TrayService::setup(&app.handle())?;
    ClipboardMonitor::start(app.handle().clone(), state.clone())?;
    RetentionService::start(state.clone());

    if let Err(error) = seed_welcome_entry(&app.handle(), &repository) {
        warn!("初始化欢迎记录失败: {error}");
    }

    // 正常启动显示速贴面板；静默启动（开机自启携带 --silent）仅驻留托盘，不弹任何窗口
    if !launch_mode.is_silent() {
        WindowCoordinator::activate_picker(&app.handle(), &state)?;
    }

    info!("FloatPaste MVP 已初始化，数据库路径: {}", db_path.display());
    Ok(())
}

fn seed_welcome_entry(app: &AppHandle, repository: &SqliteRepository) -> Result<(), AppError> {
    if !repository.list_recent(1)?.is_empty() {
        return Ok(());
    }

    let Some(text_item) = crate::services::normalize_service::NormalizeService::normalize_text(
        "欢迎使用 FloatPaste 👋  [↑↓] 导航记录 · [Enter] 快速粘贴 · [1~9] 数字键直达 · [Tab] 打开完整资料库 · [Esc] 随时退出",
        Some("使用指引".to_string()),
    ) else {
        return Ok(());
    };

    let detail = repository.save_text_item(&text_item)?;
    repository.set_favorited(&detail.id, true)?;
    let _ = app.emit(
        CLIPS_CHANGED_EVENT,
        ClipsChangedPayload::upserted(&detail),
    );
    Ok(())
}

fn resolve_app_data_dir(app: &App) -> Result<PathBuf, AppError> {
    if let Ok(path) = app.path().app_data_dir() {
        return Ok(path);
    }

    Ok(std::env::current_dir()?.join(".floatpaste-data"))
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
