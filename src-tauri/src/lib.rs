mod app_bootstrap;
mod commands;
mod domain;
mod platform;
mod repository;
mod services;

use tauri_plugin_global_shortcut::Builder as GlobalShortcutBuilder;
use tracing_subscriber::{fmt, EnvFilter};

use crate::services::shortcut_manager::ShortcutManager;

fn init_logging() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("floatpaste=info"));

    let _ = fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .try_init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

    tauri::Builder::default()
        .plugin(
            GlobalShortcutBuilder::new()
                .with_handler(|app, shortcut, event| {
                    ShortcutManager::handle_shortcut_event(app, shortcut.into_string(), &event);
                })
                .build(),
        )
        .setup(|app| app_bootstrap::bootstrap(app).map_err(Into::into))
        .invoke_handler(tauri::generate_handler![
            commands::clips::list_recent_items,
            commands::clips::list_favorite_items,
            commands::clips::get_item_detail,
            commands::clips::search_items,
            commands::clips::update_text_item,
            commands::clips::delete_item,
            commands::clips::set_item_favorited,
            commands::clips::paste_item,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::pause_monitoring,
            commands::settings::resume_monitoring,
            commands::windows::show_picker,
            commands::windows::show_picker_from_manager,
            commands::windows::hide_picker,
            commands::windows::open_manager,
            commands::windows::open_manager_from_picker
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用失败");
}
