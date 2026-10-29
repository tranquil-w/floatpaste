//! L1：隐私边界、保留与清除的真实持久化契约；不读写系统剪贴板。
use floatpaste_core::{
    domain::{clip_item::PasteMode, settings::UserSetting},
    repository::sqlite_repository::SqliteRepository,
    services::{
        clip_service::ClipService,
        history_service::HistoryService,
        image_storage::ImageStorage,
        paste_support::{self, InjectionOutcome},
        retention_service::RetentionService,
    },
    state::CoreState,
};
use std::{fs, path::PathBuf};

fn isolated_state() -> (PathBuf, CoreState) {
    let dir = std::env::temp_dir().join(format!("floatpaste-privacy-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&dir).unwrap();
    let state = CoreState::new(
        SqliteRepository::new(&dir.join("history.db")).unwrap(),
        ImageStorage::new(dir.clone()).unwrap(),
        UserSetting::default(),
    );
    (dir, state)
}

#[test]
fn lock_and_pause_invalidate_old_work_and_locked_ingest_has_no_payloads() {
    let (dir, state) = isolated_state();
    assert!(
        HistoryService::ingest_text(&state, "排除来源秘密", Some("keepass.EXE".into()))
            .unwrap()
            .is_none()
    );
    let text = HistoryService::ingest_text(&state, "原条目", None)
        .unwrap()
        .unwrap();
    let generation = state.privacy_generation();
    state.set_session_locked(true).unwrap();
    assert!(HistoryService::ingest_text(&state, "锁屏秘密", None)
        .unwrap()
        .is_none());
    let image = state
        .image_storage
        .prepare_image(&[1, 2, 3, 255], 1, 1, None)
        .unwrap();
    assert!(HistoryService::ingest_image(&state, image, None)
        .unwrap()
        .is_none());
    assert!(
        HistoryService::ingest_files(&state, vec!["C:\\secret.txt".into()], 0, None, None)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_dir(dir.join("images")).unwrap().count(), 0);
    state.set_session_locked(false).unwrap();
    assert!(matches!(
        paste_support::publish_item_if_current(
            &state,
            &text,
            PasteMode::Original,
            None,
            None,
            Some(generation)
        ),
        Err(floatpaste_core::domain::error::AppError::OperationCancelled)
    ));
    assert_eq!(
        paste_support::trigger_ctrl_v_if_current(&state, generation, 0, 0, 0).unwrap(),
        InjectionOutcome::PrivacyChanged
    );
    let mut settings = state.current_settings().unwrap();
    let before_pause = state.privacy_generation();
    settings.pause_monitoring = true;
    state.update_settings(settings).unwrap();
    assert_ne!(before_pause, state.privacy_generation());
    assert!(HistoryService::ingest_text(&state, "暂停秘密", None)
        .unwrap()
        .is_none());
    assert_eq!(state.repository.list_recent(10).unwrap().len(), 1);
    drop(state);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn retention_preserves_favorites_and_clear_removes_all_history_but_not_source_files_or_settings() {
    let (dir, state) = isolated_state();
    let mut settings = state.current_settings().unwrap();
    settings.history_retention_days = 7;
    state.update_settings(settings).unwrap();
    let expired = HistoryService::ingest_text(&state, "过期", None)
        .unwrap()
        .unwrap();
    let favorite = HistoryService::ingest_text(&state, "收藏", None)
        .unwrap()
        .unwrap();
    ClipService::set_favorited(&state, &favorite.id, true).unwrap();
    floatpaste_core::services::tag_service::TagService::set_item_tags(
        &state,
        &favorite.id,
        &["秘密标签".into()],
    )
    .unwrap();
    let deleted = HistoryService::ingest_text(&state, "已删除", None)
        .unwrap()
        .unwrap();
    ClipService::delete(&state, &deleted.id).unwrap();
    let html = floatpaste_core::services::html_format::encode_html_fragment("<b>格式秘密</b>");
    let rich = HistoryService::ingest_text_formats(&state, "格式秘密", Some(html), None)
        .unwrap()
        .unwrap();
    assert!(rich.has_html);
    let image = state
        .image_storage
        .prepare_image(&[1, 2, 3, 255], 1, 1, None)
        .unwrap();
    HistoryService::ingest_image(&state, image, None)
        .unwrap()
        .unwrap();
    let source = dir.join("用户源文件.txt");
    fs::write(&source, "用户数据").unwrap();
    HistoryService::ingest_files(
        &state,
        vec![source.to_string_lossy().into_owned()],
        0,
        None,
        None,
    )
    .unwrap()
    .unwrap();
    let connection = rusqlite::Connection::open(dir.join("history.db")).unwrap();
    let cutoff = chrono::Utc::now().timestamp_millis() - 8 * 86_400_000;
    connection
        .execute(
            "UPDATE clip_items SET created_at = ?1, last_used_at = ?1 WHERE id IN (?2, ?3)",
            rusqlite::params![cutoff, expired.id, favorite.id],
        )
        .unwrap();
    RetentionService::run_once(&state).unwrap();
    assert!(state.repository.get_item_detail(&expired.id).is_err());
    assert!(state.repository.get_item_detail(&favorite.id).is_ok());
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM clip_items WHERE id = ?1",
                [&deleted.id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    let generation = state.privacy_generation();
    let cleared = ClipService::clear_history(&state).unwrap();
    assert_eq!(cleared.pending_images, 0);
    assert!(cleared.storage_compacted);
    assert_ne!(state.privacy_generation(), generation);
    for table in [
        "clip_items",
        "clip_items_fts",
        "clip_text_formats",
        "clip_item_tags",
        "tags",
        "pending_image_deletions",
    ] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0,
            "{table}"
        );
    }
    assert_eq!(
        state
            .repository
            .load_settings()
            .unwrap()
            .history_retention_days,
        7
    );
    assert_eq!(fs::read_to_string(source).unwrap(), "用户数据");
    assert_eq!(fs::read_dir(dir.join("images")).unwrap().count(), 0);
    drop(connection);
    drop(state);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_image_deletion_survives_reopen_and_retries_without_deleting_non_owned_files() {
    use std::os::windows::fs::OpenOptionsExt;
    let (dir, state) = isolated_state();
    let image = state
        .image_storage
        .prepare_image(&[1, 2, 3, 255], 1, 1, None)
        .unwrap();
    let detail = HistoryService::ingest_image(&state, image, None)
        .unwrap()
        .unwrap();
    let image_path = detail.image_path.unwrap();
    let absolute = state
        .image_storage
        .resolve_existing_image_path(&image_path)
        .unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&absolute)
        .unwrap();
    assert_eq!(
        ClipService::clear_history(&state).unwrap().pending_images,
        1
    );
    assert_eq!(
        state.repository.pending_image_deletions().unwrap(),
        vec![image_path.clone()]
    );
    drop(state);
    drop(held);
    let state = CoreState::new(
        SqliteRepository::new(&dir.join("history.db")).unwrap(),
        ImageStorage::new(dir.clone()).unwrap(),
        UserSetting::default(),
    );
    RetentionService::run_once(&state).unwrap();
    assert!(!absolute.exists());
    assert!(state
        .repository
        .pending_image_deletions()
        .unwrap()
        .is_empty());
    let source = dir.join("source.txt");
    fs::write(&source, "保留").unwrap();
    assert!(state.image_storage.delete_image("source.txt").is_err());
    assert!(state.image_storage.delete_image("../source.txt").is_err());
    assert!(source.exists());
    fs::remove_dir(dir.join("images")).unwrap();
    state.repository.queue_image_deletion(&image_path).unwrap();
    assert_eq!(RetentionService::retry_image_deletions(&state).unwrap(), 0);
    assert!(
        state
            .repository
            .pending_image_deletions()
            .unwrap()
            .is_empty(),
        "图片目录已不存在时应完成清理任务"
    );
    drop(state);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_settings_keep_previous_retention_and_public_ingest_rejects_oversized_text() {
    let settings: UserSetting = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.history_retention_days, 0);
    assert_eq!(settings.deleted_retention_days, 30);
    let settings = UserSetting {
        excluded_apps: vec![" KeePass.exe ".into(), "keepass.EXE".into()],
        ..settings
    }
    .sanitized();
    assert_eq!(settings.excluded_apps.len(), 1);
    let (dir, state) = isolated_state();
    assert!(HistoryService::ingest_text(
        &state,
        &"x".repeat(floatpaste_core::services::input_limits::MAX_TEXT_BYTES + 1),
        None
    )
    .is_err());
    assert!(state.repository.list_recent(10).unwrap().is_empty());
    drop(state);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn v8_upgrade_adds_cleanup_queue_without_changing_existing_content() {
    let (dir, state) = isolated_state();
    let text = HistoryService::ingest_text(&state, "升级保留", None)
        .unwrap()
        .unwrap();
    drop(state);
    let connection = rusqlite::Connection::open(dir.join("history.db")).unwrap();
    connection
        .execute_batch("DROP TABLE pending_image_deletions; PRAGMA user_version = 8;")
        .unwrap();
    drop(connection);
    let repository = SqliteRepository::new(&dir.join("history.db")).unwrap();
    assert_eq!(
        repository.get_item_detail(&text.id).unwrap().full_text,
        "升级保留"
    );
    assert!(repository.pending_image_deletions().unwrap().is_empty());
    repository
        .queue_image_deletion("images/00000000-0000-0000-0000-000000000000.png")
        .unwrap();
    assert_eq!(repository.pending_image_deletions().unwrap().len(), 1);
    drop(repository);
    fs::remove_dir_all(dir).unwrap();
}
