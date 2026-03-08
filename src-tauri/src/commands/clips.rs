use tauri::{AppHandle, Emitter, State};

use crate::{
    app_bootstrap::AppState,
    domain::{
        clip_item::{
            ClipItemDetail, ClipItemSummary, PasteOption, PasteResult, SearchQuery, SearchResult,
        },
        events::CLIPS_CHANGED_EVENT,
    },
    services::{
        normalize_service::NormalizeService, paste_executor::PasteExecutor,
        search_service::SearchService,
    },
};

fn map_error(error: impl ToString) -> String {
    error.to_string()
}

#[tauri::command]
pub fn list_recent_items(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<ClipItemSummary>, String> {
    state.repository.list_recent(limit).map_err(map_error)
}

#[tauri::command]
pub fn list_favorite_items(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<ClipItemSummary>, String> {
    state.repository.list_favorites(limit).map_err(map_error)
}

#[tauri::command]
pub fn get_item_detail(state: State<'_, AppState>, id: String) -> Result<ClipItemDetail, String> {
    state.repository.get_item_detail(&id).map_err(map_error)
}

#[tauri::command]
pub fn search_items(
    state: State<'_, AppState>,
    query: SearchQuery,
) -> Result<SearchResult, String> {
    SearchService::search(&state.repository, query).map_err(map_error)
}

#[tauri::command]
pub fn update_text_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> Result<ClipItemDetail, String> {
    let normalized = NormalizeService::normalize_text(&text, None)
        .ok_or_else(|| "更新内容不能为空".to_string())?;
    let detail = state
        .repository
        .update_text(&id, &normalized)
        .map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, &detail.id);
    Ok(detail)
}

#[tauri::command]
pub fn delete_item(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.repository.delete_item(&id).map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, &id);
    Ok(())
}

#[tauri::command]
pub fn set_item_favorited(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    value: bool,
) -> Result<(), String> {
    state
        .repository
        .set_favorited(&id, value)
        .map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, &id);
    Ok(())
}

#[tauri::command]
pub fn paste_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    option: PasteOption,
) -> Result<PasteResult, String> {
    let result = PasteExecutor::paste_item(&app, &state, &id, option).map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, &id);
    Ok(result)
}
