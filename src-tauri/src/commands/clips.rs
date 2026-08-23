use tauri::{AppHandle, Emitter, State};

use crate::{
    app_bootstrap::AppState,
    domain::{
        clip_item::{
            ClipItemDetail, ClipItemSummary, PasteOption, PasteResult, SearchQuery, SearchResult,
            TagInfo,
        },
        events::{ClipsChangedPayload, CLIPS_CHANGED_EVENT, TAGS_CHANGED_EVENT},
    },
    services::{
        clip_service::ClipService, paste_executor::PasteExecutor, search_service::SearchService,
        tag_service::TagService,
    },
};

use super::map_error;

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
pub fn resolve_image_path(state: State<'_, AppState>, image_path: String) -> Result<String, String> {
    state
        .image_storage
        .resolve_existing_image_path(&image_path)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(map_error)
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
    let detail = ClipService::update_text(&state, &id, &text).map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail));
    Ok(detail)
}

#[tauri::command]
pub fn delete_item(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    ClipService::delete(&state, &id).map_err(map_error)?;
    let _ = app.emit(
        CLIPS_CHANGED_EVENT,
        ClipsChangedPayload::Deleted { id: id.clone() },
    );
    Ok(())
}

#[tauri::command]
pub fn set_item_favorited(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    value: bool,
) -> Result<(), String> {
    ClipService::set_favorited(&state, &id, value).map_err(map_error)?;
    if let Ok(summary) = state.repository.get_item_summary(&id) {
        let _ = app.emit(
            CLIPS_CHANGED_EVENT,
            ClipsChangedPayload::Upserted { item: summary },
        );
    }
    Ok(())
}

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<Vec<TagInfo>, String> {
    state.repository.list_tags().map_err(map_error)
}

#[tauri::command]
pub fn set_item_tags(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    tag_names: Vec<String>,
) -> Result<ClipItemDetail, String> {
    let detail = TagService::set_item_tags(&state, &id, &tag_names).map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::upserted(&detail));
    let _ = app.emit(TAGS_CHANGED_EVENT, ());
    Ok(detail)
}

#[tauri::command]
pub fn rename_tag(
    app: AppHandle,
    state: State<'_, AppState>,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    TagService::rename_tag(&state, &old_name, &new_name).map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::BulkChanged);
    let _ = app.emit(TAGS_CHANGED_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn delete_tag(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    TagService::delete_tag(&state, &name).map_err(map_error)?;
    let _ = app.emit(CLIPS_CHANGED_EVENT, ClipsChangedPayload::BulkChanged);
    let _ = app.emit(TAGS_CHANGED_EVENT, ());
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
    if let Ok(summary) = state.repository.get_item_summary(&id) {
        let _ = app.emit(
            CLIPS_CHANGED_EVENT,
            ClipsChangedPayload::Upserted { item: summary },
        );
    }
    Ok(result)
}
