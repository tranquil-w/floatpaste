use tauri::{AppHandle, State};
use tracing::warn;

use crate::{
    app_bootstrap::AppState,
    services::{shortcut_manager::ShortcutManager, window_coordinator::WindowCoordinator},
};

fn map_error(error: impl ToString) -> String {
    error.to_string()
}

#[tauri::command]
pub fn show_picker(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::show_picker(&app, &state).map_err(map_error)?;
    if let Err(error) = ShortcutManager::register_picker_session_shortcuts(&app) {
        warn!("通过命令打开 Picker 时注册会话快捷键失败: {error}");
    }
    Ok(())
}

#[tauri::command]
pub fn show_picker_from_manager(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::show_picker(&app, &state).map_err(map_error)?;
    if let Err(error) = ShortcutManager::register_picker_session_shortcuts(&app) {
        warn!("从资料库打开 Picker 时注册会话快捷键失败: {error}");
    }
    Ok(())
}

#[tauri::command]
pub fn hide_picker(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    ShortcutManager::unregister_picker_session_shortcuts(&app);
    WindowCoordinator::hide_picker_and_restore_target(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn open_manager(_state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::open_manager(&app).map_err(map_error)
}

#[tauri::command]
pub fn open_manager_from_picker(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    ShortcutManager::unregister_picker_session_shortcuts(&app);
    WindowCoordinator::hide_picker_and_open_manager(&app, &state).map_err(map_error)
}
