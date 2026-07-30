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
        ClipItemDetail, ClipItemSummary, NewClipFileItem, NewClipImageItem, NewClipTextItem,
        SearchQuery, SearchResult, SearchSort,
    },
    error::AppError,
};

#[derive(Clone)]
pub struct SqliteRepository {
    pub(super) connection: Arc<Mutex<Connection>>,
}

impl SqliteRepository {
    pub fn new(path: &Path) -> Result<Self, AppError> {
        let connection = Connection::open(path)?;
        super::schema::initialize_database(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
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

    /// 刷新指定记录的 created_at、updated_at 和 last_used_at 到当前时间，使其排到列表顶部。
    ///
    /// 调用场景：用户重新复制了与历史记录中某项完全相同的内容，去重命中后置顶已有项。
    /// 列表排序为 `COALESCE(last_used_at, created_at) DESC`：
    /// - 若仅刷新 created_at，曾被 mark_used 的记录（last_used_at 已设旧值）排序键
    ///   仍指向旧的 last_used_at，无法置顶。
    /// - 因此这里同时刷新 last_used_at。语义上，"重新复制相同内容"本身就是一次
    ///   活跃互动（与粘贴同类），将其计入 last_used_at 与用户对"最近使用"的直觉一致。
    pub fn bump_item(&self, id: &str) -> Result<ClipItemDetail, AppError> {
        let now = Utc::now().timestamp_millis();
        {
            let connection = self.connection.lock()?;
            connection.execute(
                "UPDATE clip_items SET created_at = ?2, updated_at = ?2, last_used_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
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

    if let Some(clip_type) = query.filters.clip_type.as_ref() {
        clauses.push(format!("{}type = ?", prefix(alias)));
        values.push(Value::Text(clip_type.as_str().to_string()));
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

    let analyzed = crate::services::normalize_service::analyze_file_paths(file_paths);
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

    use super::{bool_to_i64, SqliteRepository};
    use super::super::settings::PICKER_WINDOW_STATE_KEY;
    use crate::domain::{
        clip_item::{ClipType, SearchFilters, SearchQuery, SearchSort},
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

    fn seed_typed_item(
        repository: &SqliteRepository,
        id: &str,
        clip_type: &str,
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
                ) VALUES(?1, ?2, ?3, ?3, ?3, NULL, ?4, ?5, ?6, ?6, ?7, NULL)",
                rusqlite::params![
                    id,
                    clip_type,
                    if clip_type == "text" { preview } else { "" },
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
                 VALUES(?1, ?2, ?3, NULL)",
                rusqlite::params![id, if clip_type == "text" { preview } else { "" }, preview],
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
    fn search_filters_by_clip_type_for_recent_and_keyword_results() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let now = Utc::now();

        seed_typed_item(
            &repository,
            "text-item",
            "text",
            "shared text keyword",
            (now - Duration::minutes(3)).timestamp_millis(),
            None,
            false,
        );
        seed_typed_item(
            &repository,
            "image-item",
            "image",
            "shared image keyword",
            (now - Duration::minutes(2)).timestamp_millis(),
            None,
            false,
        );
        seed_typed_item(
            &repository,
            "file-item",
            "file",
            "shared file keyword",
            (now - Duration::minutes(1)).timestamp_millis(),
            None,
            false,
        );

        let recent_result = repository
            .search(SearchQuery {
                keyword: String::new(),
                filters: SearchFilters {
                    favorited_only: None,
                    clip_type: Some(ClipType::Image),
                    source_app: None,
                    include_deleted: None,
                },
                offset: 0,
                limit: 10,
                sort: SearchSort::RecentDesc,
            })
            .unwrap();

        assert_eq!(recent_result.total, 1);
        assert_eq!(recent_result.items.len(), 1);
        assert_eq!(recent_result.items[0].id, "image-item");

        let keyword_result = repository
            .search(SearchQuery {
                keyword: "shared".to_string(),
                filters: SearchFilters {
                    favorited_only: None,
                    clip_type: Some(ClipType::File),
                    source_app: None,
                    include_deleted: None,
                },
                offset: 0,
                limit: 10,
                sort: SearchSort::RelevanceDesc,
            })
            .unwrap();

        assert_eq!(keyword_result.total, 1);
        assert_eq!(keyword_result.items.len(), 1);
        assert_eq!(keyword_result.items[0].id, "file-item");

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
        // last_used_at 也应被刷新，避免排序仍指向旧时间戳导致条目无法置顶
        assert_ne!(bumped.last_used_at, before.last_used_at);
        assert!(bumped.last_used_at.is_some());
        // 收藏状态应保持不变
        assert!(bumped.is_favorited);

        drop(repository);
        fs::remove_file(path).unwrap();
    }

    /// 回归测试：当用户重新复制与历史记录中已有项完全相同的内容时，
    /// 该已有项应被置顶，即便它之前曾被粘贴使用过（last_used_at 非空）。
    ///
    /// 复现条件：列表排序键为 `COALESCE(last_used_at, created_at) DESC`。
    /// 若 bump_item 只更新 created_at 而不动 last_used_at，
    /// 对 last_used_at 已有值的记录，重新复制时其排序位置不会变化。
    #[test]
    fn bump_item_moves_already_used_item_to_top() {
        let path = temp_db_path();
        let repository = SqliteRepository::new(&path).unwrap();
        let now = Utc::now();

        // 一条"曾被粘贴使用过"的旧记录（last_used_at 已设为 8 分钟前）
        seed_item(
            &repository,
            "previously-used",
            "shared content",
            (now - Duration::minutes(30)).timestamp_millis(),
            Some((now - Duration::minutes(8)).timestamp_millis()),
            false,
        );
        // 一条稍后创建、但活跃时间早于"刚刚重新复制"的新记录
        seed_item(
            &repository,
            "newer-item",
            "newer content",
            (now - Duration::minutes(2)).timestamp_millis(),
            None,
            false,
        );

        // 初始顺序：newer-item（2 分钟前）排在 previously-used（last_used_at = 8 分钟前）之前
        let items = repository.list_recent(10).unwrap();
        assert_eq!(items[0].id, "newer-item");
        assert_eq!(items[1].id, "previously-used");

        // 模拟"再次复制相同内容"：去重命中后调用 bump_item
        let before_bump = Utc::now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let bumped = repository.bump_item("previously-used").unwrap();
        assert!(bumped.last_used_at.is_some());

        // 修复后：previously-used 应被置顶，因为 last_used_at 已刷新为当前时间
        let items = repository.list_recent(10).unwrap();
        assert_eq!(
            items[0].id, "previously-used",
            "重新复制相同内容应把已有项置顶，即使它曾被粘贴使用过"
        );
        assert_eq!(items[1].id, "newer-item");

        // bumped.last_used_at 必须严格晚于 bump 之前的时间戳，证明已被刷新
        let bumped_used_at = chrono::DateTime::parse_from_rfc3339(&bumped.last_used_at.unwrap())
            .unwrap()
            .with_timezone(&Utc);
        assert!(
            bumped_used_at > before_bump,
            "last_used_at 应被刷新为当前时间，实际: {bumped_used_at:?}"
        );

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
