//! 原生壳的系统侧装配：日志、数据目录与核心状态。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use floatpaste_core::{
    domain::error::AppError, repository::sqlite_repository::SqliteRepository,
    services::image_storage::ImageStorage, state::CoreState,
};
use tracing_appender::non_blocking::{ErrorCounter, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const MAX_LOG_FILES: usize = 15;

pub struct LoggingGuard {
    reporter_stop: Sender<()>,
    reporter_thread: Option<JoinHandle<()>>,
    _worker_guard: WorkerGuard,
}

impl Drop for LoggingGuard {
    fn drop(&mut self) {
        let _ = self.reporter_stop.send(());
        if let Some(reporter_thread) = self.reporter_thread.take() {
            let _ = reporter_thread.join();
        }
    }
}

/// 初始化日志：stdout + 按天滚动文件。返回的 guard 需在进程生命周期内保活。
///
/// 默认级别放到了 debug：插入符定位链路的诊断日志（锚点来源、失败现场）
/// 全在 debug 级，定位问题要开箱可查，不能要求先设 RUST_LOG 再复现
pub fn init_logging() -> Option<LoggingGuard> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter());

    let (mut log_dir, mut fallback_reason) = match resolve_log_dir() {
        Ok(log_dir) => (log_dir, None),
        Err(primary_error) => {
            let fallback = std::env::temp_dir().join("FloatPaste").join("logs");
            match std::fs::create_dir_all(&fallback) {
                Ok(()) => (fallback, Some(primary_error)),
                Err(fallback_error) => {
                    eprintln!(
                        "无法初始化日志目录: {primary_error}; 临时目录也不可用: {fallback_error}"
                    );
                    let _ = fmt()
                        .with_env_filter(env_filter)
                        .with_target(false)
                        .try_init();
                    return None;
                }
            }
        }
    };

    let file_appender = match build_file_appender(&log_dir) {
        Ok(appender) => appender,
        Err(primary_error) if fallback_reason.is_none() => {
            let fallback = std::env::temp_dir().join("FloatPaste").join("logs");
            let fallback_appender = std::fs::create_dir_all(&fallback)
                .map_err(|error| error.to_string())
                .and_then(|()| build_file_appender(&fallback).map_err(|error| error.to_string()));
            match fallback_appender {
                Ok(appender) => {
                    fallback_reason = Some(primary_error.to_string());
                    log_dir = fallback;
                    appender
                }
                Err(fallback_error) => {
                    eprintln!(
                        "无法初始化日志文件: {primary_error}; 临时目录也不可用: {fallback_error}"
                    );
                    let _ = fmt()
                        .with_env_filter(env_filter)
                        .with_target(false)
                        .try_init();
                    return None;
                }
            }
        }
        Err(error) => {
            eprintln!("无法初始化日志文件: {error}");
            let _ = fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .try_init();
            return None;
        }
    };
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);
    let dropped_lines = non_blocking_file.error_counter();

    let result = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false))
        .with(fmt::layer().with_writer(non_blocking_file).with_ansi(false))
        .try_init();

    if let Err(error) = result {
        eprintln!("无法安装日志订阅器: {error}");
        return None;
    }

    if let Some(error) = fallback_reason {
        tracing::warn!("应用日志目录不可用，已改写入临时目录: {error}");
    }
    tracing::info!("日志将写入 {}", log_dir.display());
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        executable = ?std::env::current_exe().ok(),
        diagnostic_text = std::env::var_os("FLOATPASTE_DIAGNOSTIC_LOG").is_some_and(|value| value == "1"),
        "应用启动诊断"
    );
    let (reporter_stop, reporter_thread) = start_drop_reporter(dropped_lines);
    Some(LoggingGuard {
        reporter_stop,
        reporter_thread,
        _worker_guard: guard,
    })
}

fn start_drop_reporter(counter: ErrorCounter) -> (Sender<()>, Option<JoinHandle<()>>) {
    let (stop_tx, stop_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("floatpaste-log-health".to_string())
        .spawn(move || {
            let mut reported = 0;
            loop {
                match stop_rx.recv_timeout(Duration::from_secs(30)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        report_dropped_lines(&counter, &mut reported);
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        report_dropped_lines(&counter, &mut reported);
                    }
                }
            }
        });
    if let Err(error) = &thread {
        tracing::warn!("启动日志丢弃计数监控失败: {error}");
    }
    (stop_tx, thread.ok())
}

