use rusqlite::{params, Connection};

use crate::{
    domain::error::AppError,
    services::normalize_service::space_cjk_text,
};

const CURRENT_SCHEMA_VERSION: i32 = 5;
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

/// 初始化数据库：加载初始 schema 并按版本号执行增量迁移。
///
/// 兼容历史上未记录 `user_version` 的已有数据库：当检测到 media 列缺失但
/// `user_version` 已是最新时，仍会补齐 media 列迁移。
pub(super) fn initialize_database(connection: &Connection) -> Result<(), AppError> {
    connection.execute_batch(include_str!("../../migrations/0001_init.sql"))?;

    let schema_version = current_schema_version(connection)?;
    if schema_version < CURRENT_SCHEMA_VERSION {
        apply_schema_upgrades(connection, schema_version)?;
    } else if !clip_items_has_media_columns(connection)? {
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

    if current_version < 4 {
        apply_fts_cjk_index_migration(connection)?;
    }

    if current_version < 5 {
        apply_tags_migration(connection)?;
    }

    set_schema_version(connection, CURRENT_SCHEMA_VERSION)
}

/// v4：重建 FTS 索引，将索引内容切换为 CJK 逐字空格化文本。
///
/// 旧索引（默认 unicode61 分词器）把连续中文当作单个 token，中文关键词
/// 只能命中连续中文串开头的记录，严重漏匹配。新索引写入前对每列做逐字
/// 空格化，配合查询端对称的短语查询，可命中任意位置的连续中文串。
fn apply_fts_cjk_index_migration(connection: &Connection) -> Result<(), AppError> {
    connection.execute_batch(
        "DROP TABLE IF EXISTS clip_items_fts;
         CREATE VIRTUAL TABLE clip_items_fts USING fts5(
           item_id UNINDEXED,
           full_text,
           search_text,
           source_app
         );"
    )?;

    let mut statement = connection.prepare(
        "SELECT id, full_text, search_text, source_app FROM clip_items WHERE deleted_at IS NULL",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    for row in rows {
        let (id, full_text, search_text, source_app) = row?;
        connection.execute(
            "INSERT INTO clip_items_fts(item_id, full_text, search_text, source_app) VALUES(?1, ?2, ?3, ?4)",
            params![
                id,
                space_cjk_text(&full_text),
                space_cjk_text(&search_text),
                source_app.as_deref().map(space_cjk_text).unwrap_or_default(),
            ],
        )?;
    }
    Ok(())
}

/// v5：引入标签表，并为 FTS 增加 `tags` 列（标签名空格化拼接）后全量重建索引。
///
/// 建表语句必须带 `IF NOT EXISTS`：全新库先执行 `0001_init.sql`（已含新表）
/// 再跑增量迁移，非幂等建表会让首次启动直接失败。
fn apply_tags_migration(connection: &Connection) -> Result<(), AppError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS tags(
           name TEXT PRIMARY KEY COLLATE NOCASE,
           created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS clip_item_tags(
           item_id TEXT NOT NULL REFERENCES clip_items(id) ON DELETE CASCADE,
           tag_name TEXT NOT NULL COLLATE NOCASE REFERENCES tags(name) ON DELETE CASCADE,
           PRIMARY KEY (item_id, tag_name)
         );
         CREATE INDEX IF NOT EXISTS idx_clip_item_tags_tag ON clip_item_tags(tag_name);
         DROP TABLE IF EXISTS clip_items_fts;
         CREATE VIRTUAL TABLE clip_items_fts USING fts5(
           item_id UNINDEXED,
           full_text,
           search_text,
           source_app,
           tags
         );"
    )?;

    let mut statement = connection.prepare(
        "SELECT ci.id, ci.full_text, ci.search_text, ci.source_app,
                COALESCE((
                  SELECT GROUP_CONCAT(tag_name, char(31)) FROM (
                    SELECT tag_name FROM clip_item_tags WHERE item_id = ci.id
                    ORDER BY tag_name COLLATE NOCASE
                  )
                ), '')
         FROM clip_items ci WHERE ci.deleted_at IS NULL",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (id, full_text, search_text, source_app, tags) = row?;
        connection.execute(
            "INSERT INTO clip_items_fts(item_id, full_text, search_text, source_app, tags)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                space_cjk_text(&full_text),
                space_cjk_text(&search_text),
                source_app.as_deref().map(space_cjk_text).unwrap_or_default(),
                space_cjk_text(&tags),
            ],
        )?;
    }
    Ok(())
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
