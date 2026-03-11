use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex, RwLock},
};

use tauri::{App, Manager};
use tracing::{info, warn};

use crate::{
    domain::{error::AppError, settings::UserSetting},
    launch_mode::LaunchMode,
    platform::windows::clipboard_monitor::ClipboardMonitor,
    repository::sqlite_repository::SqliteRepository,
    services::{
        image_storage::ImageStorage,
        privacy_service::SelfWriteGuard, settings_service::SettingsService,
        tray_service::TrayService, window_coordinator::WindowCoordinator,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub repository: SqliteRepository,
    pub image_storage: ImageStorage,
    settings: Arc<RwLock<UserSetting>>,
    self_write_guard: SelfWriteGuard,
    picker_session: Arc<Mutex<PickerSession>>,
    picker_active: Arc<AtomicBool>,
    quitting: Arc<AtomicBool>,
}

#[derive(Debug, Default, Clone)]
pub struct PickerSession {
    pub target_window_hwnd: Option<isize>,
    pub reopen_manager_on_close: bool,
}

impl AppState {
    pub fn new(repository: SqliteRepository, image_storage: ImageStorage, settings: UserSetting) -> Self {
        Self {
            repository,
            image_storage,
            settings: Arc::new(RwLock::new(settings)),
            self_write_guard: SelfWriteGuard::default(),
            picker_session: Arc::new(Mutex::new(PickerSession::default())),
            picker_active: Arc::new(AtomicBool::new(false)),
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

    pub fn set_picker_session(
        &self,
        hwnd: Option<isize>,
        reopen_manager_on_close: bool,
    ) -> Result<(), AppError> {
        let mut session = self.picker_session.lock()?;
        session.target_window_hwnd = hwnd;
        session.reopen_manager_on_close = reopen_manager_on_close;
        Ok(())
    }

    pub fn picker_session(&self) -> Result<PickerSession, AppError> {
        Ok(self.picker_session.lock()?.clone())
    }

    pub fn begin_picker_activation(&self) {
        self.picker_active.store(true, Ordering::SeqCst);
    }

    pub fn end_picker_activation(&self) {
        self.picker_active.store(false, Ordering::SeqCst);
    }

    pub fn is_picker_active(&self) -> bool {
        self.picker_active.load(Ordering::SeqCst)
    }
    pub fn begin_quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }
}

pub fn bootstrap(app: &mut App, launch_mode: LaunchMode) -> Result<(), AppError> {
    let data_dir = resolve_app_data_dir(app)?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("floatpaste.db");
    let repository = SqliteRepository::new(&db_path)?;
    let image_storage = ImageStorage::new(data_dir.clone())?;
    let settings = repository.load_settings()?;
    let state = AppState::new(repository, image_storage, settings);

    app.manage(state.clone());
    WindowCoordinator::configure_existing_windows(&app.handle());
    if let Err(error) = SettingsService::apply_runtime_side_effects(&app.handle(), &state) {
        warn!("启动时同步运行设置失败，应用将继续运行，但部分系统能力暂不可用: {error}");
    }
    TrayService::setup(&app.handle())?;
    ClipboardMonitor::start(app.handle().clone(), state)?;

    if !launch_mode.is_silent() {
        WindowCoordinator::open_manager(&app.handle())?;
    }

    info!("FloatPaste MVP 已初始化，数据库路径: {}", db_path.display());
    Ok(())
}

fn resolve_app_data_dir(app: &App) -> Result<PathBuf, AppError> {
    if let Ok(path) = app.path().app_data_dir() {
        return Ok(path);
    }

    Ok(std::env::current_dir()?.join(".floatpaste-data"))
}
