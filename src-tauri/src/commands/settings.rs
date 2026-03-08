use tauri::{AppHandle, State};

use crate::{
    app_bootstrap::AppState, domain::settings::UserSetting,
    services::shortcut_manager::ShortcutManager,
};

fn map_error(error: impl ToString) -> String {
    error.to_string()
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<UserSetting, String> {
    state.current_settings().map_err(map_error)
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: UserSetting,
) -> Result<UserSetting, String> {
    let previous_settings = state.current_settings().map_err(map_error)?;
    let next_value = state.update_settings(payload).map_err(map_error)?;

    if let Err(error) = ShortcutManager::update_from_settings(&app, &state) {
        let _ = state.update_settings(previous_settings.clone());
        let _ = ShortcutManager::update_from_settings(&app, &state);
        return Err(map_error(error));
    }

    Ok(next_value)
}

#[tauri::command]
pub fn pause_monitoring(app: AppHandle, state: State<'_, AppState>) -> Result<UserSetting, String> {
    let previous_settings = state.current_settings().map_err(map_error)?;
    let mut settings = previous_settings.clone();
    settings.pause_monitoring = true;
    let next_value = state.update_settings(settings).map_err(map_error)?;

    if let Err(error) = ShortcutManager::update_from_settings(&app, &state) {
        let _ = state.update_settings(previous_settings);
        let _ = ShortcutManager::update_from_settings(&app, &state);
        return Err(map_error(error));
    }

    Ok(next_value)
}

#[tauri::command]
pub fn resume_monitoring(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UserSetting, String> {
    let previous_settings = state.current_settings().map_err(map_error)?;
    let mut settings = previous_settings.clone();
    settings.pause_monitoring = false;
    let next_value = state.update_settings(settings).map_err(map_error)?;

    if let Err(error) = ShortcutManager::update_from_settings(&app, &state) {
        let _ = state.update_settings(previous_settings);
        let _ = ShortcutManager::update_from_settings(&app, &state);
        return Err(map_error(error));
    }

    Ok(next_value)
}
