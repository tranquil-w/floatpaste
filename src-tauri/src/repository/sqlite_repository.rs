use std::{
    fs,
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
        ClipItemDetail, ClipItemSummary, NewClipFileItem, NewClipImageItem, NewClipTextItem,
        SearchQuery, SearchResult, SearchSort,
    },
    error::AppError,
    settings::{StoredWindowPosition, UserSetting},
};

const USER_SETTINGS_KEY: &str = "user_settings";
// 继续复用旧 key，兼容已落盘的“仅位置”状态。
const PICKER_WINDOW_STATE_KEY: &str = "picker_last_position";
const CURRENT_SCHEMA_VERSION: i32 = 3;
const CLIP_ITEMS_MEDIA_COLUMNS: [(&str, &str); 7] = [
    ("image_path", "TEXT NULL"),
    ("image_width", "INTEGER NULL"),
    ("image_height", "INTEGER NULL"),
    ("image_format", "TEXT NULL"),
    ("file_size", "INTEGER NULL"),
    ("file_paths", "TEXT NOT NULL DEFAULT '[]'"),
    ("file_count", "INTEGER NOT NULL DEFAULT 0"),
];
const CLIP_ITEMS_TOTAL_SIZE_COLUMN: (&str, &str) = ("total_size", "INTEGER NULL");
const CLIP_ITEMS_DIRECTORY_COUNT_COLUMN: (&str, &str) =
    ("directory_count", "INTEGER NOT NULL DEFAULT 0");

#[derive(Clone)]
pub struct SqliteRepository {
    connection: Arc<Mutex<Connection>>,
}

