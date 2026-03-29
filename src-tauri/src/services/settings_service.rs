use tauri::{AppHandle, Emitter};

use crate::{
    app_bootstrap::AppState,
    domain::{error::AppError, events::SETTINGS_CHANGED_EVENT},
    services::{shortcut_manager::ShortcutManager, startup_service::StartupService},
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
        app.emit(SETTINGS_CHANGED_EVENT, &settings)
            .map_err(|error| AppError::Message(error.to_string()))?;
        Ok(())
    }
}
