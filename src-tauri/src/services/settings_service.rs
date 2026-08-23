use tauri::{AppHandle, Emitter};

use crate::{
    app_bootstrap::AppState,
    domain::{error::AppError, events::SETTINGS_CHANGED_EVENT},
    services::{
        shortcut_manager::ShortcutManager, startup_service::StartupService, tray_service::TrayService,
    },
};

pub struct SettingsService;

impl SettingsService {
    pub fn apply_runtime_side_effects(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        let settings = state.current_settings()?;
        let search = settings
            .search_shortcut_enabled
            .then(|| settings.search_shortcut.as_str());
        ShortcutManager::sync_registered_shortcuts(app, &settings.shortcut, search)?;
        StartupService::sync_from_settings(&settings)?;
        // 托盘菜单文案跟随设置（如监听状态），失败不阻塞其余副作用
        TrayService::refresh_menu(app);
        app.emit(SETTINGS_CHANGED_EVENT, &settings)?;
        Ok(())
    }
}
