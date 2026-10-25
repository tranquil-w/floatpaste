//! L1：公开录入和删除服务的图片保留契约，不访问系统剪贴板。
use std::fs;

use floatpaste_core::{
    domain::{
        clip_item::{SearchFilters, SearchQuery, SearchSort},
        settings::UserSetting,
    },
    repository::sqlite_repository::SqliteRepository,
    services::{
        clip_service::ClipService, history_service::HistoryService, image_storage::ImageStorage,
        retention_service::RetentionService,
    },
    state::CoreState,
};

#[test]
fn soft_deleted_images_keep_payload_until_retention_purges_them() {
    let dir = std::env::temp_dir().join(format!("floatpaste-images-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("history.db");
    let state = CoreState::new(
        SqliteRepository::new(&db_path).unwrap(),
        ImageStorage::new(dir.clone()).unwrap(),
        UserSetting::default(),
    );
    let prepared = state
        .image_storage
        .prepare_image(&[255, 0, 0, 255], 1, 1, None)
        .unwrap();
    let image = HistoryService::ingest_image(&state, prepared, None)
        .unwrap()
        .unwrap();
    let image_path = image.image_path.as_deref().unwrap();
    ClipService::set_favorited(&state, &image.id, true).unwrap();
    ClipService::delete(&state, &image.id).unwrap();

    assert!(state.repository.list_recent(10).unwrap().is_empty());
    assert_eq!(
        state
            .repository
            .search(SearchQuery {
                keyword: String::new(),
                filters: SearchFilters::default(),
                offset: 0,
                limit: 10,
                sort: SearchSort::RecentDesc,
            })
            .unwrap()
            .total,
        0
    );
    assert!(
        state.image_storage.load_image(image_path).is_ok(),
        "软删保留图片载荷"
    );
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM clip_items WHERE deleted_at IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1,
        "软删行仍在库中"
    );
    RetentionService::run_once(&state).unwrap();
    assert!(
        state.image_storage.load_image(image_path).is_ok(),
        "保留期内不回收图片"
    );
    assert!(
        ClipService::delete(&state, "missing-item").is_err(),
        "缺失条目仍返回错误"
    );

    // 已缺失的载荷不能阻止其他条目清理；用户文件只保存引用，始终保留源文件。
    let prepared = state
        .image_storage
        .prepare_image(&[0, 255, 0, 255], 1, 1, None)
        .unwrap();
    let missing = HistoryService::ingest_image(&state, prepared, None)
        .unwrap()
        .unwrap();
    ClipService::delete(&state, &missing.id).unwrap();
    fs::remove_file(
        state
            .image_storage
            .resolve_image_path(missing.image_path.as_deref().unwrap())
            .unwrap(),
    )
    .unwrap();
    let source_file = dir.join("user-file.txt");
    fs::write(&source_file, "用户源文件").unwrap();
    let file = HistoryService::ingest_files(
        &state,
        vec![source_file.to_string_lossy().into_owned()],
        0,
        None,
        None,
    )
    .unwrap()
    .unwrap();
    ClipService::delete(&state, &file.id).unwrap();
    connection
        .execute(
            "UPDATE clip_items SET deleted_at = ?1 WHERE deleted_at IS NOT NULL",
            [chrono::Utc::now().timestamp_millis() - 31 * 24 * 60 * 60 * 1000],
        )
        .unwrap();
    RetentionService::run_once(&state).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM clip_items", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0,
        "到期收藏软删行也应回收"
    );
    assert!(!dir.join(image_path).exists(), "到期后才回收图片文件");
    assert!(source_file.is_file(), "历史清理不删除用户源文件");
    RetentionService::run_once(&state).unwrap();

    drop(connection);
    drop(state);
    fs::remove_dir_all(dir).unwrap();
}