impl SqliteRepository {
    pub fn new(path: &Path) -> Result<Self, AppError> {
        let connection = Connection::open(path)?;
        initialize_database(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn load_settings(&self) -> Result<UserSetting, AppError> {
        let connection = self.connection.lock()?;
        let settings_json = connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [USER_SETTINGS_KEY],
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

    pub fn load_picker_window_state(&self) -> Result<Option<StoredWindowPosition>, AppError> {
        let connection = self.connection.lock()?;
        let raw_value = connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [PICKER_WINDOW_STATE_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        raw_value
            .map(|raw| serde_json::from_str::<StoredWindowPosition>(&raw))
            .transpose()
            .map_err(Into::into)
    }

    pub fn save_settings(&self, setting: &UserSetting) -> Result<(), AppError> {
        let connection = self.connection.lock()?;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![USER_SETTINGS_KEY, serde_json::to_string(setting)?],
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

    pub fn save_picker_window_state(
        &self,
        position: &StoredWindowPosition,
    ) -> Result<(), AppError> {
        let connection = self.connection.lock()?;
        connection.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![PICKER_WINDOW_STATE_KEY, serde_json::to_string(position)?],
        )?;
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
                    is_favorited, hash, created_at, updated_at, last_used_at, deleted_at,
                    image_path, image_width, image_height, image_format, file_size, file_paths, file_count, total_size, directory_count
                ) VALUES(?1, 'text', ?2, ?3, ?4, ?5, 0, ?6, ?7, ?7, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '[]', 0, NULL, 0)",
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

    pub fn save_image_item(&self, item: &NewClipImageItem) -> Result<ClipItemDetail, AppError> {
        let now = Utc::now().timestamp_millis();
        let id = Uuid::new_v4().to_string();
        {
            let connection = self.connection.lock()?;
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO clip_items(
                    id, type, full_text, preview_text, search_text, source_app,
                    is_favorited, hash, created_at, updated_at, last_used_at, deleted_at,
                    image_path, image_width, image_height, image_format, file_size, file_paths, file_count, total_size, directory_count
                ) VALUES(?1, 'image', '', ?2, ?3, ?4, 0, ?5, ?6, ?6, NULL, NULL, ?7, ?8, ?9, ?10, ?11, '[]', 0, NULL, 0)",
                params![
                    id,
                    item.normalized.preview_text,
                    item.normalized.search_text,
                    item.source_app,
                    item.normalized.hash,
                    now,
                    item.normalized.image_path,
                    item.normalized.image_width,
                    item.normalized.image_height,
                    item.normalized.image_format,
                    item.normalized.file_size
                ],
            )?;
            transaction.execute(
                "INSERT INTO clip_items_fts(item_id, full_text, search_text, source_app)
                 VALUES(?1, '', ?2, ?3)",
                params![id, item.normalized.search_text, item.source_app],
            )?;
            transaction.commit()?;
        }
        self.get_item_detail(&id)
    }

    pub fn save_file_item(&self, item: &NewClipFileItem) -> Result<ClipItemDetail, AppError> {
        let now = Utc::now().timestamp_millis();
        let id = Uuid::new_v4().to_string();
        let file_paths_json = serde_json::to_string(&item.normalized.file_paths)?;
        {
            let connection = self.connection.lock()?;
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO clip_items(
                    id, type, full_text, preview_text, search_text, source_app,
                    is_favorited, hash, created_at, updated_at, last_used_at, deleted_at,
                    image_path, image_width, image_height, image_format, file_size, file_paths, file_count, total_size, directory_count
                ) VALUES(?1, 'file', '', ?2, ?3, ?4, 0, ?5, ?6, ?6, NULL, NULL, NULL, NULL, NULL, NULL, NULL, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    item.normalized.preview_text,
                    item.normalized.search_text,
                    item.source_app,
                    item.normalized.hash,
                    now,
                    file_paths_json,
                    item.normalized.file_count,
                    item.normalized.total_size,
                    item.normalized.directory_count
                ],
            )?;
            transaction.execute(
                "INSERT INTO clip_items_fts(item_id, full_text, search_text, source_app)
                 VALUES(?1, '', ?2, ?3)",
                params![id, item.normalized.search_text, item.source_app],
            )?;
            transaction.commit()?;
        }
        self.get_item_detail(&id)
    }

    pub fn list_recent(&self, limit: u32) -> Result<Vec<ClipItemSummary>, AppError> {
        let connection = self.connection.lock()?;
        let sql = format!(
            "SELECT id, type, preview_text, source_app, is_favorited,
                    image_path, image_width, image_height, image_format, file_size,
                    file_paths, file_count, total_size, directory_count,
                    created_at, updated_at, last_used_at, substr(full_text, 1, 3000)
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
            "SELECT id, type, preview_text, source_app, is_favorited,
                    image_path, image_width, image_height, image_format, file_size,
                    file_paths, file_count, total_size, directory_count,
                    created_at, updated_at, last_used_at, substr(full_text, 1, 3000)
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
                    is_favorited, created_at, updated_at, last_used_at, hash,
                    image_path, image_width, image_height, image_format, file_size,
                    file_paths, file_count, total_size, directory_count
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
            "SELECT id, type, preview_text, source_app, is_favorited,
                    image_path, image_width, image_height, image_format, file_size,
                    file_paths, file_count, total_size, directory_count,
                    created_at, updated_at, last_used_at, substr(full_text, 1, 3000)
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
            "SELECT ci.id, ci.type, ci.preview_text, ci.source_app, ci.is_favorited,
                    ci.image_path, ci.image_width, ci.image_height, ci.image_format, ci.file_size,
                    ci.file_paths, ci.file_count, ci.total_size, ci.directory_count,
                    ci.created_at, ci.updated_at, ci.last_used_at, substr(ci.full_text, 1, 3000)
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

fn initialize_database(connection: &Connection) -> Result<(), AppError> {
    connection.execute_batch(include_str!("../../migrations/0001_init.sql"))?;

    let schema_version = current_schema_version(connection)?;
    if schema_version < CURRENT_SCHEMA_VERSION {
        apply_schema_upgrades(connection, schema_version)?;
    } else if !clip_items_has_media_columns(connection)? {
        // 兼容历史上未记录 user_version 的已有数据库。
        apply_media_columns_migration(connection)?;
        set_schema_version(connection, CURRENT_SCHEMA_VERSION)?;
    }

    Ok(())
}

fn apply_schema_upgrades(connection: &Connection, current_version: i32) -> Result<(), AppError> {
    if current_version < 2 {
        apply_media_columns_migration(connection)?;
    }

    if current_version < 3 {
        apply_directory_count_migration(connection)?;
    }

    set_schema_version(connection, CURRENT_SCHEMA_VERSION)
}

fn apply_media_columns_migration(connection: &Connection) -> Result<(), AppError> {
    for (column_name, definition) in clip_items_media_columns() {
        if column_exists(connection, "clip_items", column_name)? {
            continue;
        }

        connection.execute(
            &format!("ALTER TABLE clip_items ADD COLUMN {column_name} {definition}"),
            [],
        )?;
    }

    Ok(())
}

fn apply_directory_count_migration(connection: &Connection) -> Result<(), AppError> {
    let (column_name, definition) = CLIP_ITEMS_DIRECTORY_COUNT_COLUMN;
    if !column_exists(connection, "clip_items", column_name)? {
        connection.execute(
            &format!("ALTER TABLE clip_items ADD COLUMN {column_name} {definition}"),
            [],
        )?;
    }

    Ok(())
}

fn current_schema_version(connection: &Connection) -> Result<i32, AppError> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(Into::into)
}

fn set_schema_version(connection: &Connection, version: i32) -> Result<(), AppError> {
    connection.pragma_update(None, "user_version", version)?;
    Ok(())
}

fn clip_items_has_media_columns(connection: &Connection) -> Result<bool, AppError> {
    for (column_name, _) in clip_items_media_columns() {
        if !column_exists(connection, "clip_items", column_name)? {
            return Ok(false);
        }
    }

    Ok(true)
}

fn clip_items_media_columns() -> impl Iterator<Item = (&'static str, &'static str)> {
    CLIP_ITEMS_MEDIA_COLUMNS
        .iter()
        .copied()
        .chain(std::iter::once(CLIP_ITEMS_TOTAL_SIZE_COLUMN))
        .chain(std::iter::once(CLIP_ITEMS_DIRECTORY_COUNT_COLUMN))
}

fn column_exists(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
) -> Result<bool, AppError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

    for existing_column in rows {
        if existing_column? == column_name {
            return Ok(true);
        }
    }

    Ok(false)
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
    let r#type: String = row.get(1)?;
    let preview_text: String = row.get(2)?;
    let file_paths = parse_file_paths(row.get::<_, String>(10)?);
    let file_count: i32 = row.get(11)?;
    let stored_total_size: Option<i64> = row.get(12)?;
    let stored_directory_count: i32 = row.get(13)?;
    let resolved_file_fields = resolve_file_clip_fields(
        &r#type,
        &file_paths,
        file_count,
        stored_directory_count,
        stored_total_size,
    );

    let tooltip_text: Option<String> = row
        .get::<_, Option<String>>(17)
        .unwrap_or_default()
        .filter(|s| !s.is_empty());

    Ok(ClipItemSummary {
        id: row.get(0)?,
        r#type,
        content_preview: resolved_file_fields.content_preview.unwrap_or(preview_text),
        tooltip_text,
        source_app: row.get(3)?,
        is_favorited: row.get::<_, i64>(4)? == 1,
        file_count: resolved_file_fields.file_count,
        directory_count: resolved_file_fields.directory_count,
        created_at: timestamp_to_iso(row.get_ref(14)?),
        updated_at: timestamp_to_iso(row.get_ref(15)?),
        last_used_at: optional_timestamp_to_iso(row.get_ref(16)?),
        image_path: row.get(5)?,
        image_width: row.get(6)?,
        image_height: row.get(7)?,
        image_format: row.get(8)?,
        file_size: row.get(9)?,
    })
}

fn map_detail_row(row: &Row<'_>) -> rusqlite::Result<ClipItemDetail> {
    let r#type: String = row.get(1)?;
    let preview_text: String = row.get(2)?;
    let file_paths = parse_file_paths(row.get::<_, String>(16)?);
    let file_count: i32 = row.get(17)?;
    let stored_total_size: Option<i64> = row.get(18)?;
    let stored_directory_count: i32 = row.get(19)?;
    let resolved_file_fields = resolve_file_clip_fields(
        &r#type,
        &file_paths,
        file_count,
        stored_directory_count,
        stored_total_size,
    );

    Ok(ClipItemDetail {
        id: row.get(0)?,
        r#type,
        content_preview: resolved_file_fields.content_preview.unwrap_or(preview_text),
        full_text: row.get(3)?,
        search_text: row.get(4)?,
        source_app: row.get(5)?,
        is_favorited: row.get::<_, i64>(6)? == 1,
        created_at: timestamp_to_iso(row.get_ref(7)?),
        updated_at: timestamp_to_iso(row.get_ref(8)?),
        last_used_at: optional_timestamp_to_iso(row.get_ref(9)?),
        hash: row.get(10)?,
        image_path: row.get(11)?,
        image_width: row.get(12)?,
        image_height: row.get(13)?,
        image_format: row.get(14)?,
        file_size: row.get(15)?,
        file_paths,
        file_count: resolved_file_fields.file_count,
        directory_count: resolved_file_fields.directory_count,
        total_size: resolved_file_fields.total_size,
    })
}

fn parse_file_paths(file_paths_str: String) -> Vec<String> {
    serde_json::from_str(&file_paths_str).unwrap_or_else(|error| {
        warn!("解析 file_paths JSON 失败: {error}");
        Vec::new()
    })
}

#[derive(Debug, Clone)]
struct ResolvedFileClipFields {
    content_preview: Option<String>,
    file_count: i32,
    directory_count: i32,
    total_size: Option<i64>,
}

fn resolve_file_clip_fields(
    clip_type: &str,
    file_paths: &[String],
    stored_file_count: i32,
    stored_directory_count: i32,
    stored_total_size: Option<i64>,
) -> ResolvedFileClipFields {
    if clip_type != "file" {
        return ResolvedFileClipFields {
            content_preview: None,
            file_count: stored_file_count,
            directory_count: stored_directory_count,
            total_size: stored_total_size,
        };
    }

    let analyzed = analyze_file_paths(file_paths);
    let file_count = if stored_file_count > 0 {
        stored_file_count
    } else {
        file_paths.len() as i32
    };
    let directory_count = stored_directory_count
        .max(analyzed.directory_count)
        .clamp(0, file_count);
    let total_size = if directory_count > 0 {
        None
    } else {
        stored_total_size.or(analyzed.total_size)
    };

    ResolvedFileClipFields {
        content_preview: Some(crate::services::normalize_service::build_file_preview(
            file_paths,
            file_count,
            directory_count,
            total_size,
        )),
        file_count,
        directory_count,
        total_size,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct FilePathAnalysis {
    directory_count: i32,
    total_size: Option<i64>,
}

fn analyze_file_paths(file_paths: &[String]) -> FilePathAnalysis {
    let mut total_size = 0i64;
    let mut directory_count = 0i32;
    let mut size_available = true;

    for path in file_paths {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => {
                size_available = false;
                continue;
            }
        };

        if metadata.is_dir() {
            directory_count += 1;
            size_available = false;
            continue;
        }

        let file_size = match i64::try_from(metadata.len()) {
            Ok(file_size) => file_size,
            Err(_) => {
                size_available = false;
                continue;
            }
        };
        total_size = match total_size.checked_add(file_size) {
            Some(total_size) => total_size,
            None => {
                size_available = false;
                continue;
            }
        };
    }

    FilePathAnalysis {
        directory_count,
        total_size: if size_available {
            Some(total_size)
        } else {
            None
        },
    }
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
    use rusqlite::{params, Connection};

    use super::{bool_to_i64, SqliteRepository, PICKER_WINDOW_STATE_KEY};
    use crate::domain::{
        clip_item::{SearchFilters, SearchQuery, SearchSort},
        settings::StoredWindowPosition,
    };

    const LEGACY_SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS clip_items (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  full_text TEXT NOT NULL,
  preview_text TEXT NOT NULL,
  search_text TEXT NOT NULL,
  source_app TEXT NULL,
  is_favorited INTEGER NOT NULL DEFAULT 0,
  hash TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_used_at INTEGER NULL,
  deleted_at INTEGER NULL
);

CREATE INDEX IF NOT EXISTS idx_clip_items_created_at ON clip_items(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_clip_items_last_used_at ON clip_items(last_used_at DESC);
CREATE INDEX IF NOT EXISTS idx_clip_items_hash ON clip_items(hash);
CREATE INDEX IF NOT EXISTS idx_clip_items_source_app ON clip_items(source_app);

CREATE VIRTUAL TABLE IF NOT EXISTS clip_items_fts USING fts5(
  item_id UNINDEXED,
  full_text,
  search_text,
  source_app
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS excluded_apps (
  executable_name TEXT PRIMARY KEY
);
"#;

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "floatpaste-repository-test-{}.db",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn new_upgrades_legacy_database_with_media_columns() {
        let path = temp_db_path();
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(LEGACY_SCHEMA_SQL).unwrap();
        drop(connection);

        let repository = SqliteRepository::new(&path).unwrap();
        let detail = repository
            .save_file_item(&crate::domain::clip_item::NewClipFileItem {
                normalized: crate::domain::clip_item::NormalizedClipFile {
                    preview_text: "文件: demo.txt".to_string(),
                    search_text: "demo.txt".to_string(),
                    hash: "file-hash".to_string(),
                    file_paths: vec!["C:\\Temp\\demo.txt".to_string()],
                    file_count: 1,
                    directory_count: 0,
                    total_size: Some(42),
                },
                source_app: Some("资源管理器".to_string()),
            })
            .unwrap();

        assert_eq!(detail.r#type, "file");
        assert_eq!(detail.file_paths, vec!["C:\\Temp\\demo.txt".to_string()]);
        assert_eq!(detail.total_size, Some(42));

        let version = repository
            .connection
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
            .unwrap();
        assert_eq!(version, 3);

        drop(repository);
        fs::remove_file(path).unwrap();
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
    fn save_image_item_round_trips_image_path_and_metadata() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let item = crate::domain::clip_item::NewClipImageItem {
            normalized: crate::domain::clip_item::NormalizedClipImage {
                preview_text: "图片 (16 x 16, 92.0 B)".to_string(),
                search_text: "图片 16 x 16 png 92.0 b".to_string(),
                hash: "image-hash".to_string(),
                image_path: Some("images/test.png".to_string()),
                image_width: Some(16),
                image_height: Some(16),
                image_format: Some("png".to_string()),
                file_size: Some(92),
            },
            source_app: Some("画图".to_string()),
        };

        let detail = repository.save_image_item(&item).unwrap();

        assert_eq!(detail.r#type, "image");
        assert_eq!(detail.image_path.as_deref(), Some("images/test.png"));
        assert_eq!(detail.image_width, Some(16));
        assert_eq!(detail.image_height, Some(16));
        assert_eq!(detail.image_format.as_deref(), Some("png"));
        assert_eq!(detail.file_size, Some(92));

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn summary_includes_image_metadata_for_recent_and_search_results() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let item = crate::domain::clip_item::NewClipImageItem {
            normalized: crate::domain::clip_item::NormalizedClipImage {
                preview_text: "图片 (16 x 16, 92 B)".to_string(),
                search_text: "diagram preview png".to_string(),
                hash: "image-hash-summary".to_string(),
                image_path: Some("images/test-summary.png".to_string()),
                image_width: Some(16),
                image_height: Some(16),
                image_format: Some("png".to_string()),
                file_size: Some(92),
            },
            source_app: Some("画图".to_string()),
        };

        let detail = repository.save_image_item(&item).unwrap();
        let recent_items = repository.list_recent(10).unwrap();
        let recent_summary = recent_items
            .iter()
            .find(|summary| summary.id == detail.id)
            .expect("recent summary should contain saved image item");

        assert_eq!(recent_summary.image_path.as_deref(), Some("images/test-summary.png"));
        assert_eq!(recent_summary.image_width, Some(16));
        assert_eq!(recent_summary.image_height, Some(16));
        assert_eq!(recent_summary.image_format.as_deref(), Some("png"));
        assert_eq!(recent_summary.file_size, Some(92));

        let search_result = repository
            .search(SearchQuery {
                keyword: "diagram".to_string(),
                filters: SearchFilters::default(),
                offset: 0,
                limit: 10,
                sort: SearchSort::RelevanceDesc,
            })
            .unwrap();
        let search_summary = search_result
            .items
            .iter()
            .find(|summary| summary.id == detail.id)
            .expect("search summary should contain saved image item");

        assert_eq!(search_summary.image_path.as_deref(), Some("images/test-summary.png"));
        assert_eq!(search_summary.image_width, Some(16));
        assert_eq!(search_summary.image_height, Some(16));
        assert_eq!(search_summary.image_format.as_deref(), Some("png"));
        assert_eq!(search_summary.file_size, Some(92));

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

    #[test]
    fn save_and_load_picker_window_state() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let position = StoredWindowPosition {
            x: 320,
            y: 180,
            width: Some(520),
            height: Some(640),
        };

        repository.save_picker_window_state(&position).unwrap();
        let loaded = repository.load_picker_window_state().unwrap();

        assert_eq!(loaded, Some(position));

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn load_picker_window_state_is_backward_compatible_with_position_only_payload() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let connection = repository.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2)",
                params![PICKER_WINDOW_STATE_KEY, r#"{"x":320,"y":180}"#],
            )
            .unwrap();
        drop(connection);

        let loaded = repository.load_picker_window_state().unwrap();

        assert_eq!(
            loaded,
            Some(StoredWindowPosition {
                x: 320,
                y: 180,
                width: None,
                height: None,
            })
        );

        drop(repository);
        fs::remove_file(path).unwrap();
    }
}
