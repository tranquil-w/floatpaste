mod app_bootstrap;
mod commands;
mod domain;
mod launch_mode;
mod platform;
mod repository;
mod services;

use tauri_plugin_global_shortcut::Builder as GlobalShortcutBuilder;
use tracing_subscriber::{fmt, EnvFilter};

use crate::{launch_mode::LaunchMode, services::shortcut_manager::ShortcutManager};

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
    let launch_mode = LaunchMode::from_env();
    let _single_instance =
        match crate::platform::windows::single_instance::acquire_or_focus_existing(launch_mode) {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(
                    "单实例检查或唤醒已有实例失败，将继续启动当前实例作为恢复路径: {error}"
                );
                None
            }
        };

    tauri::Builder::default()
        .plugin(
            GlobalShortcutBuilder::new()
                .with_handler(|app, shortcut, event| {
                    ShortcutManager::handle_shortcut_event(app, shortcut.into_string(), &event);
                })
                .build(),
        )
        .setup(move |app| app_bootstrap::bootstrap(app, launch_mode).map_err(Into::into))
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
            commands::windows::open_editor_from_picker,
            commands::windows::open_editor_from_workbench,
            commands::windows::hide_editor,
            commands::windows::open_workbench_global,
            commands::windows::hide_workbench
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用失败");
}
