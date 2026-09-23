//! 原生壳的系统侧装配：日志、数据目录与核心状态。

use std::path::PathBuf;

use floatpaste_core::{
    domain::error::AppError, repository::sqlite_repository::SqliteRepository,
    services::image_storage::ImageStorage, state::CoreState,
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// 初始化日志：stdout + 按天滚动文件。返回的 guard 需在进程生命周期内保活。
///
/// 默认级别放到了 debug：插入符定位链路的诊断日志（锚点来源、失败现场）
/// 全在 debug 级，定位问题要开箱可查，不能要求先设 RUST_LOG 再复现
pub fn init_logging() -> Option<WorkerGuard> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter());

    let Some(log_dir) = resolve_log_dir() else {
        let _ = fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .try_init();
        return None;
    };

    let file_appender = tracing_appender::rolling::daily(&log_dir, "floatpaste-native.log");
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    let result = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false))
        .with(fmt::layer().with_writer(non_blocking_file).with_ansi(false))
        .try_init();

    if result.is_err() {
        let _ = fmt()
            .with_env_filter(default_filter())
            .with_target(false)
            .try_init();
        return None;
    }

    tracing::info!("日志将写入 {}", log_dir.display());
    Some(guard)
}

/// 两个 crate 的 target 前缀不同（floatpaste / floatpaste_core），都显式列出
fn default_filter() -> EnvFilter {
    EnvFilter::new("floatpaste=debug,floatpaste_core=debug")
}

fn resolve_log_dir() -> Option<PathBuf> {
    resolve_data_dir()
        .ok()
        .map(|dir| dir.join("logs"))
        .and_then(|dir| std::fs::create_dir_all(&dir).is_ok().then_some(dir))
}

/// 初始化核心状态：与老 Tauri 壳共用同一数据目录（数据库/图片/设置无缝继承）。
pub fn init_core() -> Result<CoreState, AppError> {
    let data_dir = resolve_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let repository = SqliteRepository::new(&data_dir.join("floatpaste.db"))?;
    let image_storage = ImageStorage::new(data_dir)?;
    let settings = repository.load_settings()?;
    Ok(CoreState::new(repository, image_storage, settings))
}

fn resolve_data_dir() -> Result<PathBuf, AppError> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Ok(PathBuf::from(appdata).join("com.floatpaste"));
    }

    Ok(std::env::current_dir()?.join(".floatpaste-data"))
}