fn report_dropped_lines(counter: &ErrorCounter, reported: &mut usize) {
    let total = counter.dropped_lines();
    if total > *reported {
        tracing::warn!(
            newly_dropped_lines = total - *reported,
            total_dropped_lines = total,
            "异步日志队列已丢弃日志"
        );
        *reported = total;
    }
}

fn build_file_appender(
    log_dir: &Path,
) -> Result<RollingFileAppender, tracing_appender::rolling::InitError> {
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("floatpaste-native.log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
}

/// 两个 crate 的 target 前缀不同（floatpaste / floatpaste_core），都显式列出
fn default_filter() -> EnvFilter {
    EnvFilter::new("floatpaste=debug,floatpaste_core=debug")
}

fn resolve_log_dir() -> Result<PathBuf, String> {
    let dir = resolve_data_dir()
        .map_err(|error| error.to_string())?
        .join("logs");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// 初始化核心状态：日志与数据库共用数据目录解析及旧目录迁移。
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
        return resolve_appdata_dir(Path::new(&appdata));
    }

    Ok(std::env::current_dir()?.join(".floatpaste-data"))
}

fn resolve_appdata_dir(appdata: &Path) -> Result<PathBuf, AppError> {
    let data_dir = appdata.join("floatpaste");
    let legacy_dir = appdata.join("com.floatpaste");
    // 整目录重命名保留数据库旁车、图片及日志；两个目录并存时不合并或覆盖。
    // 必须早于日志创建目录，否则空的新日志目录会挡住旧数据迁移。
    if !data_dir.exists() && legacy_dir.exists() {
        if let Err(error) = std::fs::rename(&legacy_dir, &data_dir) {
            // 日志初始化早于单实例检查，另一个启动进程可能已完成迁移。
            if legacy_dir.exists() || !data_dir.is_dir() {
                return Err(AppError::Message(format!(
                    "迁移旧数据目录失败（{} → {}）：{error}。请退出旧版 FloatPaste 后重试。",
                    legacy_dir.display(),
                    data_dir.display()
                )));
            }
        }
    }
    Ok(data_dir)
}

#[cfg(test)]
mod tests {
    use super::resolve_appdata_dir;
    use std::{fs, os::windows::fs::OpenOptionsExt, time::SystemTime};

    #[test]
    fn data_directory_migration_preserves_files_and_existing_destination() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = temp.join(format!(
            "floatpaste-data-dir-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        assert_eq!(resolve_appdata_dir(&root).unwrap(), root.join("floatpaste"));
        assert!(!root.join("floatpaste").exists());

        let old = root.join("com.floatpaste");
        fs::create_dir_all(old.join("images")).unwrap();
        fs::create_dir_all(old.join("logs")).unwrap();
        for name in [
            "floatpaste.db",
            "floatpaste.db-wal",
            "images/test.png",
            "logs/test.log",
        ] {
            fs::write(old.join(name), name).unwrap();
        }
        let current = resolve_appdata_dir(&root).unwrap();
        assert!(!old.exists());
        for name in [
            "floatpaste.db",
            "floatpaste.db-wal",
            "images/test.png",
            "logs/test.log",
        ] {
            assert_eq!(fs::read_to_string(current.join(name)).unwrap(), name);
        }
        assert_eq!(resolve_appdata_dir(&root).unwrap(), current);

        fs::create_dir(&old).unwrap();
        fs::write(old.join("floatpaste.db"), "旧目录独有数据").unwrap();
        assert_eq!(resolve_appdata_dir(&root).unwrap(), current);
        assert_eq!(
            fs::read_to_string(current.join("floatpaste.db")).unwrap(),
            "floatpaste.db"
        );
        assert_eq!(
            fs::read_to_string(old.join("floatpaste.db")).unwrap(),
            "旧目录独有数据"
        );

        let blocked = root.join("blocked");
        fs::create_dir_all(blocked.join("com.floatpaste")).unwrap();
        let locked_path = blocked.join("com.floatpaste/floatpaste.db");
        fs::write(&locked_path, "被旧版占用的数据").unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked_path)
            .unwrap();
        assert!(resolve_appdata_dir(&blocked).is_err());
        assert!(!blocked.join("floatpaste").exists());
        drop(lock);
        let migrated = resolve_appdata_dir(&blocked).unwrap();
        assert_eq!(
            fs::read_to_string(migrated.join("floatpaste.db")).unwrap(),
            "被旧版占用的数据"
        );

        // 只清理本测试直接创建、位于临时目录下的唯一目录。
        assert_eq!(root.canonicalize().unwrap().parent(), Some(temp.as_path()));
        fs::remove_dir_all(&root).unwrap();
    }
}
