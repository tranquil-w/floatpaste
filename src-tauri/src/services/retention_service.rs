use std::{thread, time::Duration};

use tracing::{info, warn};

use crate::{app_bootstrap::AppState, domain::error::AppError};

/// 软删除记录的物理清除保留期：给误删恢复与问题排查留出缓冲窗口。
const SOFT_DELETE_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// 清理周期：与"每天启动一次"的桌面使用节奏对齐。
const RETENTION_INTERVAL_SECS: u64 = 24 * 60 * 60;
/// 启动后延迟执行首轮清理，避开启动高峰（窗口创建、设置副作用、首批剪贴采集）。
const FIRST_RUN_DELAY: Duration = Duration::from_secs(60);
/// 单轮物理删除超过该行数时顺带 VACUUM 收缩数据库文件。
const VACUUM_THRESHOLD: usize = 500;

/// 剪贴历史保留策略：把 `history_limit` 设置从"仅展示"兑现为真实的存储上限。
///
/// 职责：定期回收超过保留期的软删记录、把未收藏历史压缩到用户设置的上限内，
/// 并尽力清理对应图片文件。所有删除均为尽力而为，失败只记录日志不影响运行。
pub struct RetentionService;

impl RetentionService {
    /// 启动后台清理线程：启动延迟一轮，此后按固定周期执行，退出标志生效时结束。
    pub fn start(state: AppState) {
        thread::spawn(move || {
            thread::sleep(FIRST_RUN_DELAY);

            loop {
                if state.is_quitting() {
                    break;
                }

                if let Err(error) = Self::run_once(&state) {
                    warn!("执行剪贴历史清理失败，将于下轮重试: {error}");
                }

                for _ in 0..RETENTION_INTERVAL_SECS {
                    if state.is_quitting() {
                        return;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            }
        });
    }

    pub fn run_once(state: &AppState) -> Result<(), AppError> {
        let settings = state.current_settings()?;
        let purged = state
            .repository
            .purge_soft_deleted(SOFT_DELETE_RETENTION_MS)?;
        let overflow = state
            .repository
            .enforce_history_limit(settings.history_limit)?;

        let removed_count = purged.len() + overflow.len();
        for path in purged.iter().chain(overflow.iter()) {
            if path.is_empty() {
                continue;
            }
            if let Err(error) = state.image_storage.delete_image(path) {
                warn!("清理图片文件失败: {path}, error={error}");
            }
        }

        if removed_count >= VACUUM_THRESHOLD {
            if let Err(error) = state.repository.vacuum() {
                warn!("VACUUM 数据库失败: {error}");
            }
        }

        if removed_count > 0 {
            info!(
                "剪贴历史清理完成：回收软删 {} 条、溢出上限 {} 条",
                purged.len(),
                overflow.len()
            );
        }
        Ok(())
    }
}
