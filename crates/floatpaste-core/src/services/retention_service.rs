use std::{thread, time::Duration};

use tracing::{info, warn};

use crate::{domain::error::AppError, state::CoreState};

/// 清理周期：与"每天启动一次"的桌面使用节奏对齐。
const RETENTION_INTERVAL_SECS: u64 = 24 * 60 * 60;
/// 启动后延迟执行首轮清理，避开启动高峰（窗口创建、设置副作用、首批剪贴采集）。
const FIRST_RUN_DELAY: Duration = Duration::from_secs(60);
/// 单轮物理删除超过该行数时顺带 VACUUM 收缩数据库文件。
const VACUUM_THRESHOLD: usize = 500;

/// E2E 隔离钩子：FLOATPASTE_RETENTION_FIRST_DELAY_MS 覆盖首轮清理延迟，
/// 让端到端测试在秒级时间尺度内验证清理接线；缺省或非法值走默认 60s。
fn first_run_delay() -> Duration {
    std::env::var("FLOATPASTE_RETENTION_FIRST_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(FIRST_RUN_DELAY)
}

/// 剪贴历史保留策略：把 `history_limit` 设置从"仅展示"兑现为真实的存储上限。
///
/// 职责：定期回收超过保留期的软删记录、把未收藏历史压缩到用户设置的上限内，
/// 并尽力清理对应图片文件。所有删除均为尽力而为，失败只记录日志不影响运行。
pub struct RetentionService;

impl RetentionService {
    /// 启动后台清理线程：启动延迟一轮，此后按固定周期执行，退出标志生效时结束。
    pub fn start(state: CoreState) {
        thread::spawn(move || {
            thread::sleep(first_run_delay());

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

    pub fn run_once(state: &CoreState) -> Result<(), AppError> {
        let settings = state.current_settings()?;
        let purged = state
            .repository
            .purge_soft_deleted(i64::from(settings.deleted_retention_days) * 86_400_000)?;
        let expired = state
            .repository
            .purge_expired_history(settings.history_retention_days)?;
        let overflow = state
            .repository
            .enforce_history_limit(settings.history_limit)?;

        let removed_count = purged.len() + overflow.len() + expired;
        Self::retry_image_deletions(state)?;

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

    /// 队列与行删除同事务提交；进程重启或文件被占用都不会遗失清理任务。
    pub fn retry_image_deletions(state: &CoreState) -> Result<usize, AppError> {
        let mut pending = 0;
        for path in state.repository.pending_image_deletions()? {
            match state.image_storage.delete_image(&path) {
                Ok(()) => state.repository.finish_image_deletion(&path)?,
                Err(error) => {
                    pending += 1;
                    warn!(error = %error, "应用图片清理失败，保留任务等待重试");
                }
            }
        }
        Ok(pending)
    }
}
