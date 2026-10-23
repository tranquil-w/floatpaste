//! L1：格式身份、迁移、编辑和载荷生命周期；不访问系统剪贴板。
use std::fs;

use floatpaste_core::{
    domain::{
        clip_item::{resolve_secondary_mode, PasteMode},
        settings::{SecondaryPasteDefaults, UserSetting},
    },
    repository::sqlite_repository::SqliteRepository,
    services::{
        clip_service::ClipService, history_service::HistoryService,
        html_format::encode_html_fragment, image_storage::ImageStorage, tag_service::TagService,
        text_identity::text_content_hash,
    },
    state::CoreState,
};

#[test]
fn rich_text_identity_edit_migration_and_payload_lifecycle() {
    let dir = std::env::temp_dir().join(format!("floatpaste-html-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("history.db");
    let state = CoreState::new(
        SqliteRepository::new(&path).unwrap(),
        ImageStorage::new(dir.clone()).unwrap(),
        UserSetting::default(),
    );
    let text = "中文😀 rich text";
    let plain = HistoryService::ingest_text(&state, text, None)
        .unwrap()
        .unwrap();
    TagService::set_item_tags(&state, &plain.id, &["迁移标签".into()]).unwrap();
    state.repository.set_favorited(&plain.id, true).unwrap();
    drop(state);
    // 模拟 v7 的原文身份。迁移只更新身份，不改主键、原文和用户元数据。
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch("DROP TABLE clip_text_formats; PRAGMA user_version = 7;")
        .unwrap();
    connection
        .execute("UPDATE clip_items SET hash = 'v1-old-identity'", [])
        .unwrap();
    drop(connection);
    let state = CoreState::new(
        SqliteRepository::new(&path).unwrap(),
        ImageStorage::new(dir.clone()).unwrap(),
        UserSetting::default(),
    );
    let migrated = state.repository.get_item_detail(&plain.id).unwrap();
    assert_eq!(migrated.full_text, text);
    assert!(migrated.is_favorited);
    assert_eq!(migrated.tags, ["迁移标签"]);
    assert_eq!(migrated.hash, text_content_hash(text));
    assert_eq!(
        HistoryService::ingest_text(&state, text, None)
            .unwrap()
            .unwrap()
            .id,
        plain.id
    );

    let html = encode_html_fragment("<p>中文😀 <b>rich text</b></p>");
    let rich = HistoryService::ingest_text_formats(&state, text, Some(html.clone()), None)
        .unwrap()
        .unwrap();
    assert_ne!(rich.id, plain.id);
    assert_ne!(rich.hash, migrated.hash);
    assert!(rich.html_bytes.is_none(), "普通详情不读取载荷");
    assert_eq!(
        state
            .repository
            .get_item_for_paste(&rich.id)
            .unwrap()
            .html_bytes
            .as_ref(),
        Some(&html)
    );
    assert!(
        state
            .repository
            .get_item_summary(&rich.id)
            .unwrap()
            .has_html
    );
    // 次级动作解析沿用可配置前的既有行为：富文本纯文本、普通文本原文
    let defaults = SecondaryPasteDefaults::default();
    assert_eq!(
        resolve_secondary_mode(&rich.to_summary(), &defaults),
        PasteMode::PlainText
    );
    assert_eq!(
        resolve_secondary_mode(&plain.to_summary(), &defaults),
        PasteMode::Original
    );
    assert_eq!(
        HistoryService::ingest_text_formats(&state, text, Some(html.clone()), None)
            .unwrap()
            .unwrap()
            .id,
        rich.id
    );
    let other = HistoryService::ingest_text_formats(
        &state,
        text,
        Some(encode_html_fragment("<p>中文😀 <i>rich text</i></p>")),
        None,
    )
    .unwrap()
    .unwrap();
    assert_ne!(other.id, rich.id);
    // 仅标签、相同文本保存保留格式；正文改变原子更新为纯文本身份。
    TagService::set_item_tags(&state, &rich.id, &["格式标签".into()]).unwrap();
    assert!(
        ClipService::update_text(&state, &rich.id, text)
            .unwrap()
            .has_html
    );
    assert_eq!(
        state
            .repository
            .get_item_for_paste(&rich.id)
            .unwrap()
            .html_bytes,
        Some(html)
    );
    let edited = ClipService::update_text(&state, &rich.id, "更新正文").unwrap();
    assert!(edited.html_bytes.is_none());
    assert_eq!(edited.hash, text_content_hash("更新正文"));
    assert_eq!(edited.tags, ["格式标签"]);
    assert!(
        !state
            .repository
            .get_item_summary(&rich.id)
            .unwrap()
            .has_html
    );

    let invalid = HistoryService::ingest_text_formats(
        &state,
        "损坏的格式回退",
        Some(b"bad html".to_vec()),
        None,
    )
    .unwrap()
    .unwrap();
    assert!(invalid.html_bytes.is_none());
    ClipService::delete(&state, &other.id).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let payload_count = || {
        connection
            .query_row(
                "SELECT count(*) FROM clip_text_formats WHERE item_id = ?1",
                [&other.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert_eq!(payload_count(), 1, "软删保留格式");
    state.repository.purge_soft_deleted(-1).unwrap();
    assert_eq!(payload_count(), 0, "物理删除级联清理格式");
    drop(connection);
    drop(state);
    fs::remove_dir_all(dir).unwrap();
}
