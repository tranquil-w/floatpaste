use tauri::{AppHandle, State};
use tracing::{error, warn};

use crate::{
    app_bootstrap::AppState, domain::settings::UserSetting,
    services::settings_service::SettingsService,
};

fn map_error(error: impl ToString) -> String {
    error.to_string()
}

fn persist_and_apply_settings(
    app: &AppHandle,
    state: &AppState,
    payload: UserSetting,
) -> Result<UserSetting, String> {
    let previous_settings = state.current_settings().map_err(map_error)?;
    let next_value = state.update_settings(payload).map_err(map_error)?;

    if let Err(error) = SettingsService::apply_runtime_side_effects(app, state) {
        warn!("应用设置副作用失败，准备回滚到旧配置: {error}");

        if let Err(rollback_error) = state.update_settings(previous_settings.clone()) {
            error!("设置持久化回滚失败，当前配置可能处于不一致状态: {rollback_error}");
        } else if let Err(resync_error) = SettingsService::apply_runtime_side_effects(app, state) {
            error!("设置回滚后重新同步运行时副作用失败: {resync_error}");
        }

        return Err(map_error(error));
    }

    Ok(next_value)
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
    persist_and_apply_settings(&app, &state, payload)
}

#[tauri::command]
pub fn pause_monitoring(app: AppHandle, state: State<'_, AppState>) -> Result<UserSetting, String> {
    let mut settings = state.current_settings().map_err(map_error)?;
    settings.pause_monitoring = true;
    persist_and_apply_settings(&app, &state, settings)
}

#[tauri::command]
pub fn resume_monitoring(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UserSetting, String> {
    let mut settings = state.current_settings().map_err(map_error)?;
    settings.pause_monitoring = false;
    persist_and_apply_settings(&app, &state, settings)
}
