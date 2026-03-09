use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use rusqlite::{
    params,
    types::{Value, ValueRef},
    Connection, OptionalExtension, Row,
};
use tracing::warn;
use uuid::Uuid;

use crate::domain::{
    clip_item::{
        ClipItemDetail, ClipItemSummary, NewClipTextItem, SearchQuery, SearchResult, SearchSort,
    },
    error::AppError,
    settings::UserSetting,
};

#[derive(Clone)]
pub struct SqliteRepository {
    connection: Arc<Mutex<Connection>>,
}

impl SqliteRepository {
    pub fn new(path: &Path) -> Result<Self, AppError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(include_str!("../../migrations/0001_init.sql"))?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn load_settings(&self) -> Result<UserSetting, AppError> {
        let connection = self.connection.lock()?;
        let settings_json = connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'user_settings'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        let mut setting = settings_json
            .map(|raw| serde_json::from_str::<UserSetting>(&raw))
            .transpose()?
            .unwrap_or_default();

        let mut statement = connection
            .prepare("SELECT executable_name FROM excluded_apps ORDER BY executable_name ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let excluded_apps = rows.collect::<Result<Vec<_>, _>>()?;
        if !excluded_apps.is_empty() {
            setting.excluded_apps = excluded_apps;
        }

        Ok(setting.sanitized())
    }

    pub fn save_settings(&self, setting: &UserSetting) -> Result<(), AppError> {
        let connection = self.connection.lock()?;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO settings(key, value) VALUES('user_settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(setting)?],
        )?;
        transaction.execute("DELETE FROM excluded_apps", [])?;
        for app in &setting.excluded_apps {
            transaction.execute(
                "INSERT INTO excluded_apps(executable_name) VALUES(?1)",
                [app],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn save_text_item(&self, item: &NewClipTextItem) -> Result<ClipItemDetail, AppError> {
        let now = Utc::now().timestamp_millis();
        let id = Uuid::new_v4().to_string();
        {
            let connection = self.connection.lock()?;
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO clip_items(
                    id, type, full_text, preview_text, search_text, source_app,
                    is_favorited, hash, created_at, updated_at, last_used_at, deleted_at
                ) VALUES(?1, 'text', ?2, ?3, ?4, ?5, 0, ?6, ?7, ?7, NULL, NULL)",
                params![
                    id,
                    item.normalized.full_text,
                    item.normalized.preview_text,
                    item.normalized.search_text,
                    item.source_app,
                    item.normalized.hash,
                    now
                ],
            )?;
            transaction.execute(
                "INSERT INTO clip_items_fts(item_id, full_text, search_text, source_app)
                 VALUES(?1, ?2, ?3, ?4)",
                params![
                    id,
                    item.normalized.full_text,
                    item.normalized.search_text,
                    item.source_app
                ],
            )?;
            transaction.commit()?;
        }
        self.get_item_detail(&id)
    }

    pub fn list_recent(&self, limit: u32) -> Result<Vec<ClipItemSummary>, AppError> {
        let connection = self.connection.lock()?;
        let sql = format!(
            "SELECT id, preview_text, source_app, is_favorited, created_at, updated_at, last_used_at
             FROM clip_items
             WHERE deleted_at IS NULL
             ORDER BY {}
             LIMIT ?1",
            activity_order_clause(""),
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([limit], map_summary_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_favorites(&self, limit: u32) -> Result<Vec<ClipItemSummary>, AppError> {
        let connection = self.connection.lock()?;
        let sql = format!(
            "SELECT id, preview_text, source_app, is_favorited, created_at, updated_at, last_used_at
             FROM clip_items
             WHERE deleted_at IS NULL AND is_favorited = 1
             ORDER BY {}
             LIMIT ?1",
            activity_order_clause(""),
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([limit], map_summary_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_item_detail(&self, id: &str) -> Result<ClipItemDetail, AppError> {
        let connection = self.connection.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, type, preview_text, full_text, search_text, source_app,
                    is_favorited, created_at, updated_at, last_used_at, hash
             FROM clip_items
             WHERE id = ?1 AND deleted_at IS NULL",
        )?;

        statement
            .query_row([id], map_detail_row)
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    AppError::Message("未找到对应剪贴记录".to_string())
                }
                other => AppError::Sqlite(other),
            })
    }

    pub fn update_text(
        &self,
        id: &str,
        text: &NewClipTextItem,
    ) -> Result<ClipItemDetail, AppError> {
        let now = Utc::now().timestamp_millis();
        {
            let connection = self.connection.lock()?;
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "UPDATE clip_items
                 SET full_text = ?2, preview_text = ?3, search_text = ?4, source_app = COALESCE(source_app, ?5),
                     hash = ?6, updated_at = ?7
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![
                    id,
                    text.normalized.full_text,
                    text.normalized.preview_text,
                    text.normalized.search_text,
                    text.source_app,
                    text.normalized.hash,
                    now
                ],
            )?;
            transaction.execute("DELETE FROM clip_items_fts WHERE item_id = ?1", [id])?;
            transaction.execute(
                "INSERT INTO clip_items_fts(item_id, full_text, search_text, source_app)
                 VALUES(?1, ?2, ?3, ?4)",
                params![
                    id,
                    text.normalized.full_text,
                    text.normalized.search_text,
                    text.source_app
                ],
            )?;
            transaction.commit()?;
        }
        self.get_item_detail(id)
    }

    pub fn delete_item(&self, id: &str) -> Result<(), AppError> {
        let now = Utc::now().timestamp_millis();
        let connection = self.connection.lock()?;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "UPDATE clip_items SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![id, now],
        )?;
        transaction.execute("DELETE FROM clip_items_fts WHERE item_id = ?1", [id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_favorited(&self, id: &str, value: bool) -> Result<(), AppError> {
        let connection = self.connection.lock()?;
        connection.execute(
            "UPDATE clip_items SET is_favorited = ?2, updated_at = ?3 WHERE id = ?1 AND deleted_at IS NULL",
            params![id, bool_to_i64(value), Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn mark_used(&self, id: &str) -> Result<(), AppError> {
        let now = Utc::now().timestamp_millis();
        let connection = self.connection.lock()?;
        connection.execute(
            "UPDATE clip_items SET last_used_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn has_recent_hash(&self, hash: &str, cutoff_timestamp_ms: i64) -> Result<bool, AppError> {
        let connection = self.connection.lock()?;
        let value = connection.query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM clip_items
                WHERE hash = ?1 AND deleted_at IS NULL AND created_at >= ?2
            )",
            params![hash, cutoff_timestamp_ms],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(value > 0)
    }

    /// 查找（未删除的）具有相同 hash 的现有记录，返回其 id
    pub fn find_existing_by_hash(&self, hash: &str) -> Result<Option<String>, AppError> {
        let connection = self.connection.lock()?;
        let id = connection
            .query_row(
                "SELECT id FROM clip_items WHERE hash = ?1 AND deleted_at IS NULL LIMIT 1",
                params![hash],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(id)
    }

    /// 刷新指定记录的 created_at 和 updated_at 到当前时间，使其排到列表顶部
    pub fn bump_item(&self, id: &str) -> Result<ClipItemDetail, AppError> {
        let now = Utc::now().timestamp_millis();
        {
            let connection = self.connection.lock()?;
            connection.execute(
                "UPDATE clip_items SET created_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, now],
            )?;
        }
        self.get_item_detail(id)
    }

    pub fn search(&self, query: SearchQuery) -> Result<SearchResult, AppError> {
        let normalized = query.normalized();
        let keyword = normalized.keyword.trim().to_string();
        if keyword.is_empty() {
            return self.search_recent(normalized);
        }
        self.search_with_keyword(normalized)
    }

    fn search_recent(&self, query: SearchQuery) -> Result<SearchResult, AppError> {
        let (where_clause, mut values) = build_filters_clause(&query);
        let connection = self.connection.lock()?;
        let count_sql = format!("SELECT COUNT(*) FROM clip_items WHERE {where_clause}");
        let total = connection.query_row(
            &count_sql,
            rusqlite::params_from_iter(values.clone()),
            |row| row.get::<_, u32>(0),
        )?;

        values.push(Value::Integer(i64::from(query.limit)));
        values.push(Value::Integer(i64::from(query.offset)));
        let sql = format!(
            "SELECT id, preview_text, source_app, is_favorited, created_at, updated_at, last_used_at
             FROM clip_items
             WHERE {where_clause}
             ORDER BY {}
             LIMIT ? OFFSET ?",
            activity_order_clause(""),
        );

        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), map_summary_row)?;
        Ok(SearchResult {
            items: rows.collect::<Result<Vec<_>, _>>()?,
            total,
            offset: query.offset,
            limit: query.limit,
        })
    }

    fn search_with_keyword(&self, query: SearchQuery) -> Result<SearchResult, AppError> {
        let fts_query = build_fts_query(&query.keyword);
        if fts_query.is_empty() {
            return self.search_recent(query);
        }

        let (filter_clause, filter_values) = build_filters_clause_with_alias(&query, "ci");
        let connection = self.connection.lock()?;
        let count_sql = format!(
            "SELECT COUNT(*)
             FROM clip_items_fts
             JOIN clip_items ci ON ci.id = clip_items_fts.item_id
             WHERE clip_items_fts MATCH ? AND {filter_clause}"
        );

        let mut count_values = vec![Value::Text(fts_query.clone())];
        count_values.extend(filter_values.clone());
        let total = connection.query_row(
            &count_sql,
            rusqlite::params_from_iter(count_values),
            |row| row.get::<_, u32>(0),
        )?;

        let mut data_values = vec![Value::Text(fts_query)];
        data_values.extend(filter_values);
        data_values.push(Value::Integer(i64::from(query.limit)));
        data_values.push(Value::Integer(i64::from(query.offset)));
        let order_clause = match query.sort {
            SearchSort::RecentDesc => activity_order_clause("ci"),
            SearchSort::RelevanceDesc => "bm25(clip_items_fts) ASC,
                 COALESCE(ci.last_used_at, ci.created_at) DESC,
                 ci.created_at DESC"
                .to_string(),
        };

        let sql = format!(
            "SELECT ci.id, ci.preview_text, ci.source_app, ci.is_favorited,
                    ci.created_at, ci.updated_at, ci.last_used_at
             FROM clip_items_fts
             JOIN clip_items ci ON ci.id = clip_items_fts.item_id
             WHERE clip_items_fts MATCH ? AND {filter_clause}
             ORDER BY {order_clause}
             LIMIT ? OFFSET ?"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(data_values), map_summary_row)?;
        Ok(SearchResult {
            items: rows.collect::<Result<Vec<_>, _>>()?,
            total,
            offset: query.offset,
            limit: query.limit,
        })
    }
}

fn build_filters_clause(query: &SearchQuery) -> (String, Vec<Value>) {
    build_filters_clause_with_alias(query, "")
}

fn build_filters_clause_with_alias(query: &SearchQuery, alias: &str) -> (String, Vec<Value>) {
    let mut clauses = vec![format!("{}deleted_at IS NULL", prefix(alias))];
    let mut values = Vec::new();

    if query.filters.favorited_only.unwrap_or(false) {
        clauses.push(format!("{}is_favorited = 1", prefix(alias)));
    }

    if let Some(source_app) = query
        .filters
        .source_app
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        clauses.push(format!("{}source_app = ?", prefix(alias)));
        values.push(Value::Text(source_app.to_string()));
    }

    (clauses.join(" AND "), values)
}

fn prefix(alias: &str) -> String {
    if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    }
}

fn activity_order_clause(alias: &str) -> String {
    format!(
        "COALESCE({0}last_used_at, {0}created_at) DESC,
         {0}created_at DESC",
        prefix(alias)
    )
}

fn build_fts_query(keyword: &str) -> String {
    keyword
        .split_whitespace()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn map_summary_row(row: &Row<'_>) -> rusqlite::Result<ClipItemSummary> {
    Ok(ClipItemSummary {
        id: row.get(0)?,
        content_preview: row.get(1)?,
        source_app: row.get(2)?,
        is_favorited: row.get::<_, i64>(3)? == 1,
        created_at: timestamp_to_iso(row.get_ref(4)?),
        updated_at: timestamp_to_iso(row.get_ref(5)?),
        last_used_at: optional_timestamp_to_iso(row.get_ref(6)?),
    })
}

fn map_detail_row(row: &Row<'_>) -> rusqlite::Result<ClipItemDetail> {
    Ok(ClipItemDetail {
        id: row.get(0)?,
        r#type: row.get(1)?,
        content_preview: row.get(2)?,
        full_text: row.get(3)?,
        search_text: row.get(4)?,
        source_app: row.get(5)?,
        is_favorited: row.get::<_, i64>(6)? == 1,
        created_at: timestamp_to_iso(row.get_ref(7)?),
        updated_at: timestamp_to_iso(row.get_ref(8)?),
        last_used_at: optional_timestamp_to_iso(row.get_ref(9)?),
        hash: row.get(10)?,
    })
}

fn timestamp_to_iso(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Integer(timestamp) => {
            if let Some(date) = DateTime::<Utc>::from_timestamp_millis(timestamp) {
                return date.to_rfc3339();
            }
            warn!("遇到非法时间戳: {timestamp}");
            Utc::now().to_rfc3339()
        }
        other => {
            warn!("时间戳字段类型异常: {:?}", other.data_type());
            Utc::now().to_rfc3339()
        }
    }
}

fn optional_timestamp_to_iso(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Null => None,
        _ => Some(timestamp_to_iso(value)),
    }
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use chrono::{Duration, Utc};

    use super::{bool_to_i64, SqliteRepository};
    use crate::domain::clip_item::{SearchFilters, SearchQuery, SearchSort};

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "floatpaste-repository-test-{}.db",
            uuid::Uuid::new_v4()
        ))
    }

    fn seed_item(
        repository: &SqliteRepository,
        id: &str,
        preview: &str,
        created_at: i64,
        last_used_at: Option<i64>,
        is_favorited: bool,
    ) {
        let connection = repository.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO clip_items(
                    id, type, full_text, preview_text, search_text, source_app,
                    is_favorited, hash, created_at, updated_at, last_used_at, deleted_at
                ) VALUES(?1, 'text', ?2, ?2, ?2, NULL, ?3, ?4, ?5, ?5, ?6, NULL)",
                rusqlite::params![
                    id,
                    preview,
                    bool_to_i64(is_favorited),
                    format!("hash-{id}"),
                    created_at,
                    last_used_at
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO clip_items_fts(item_id, full_text, search_text, source_app)
                 VALUES(?1, ?2, ?2, NULL)",
                rusqlite::params![id, preview],
            )
            .unwrap();
    }

    #[test]
    fn list_recent_prioritizes_newest_created_item_before_older_used_item() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let now = Utc::now();

        seed_item(
            &repository,
            "older-used",
            "older used",
            (now - Duration::minutes(10)).timestamp_millis(),
            Some((now - Duration::minutes(2)).timestamp_millis()),
            false,
        );
        seed_item(
            &repository,
            "new-created",
            "new created",
            (now - Duration::minutes(1)).timestamp_millis(),
            None,
            false,
        );

        let items = repository.list_recent(10).unwrap();

        assert_eq!(items[0].id, "new-created");
        assert_eq!(items[1].id, "older-used");

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn mark_used_moves_item_to_top_until_newer_capture_arrives() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let now = Utc::now();

        seed_item(
            &repository,
            "older-created",
            "older created",
            (now - Duration::minutes(5)).timestamp_millis(),
            None,
            false,
        );
        seed_item(
            &repository,
            "new-created",
            "new created",
            (now - Duration::minutes(1)).timestamp_millis(),
            None,
            false,
        );

        repository.mark_used("older-created").unwrap();
        let after_mark_used = repository.list_recent(10).unwrap();
        assert_eq!(after_mark_used[0].id, "older-created");

        let latest_created_at = Utc::now().timestamp_millis() + 1_000;
        seed_item(
            &repository,
            "latest-created",
            "latest created",
            latest_created_at,
            None,
            false,
        );
        let after_new_capture = repository.list_recent(10).unwrap();
        assert_eq!(after_new_capture[0].id, "latest-created");

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn search_recent_desc_uses_activity_time_without_favorite_boost() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let now = Utc::now();

        seed_item(
            &repository,
            "favorite-older",
            "shared keyword",
            (now - Duration::minutes(8)).timestamp_millis(),
            Some((now - Duration::minutes(4)).timestamp_millis()),
            true,
        );
        seed_item(
            &repository,
            "newer-normal",
            "shared keyword",
            (now - Duration::minutes(1)).timestamp_millis(),
            None,
            false,
        );

        let result = repository
            .search(SearchQuery {
                keyword: String::new(),
                filters: SearchFilters::default(),
                offset: 0,
                limit: 10,
                sort: SearchSort::RecentDesc,
            })
            .unwrap();

        assert_eq!(result.items[0].id, "newer-normal");

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn find_existing_by_hash_returns_id() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let now = Utc::now();

        seed_item(
            &repository,
            "item-a",
            "hello",
            (now - Duration::minutes(5)).timestamp_millis(),
            None,
            false,
        );
        // seed_item 使用 format!("hash-{id}") 作为 hash
        let found = repository.find_existing_by_hash("hash-item-a").unwrap();
        assert_eq!(found, Some("item-a".to_string()));

        let not_found = repository
            .find_existing_by_hash("nonexistent-hash")
            .unwrap();
        assert_eq!(not_found, None);

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn find_existing_by_hash_ignores_deleted() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let now = Utc::now();

        seed_item(
            &repository,
            "deleted-item",
            "deleted content",
            (now - Duration::minutes(5)).timestamp_millis(),
            None,
            false,
        );
        // 软删除这条记录
        repository.delete_item("deleted-item").unwrap();

        let found = repository
            .find_existing_by_hash("hash-deleted-item")
            .unwrap();
        assert_eq!(found, None);

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn bump_item_updates_timestamps() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let old_time = (Utc::now() - Duration::hours(1)).timestamp_millis();

        seed_item(&repository, "old-item", "bump me", old_time, None, true);

        let before = repository.get_item_detail("old-item").unwrap();
        let bumped = repository.bump_item("old-item").unwrap();

        // 时间戳应该被更新到更新的值
        assert_ne!(bumped.created_at, before.created_at);
        assert_ne!(bumped.updated_at, before.updated_at);
        // 收藏状态应保持不变
        assert!(bumped.is_favorited);

        drop(repository);
        fs::remove_file(path).unwrap();
    }
}
