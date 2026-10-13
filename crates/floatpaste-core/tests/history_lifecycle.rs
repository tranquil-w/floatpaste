//! L1 进程内生命周期集成：临时库组装 CoreState，沿公共 API 串行验证
//! 「录入 → 去重置顶 → 标签 → 收藏 → 搜索 → 软删 → 保留清理」完整业务流。
//! 单测试函数串行推进（各阶段共享同一份状态），断言按阶段分组。

use std::{fs, path::PathBuf};

use floatpaste_core::{
    domain::{
        clip_item::{SearchFilters, SearchQuery, SearchSort},
        settings::UserSetting,
    },
    repository::sqlite_repository::SqliteRepository,
    services::{
        history_service::HistoryService, image_storage::ImageStorage,
        retention_service::RetentionService, tag_service::TagService,
    },
    state::CoreState,
};
use uuid::Uuid;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("floatpaste-l1-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn search_all(repository: &SqliteRepository, keyword: &str) -> u32 {
    let result = repository
        .search(SearchQuery {
            keyword: keyword.to_string(),
            filters: SearchFilters::default(),
            offset: 0,
            limit: 100,
            sort: SearchSort::RelevanceDesc,
        })
        .unwrap();
    result.total
}

#[test]
fn clip_history_lifecycle() {
    let dir = temp_dir();
    let repository = SqliteRepository::new(&dir.join("floatpaste.db")).unwrap();
    let image_storage = ImageStorage::new(dir.clone()).unwrap();
    let mut settings = UserSetting::default();
    // 压缩上限取小值让保留清理阶段只需少量条目即可触发（该值由
    // run_once 直接消费，不经 sanitized 的 100..=10000 clamp）
    settings.history_limit = 2;
    let state = CoreState::new(repository.clone(), image_storage, settings);

    // ── 录入与去重置顶 ──
    let first = HistoryService::ingest_text(&state, "lifecycle 首条内容", Some("app-a.exe".to_string()))
        .unwrap()
        .expect("全新文本应入库");
    HistoryService::ingest_text(&state, "第二条内容", None).unwrap();
    let before = repository.list_recent(10).unwrap().len();
    assert_eq!(before, 2);

    let bumped = HistoryService::ingest_text(&state, "lifecycle 首条内容", None)
        .unwrap()
        .expect("重复内容应走 bump 而非新条目");
    assert_eq!(bumped.id, first.id, "重复文本应命中已有条目");
    assert_eq!(
        repository.list_recent(10).unwrap().len(),
        2,
        "重复录入不得新增条目"
    );
    assert_eq!(
        repository.list_recent(10).unwrap()[0].id,
        first.id,
        "bump 后条目应排到列表顶部"
    );

    // ── 标签 ──
    TagService::set_item_tags(
        &state,
        &first.id,
        &["工作".to_string(), "临时".to_string()],
    )
    .unwrap();
    let tags = repository.list_tags().unwrap();
    assert_eq!(tags.len(), 2, "两个标签都应建行");
    assert!(tags.iter().all(|tag| tag.item_count == 1));

    // ── 收藏 ──
    repository.set_favorited(&first.id, true).unwrap();
    let favorited = repository
        .search(SearchQuery {
            keyword: String::new(),
            filters: SearchFilters {
                favorited_only: Some(true),
                ..SearchFilters::default()
            },
            offset: 0,
            limit: 100,
            sort: SearchSort::RecentDesc,
        })
        .unwrap();
    assert_eq!(favorited.total, 1);
    assert_eq!(favorited.items[0].id, first.id);

    // ── 搜索 ──
    assert_eq!(search_all(&repository, "首条"), 1, "关键词应命中首条");
    assert_eq!(search_all(&repository, "不存在的词"), 0);

    // ── 软删：列表与搜索立即不可见 ──
    repository.delete_item(&first.id).unwrap();
    assert!(!repository
        .list_recent(10)
        .unwrap()
        .iter()
        .any(|item| item.id == first.id));
    assert_eq!(search_all(&repository, "首条"), 0, "软删后不得再被搜到");

    // ── 保留清理：软删回收 + 上限压缩 + 收藏豁免 ──
    // 软删行仍在库内（deleted_at 非空），直接回拨到 31 天前越过保留期
    {
        let connection = rusqlite::Connection::open(dir.join("floatpaste.db")).unwrap();
        connection
            .execute(
                "UPDATE clip_items SET deleted_at = ?1 WHERE id = ?2",
                rusqlite::params![utc_ms_ago(31 * 24 * 60 * 60), first.id],
            )
            .unwrap();
    }
    HistoryService::ingest_text(&state, "第三条内容", None).unwrap();
    HistoryService::ingest_text(&state, "第四条内容", None).unwrap();
    RetentionService::run_once(&state).unwrap();

    let surviving = repository.list_recent(10).unwrap();
    assert_eq!(
        surviving.len(),
        2,
        "未收藏未删条目应被压缩到 history_limit"
    );
    assert!(
        surviving.iter().all(|item| item.id != first.id),
        "超期软删行应被物理清除，收藏也救不回"
    );
    {
        let connection = rusqlite::Connection::open(dir.join("floatpaste.db")).unwrap();
        let soft_deleted: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(soft_deleted, 0, "物理清除后库内不应残留软删行");
    }
    assert!(
        repository.list_tags().unwrap().is_empty(),
        "失去全部关联的标签应随清理回收"
    );

    let _ = fs::remove_dir_all(&dir);
}

fn utc_ms_ago(seconds: i64) -> i64 {
    // 与仓储侧时间戳同源（UTC 毫秒），负偏移即「seconds 秒之前」
    chrono::Utc::now().timestamp_millis() - seconds * 1000
}
