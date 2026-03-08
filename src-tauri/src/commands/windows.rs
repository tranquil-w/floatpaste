use std::{thread, time::Duration};

use tauri::{AppHandle, Manager, State};

use crate::{app_bootstrap::AppState, services::window_coordinator::WindowCoordinator};

fn map_error(error: impl ToString) -> String {
    error.to_string()
}

#[tauri::command]
pub fn show_picker(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::show_picker(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn show_picker_from_manager(_state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let app_for_thread = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        let app_for_ui = app_for_thread.clone();
        let _ = app_for_thread.run_on_main_thread(move || {
            if let Some(state) = app_for_ui.try_state::<AppState>() {
                let _ = WindowCoordinator::show_picker(&app_for_ui, &state);
            }
        });
    });

    Ok(())
}

#[tauri::command]
pub fn hide_picker(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::hide_picker_and_restore_target(&app, &state).map_err(map_error)
}

#[tauri::command]
pub fn open_manager(_state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    WindowCoordinator::open_manager(&app).map_err(map_error)
}
