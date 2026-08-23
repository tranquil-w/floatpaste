mod app_bootstrap;
mod commands;
mod domain;
mod launch_mode;
mod platform;
mod repository;
mod services;

use std::path::PathBuf;

use tauri_plugin_global_shortcut::Builder as GlobalShortcutBuilder;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{launch_mode::LaunchMode, services::shortcut_manager::ShortcutManager};

/// 初始化日志：同时输出到 stdout 与按天滚动的日志文件（位于应用数据目录的 logs 子目录）。
///
/// 返回的 `WorkerGuard` 必须在应用整个生命周期内保活，否则非阻塞写入可能丢失尾部日志。
/// 文件落盘使便携版用户能够提供日志片段辅助排查问题。
fn init_logging() -> Option<WorkerGuard> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("floatpaste=info"));

    let Some(log_dir) = resolve_log_dir() else {
        // 无法定位日志目录时退化为仅 stdout，不阻塞启动。
        let _ = fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .try_init();
        return None;
    };

    let file_appender = tracing_appender::rolling::daily(&log_dir, "floatpaste.log");
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    let result = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false))
        .with(
            fmt::layer()
                .with_writer(non_blocking_file)
                .with_ansi(false),
        )
        .try_init();

    if result.is_err() {
        // 已有 subscriber（如测试环境）时退化为仅 stdout。
        let _ = fmt()
            .with_env_filter(EnvFilter::new("floatpaste=info"))
            .with_target(false)
            .try_init();
        return None;
    }

    tracing::info!("日志将写入 {}", log_dir.display());
    Some(guard)
}

/// 解析日志目录：优先使用应用数据目录下的 logs 子目录，失败时回退到当前目录。
fn resolve_log_dir() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = PathBuf::from(appdata)
            .join("com.floatpaste")
            .join("logs");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir);
        }
    }

    let fallback = std::env::current_dir().ok()?.join(".floatpaste-data").join("logs");
    std::fs::create_dir_all(&fallback).ok()?;
    Some(fallback)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _log_guard = init_logging();
    let launch_mode = LaunchMode::from_env();
    let _single_instance =
        match crate::platform::windows::single_instance::acquire_or_focus_existing(launch_mode) {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => return,
            Err(error) => {
                // 已有实例存在但唤醒失败（或互斥量创建失败）时必须退出当前进程：
                // 继续启动会形成双实例并发写同一 SQLite 库，数据风险远大于本次启动失败
                tracing::error!("单实例检查失败，退出当前实例: {error}");
                return;
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
            commands::clips::resolve_image_path,
            commands::clips::search_items,
            commands::clips::update_text_item,
            commands::clips::delete_item,
            commands::clips::set_item_favorited,
            commands::clips::paste_item,
            commands::clips::list_tags,
            commands::clips::set_item_tags,
            commands::clips::rename_tag,
            commands::clips::delete_tag,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::pause_monitoring,
            commands::settings::resume_monitoring,
            commands::windows::show_picker,
            commands::windows::hide_picker,
            commands::windows::open_settings,
            commands::windows::open_editor_from_picker,
            commands::windows::open_editor_from_search,
            commands::windows::hide_editor,
            commands::windows::open_search_global,
            commands::windows::hide_search,
            commands::windows::prepare_search_window_drag,
            commands::windows::show_tooltip,
            commands::windows::tooltip_ready,
            commands::windows::hide_tooltip
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app_handle, event| {
            // 退出请求阶段：在窗口实际销毁前同步清理低级资源（鼠标钩子、可见窗口等），
            // 避免进程退出时仍有 Chrome_WidgetWin_0 类窗口存活而触发
            // Chromium 的 `Failed to unregister class ... Error = 1412` 报错。
            if let tauri::RunEvent::ExitRequested { .. } = event {
                crate::services::window_coordinator::WindowCoordinator::prepare_for_exit(app_handle);
            }
        });
}
