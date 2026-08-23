use tauri::{AppHandle, State};
use std::time::Duration;
use std::collections::HashMap;

use crate::{
    app_bootstrap::AppState,
    services::{tooltip_window::TooltipWindow, window_coordinator::WindowCoordinator},
};

use super::map_error;

#[tauri::command]
pub fn show_picker(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::activate_picker(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn hide_picker(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    // hide_picker_and_restore_target 内部会注销会话快捷键
    WindowCoordinator::hide_picker_and_restore_target(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn open_settings(_state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::open_settings(&app).map_err(map_error)
}

#[tauri::command]
pub fn open_editor_from_picker(
    item_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    WindowCoordinator::open_editor_from_picker(&app, &state, item_id).map_err(map_error)
}

#[tauri::command]
pub fn open_editor_from_search(
    item_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    WindowCoordinator::open_editor_from_search(&app, &state, item_id).map_err(map_error)
}

#[tauri::command]
pub fn hide_editor(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::hide_editor_and_restore_source(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn open_search_global(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::open_search_global(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn hide_search(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::hide_search_and_restore_target(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn prepare_search_window_drag(state: State<'_, AppState>) -> Result<(), String> {
    state
        .mark_search_focus_loss_ignored_for(Duration::from_millis(1500))
        .map_err(map_error)
}

#[tauri::command]
pub async fn show_tooltip(
    app: AppHandle,
    request_id: u32,
    x: f64,
    y: f64,
    html: String,
    theme: String,
    theme_vars: HashMap<String, String>,
) -> Result<(), String> {
    TooltipWindow::show_tooltip(&app, request_id, x, y, html, &theme, &theme_vars).map_err(map_error)
}

#[tauri::command]
pub fn tooltip_ready(app: AppHandle, request_id: u32, width: u32, height: u32) -> Result<(), String> {
    TooltipWindow::on_tooltip_ready(&app, request_id, width, height).map_err(map_error)
}

#[tauri::command]
pub fn hide_tooltip(app: AppHandle) -> Result<(), String> {
    TooltipWindow::hide_tooltip(&app).map_err(map_error)
}

