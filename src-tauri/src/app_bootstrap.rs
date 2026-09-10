use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};

use tauri::{App, AppHandle, Emitter, Manager};
use tracing::{info, warn};

use floatpaste_core::state::CoreState;

use crate::{
    domain::{
        editor_session::EditorSession,
        error::AppError,
        events::{ClipsChangedPayload, CLIPS_CHANGED_EVENT},
        search_session::SearchSession,
    },
    launch_mode::LaunchMode,
    repository::sqlite_repository::SqliteRepository,
    services::{
        image_storage::ImageStorage, settings_service::SettingsService, tray_service::TrayService,
        window_coordinator::WindowCoordinator,
    },
};

/// 壳层应用状态 = 核心状态（仓储/设置/图片存储）+ Tauri 窗口会话状态。
///
/// 核心成员经 `Deref` 直接访问（`state.repository`、`state.current_settings()` 等），
/// 窗口会话成员是 Tauri 特有的编排概念，不进入 core。
#[derive(Clone)]
pub struct AppState {
    pub core: CoreState,
    picker: PickerState,
    search: SearchState,
    editor: EditorState,
}

impl std::ops::Deref for AppState {
    type Target = CoreState;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
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
    active: Arc<std::sync::atomic::AtomicBool>,
    session_shortcuts_registered: Arc<std::sync::atomic::AtomicBool>,
}

/// Search 搜索窗口的会话状态。
#[derive(Clone, Default)]
struct SearchState {
    session: Arc<Mutex<Option<SearchSession>>>,
    active: Arc<std::sync::atomic::AtomicBool>,
    session_monitor_token: Arc<std::sync::atomic::AtomicU64>,
    focus_loss_ignore_deadline: Arc<Mutex<Option<Instant>>>,
}

/// Editor 编辑器窗口的会话状态。
#[derive(Clone, Default)]
struct EditorState {
    session: Arc<Mutex<Option<EditorSession>>>,
    active: Arc<std::sync::atomic::AtomicBool>,
}

impl AppState {
    pub fn new(
        repository: SqliteRepository,
        image_storage: ImageStorage,
        settings: crate::domain::settings::UserSetting,
    ) -> Self {
        Self {
            core: CoreState::new(repository, image_storage, settings),
            picker: PickerState::default(),
            search: SearchState::default(),
            editor: EditorState::default(),
        }
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
    start_clipboard_monitor(app.handle().clone(), state.clone())?;
    crate::services::retention_service::RetentionService::start(state.core.clone());

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

/// 启动剪贴板监听：录入成功后通过 Tauri 事件广播给前端窗口。
fn start_clipboard_monitor(handle: AppHandle, state: AppState) -> Result<(), AppError> {
    let on_upsert: crate::platform::windows::clipboard_monitor::ClipUpsertSink =
        Arc::new(move |detail| {
            let _ = handle.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail));
        });
    crate::platform::windows::clipboard_monitor::ClipboardMonitor::start(
        state.core.clone(),
        on_upsert,
    )
}

fn seed_welcome_entry(app: &AppHandle, repository: &SqliteRepository) -> Result<(), AppError> {
    let Some(detail) =
        crate::services::history_service::HistoryService::ensure_welcome_item(repository)?
    else {
        return Ok(());
    };

    let _ = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail));
    Ok(())
}

fn resolve_app_data_dir(app: &App) -> Result<PathBuf, AppError> {
    if let Ok(path) = app.path().app_data_dir() {
        return Ok(path);
    }

    Ok(std::env::current_dir()?.join(".floatpaste-data"))
}
