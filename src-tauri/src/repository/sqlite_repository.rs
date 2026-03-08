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

        let mut statement =
            connection.prepare("SELECT executable_name FROM excluded_apps ORDER BY executable_name ASC")?;
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
        let mut statement = connection.prepare(
            "SELECT id, preview_text, source_app, is_favorited, created_at, updated_at, last_used_at
             FROM clip_items
             WHERE deleted_at IS NULL
             ORDER BY
               is_favorited DESC,
               CASE WHEN last_used_at IS NULL THEN 1 ELSE 0 END ASC,
               last_used_at DESC,
               created_at DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], map_summary_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_favorites(&self, limit: u32) -> Result<Vec<ClipItemSummary>, AppError> {
        let connection = self.connection.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, preview_text, source_app, is_favorited, created_at, updated_at, last_used_at
             FROM clip_items
             WHERE deleted_at IS NULL AND is_favorited = 1
             ORDER BY
               CASE WHEN last_used_at IS NULL THEN 1 ELSE 0 END ASC,
               last_used_at DESC,
               created_at DESC
             LIMIT ?1",
        )?;
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

    pub fn update_text(&self, id: &str, text: &NewClipTextItem) -> Result<ClipItemDetail, AppError> {
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
             ORDER BY
               is_favorited DESC,
               CASE WHEN last_used_at IS NULL THEN 1 ELSE 0 END ASC,
               last_used_at DESC,
               created_at DESC
             LIMIT ? OFFSET ?"
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
            SearchSort::RecentDesc => {
                "ci.is_favorited DESC,
                 CASE WHEN ci.last_used_at IS NULL THEN 1 ELSE 0 END ASC,
                 ci.last_used_at DESC,
                 ci.created_at DESC"
            }
            SearchSort::RelevanceDesc => {
                "ci.is_favorited DESC,
                 bm25(clip_items_fts) ASC,
                 CASE WHEN ci.last_used_at IS NULL THEN 1 ELSE 0 END ASC,
                 ci.last_used_at DESC,
                 ci.created_at DESC"
            }
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
