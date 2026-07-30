use rusqlite::Connection;

use crate::domain::error::AppError;

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
