use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::domain::clip_item::TextKind;
use crate::services::image_storage::ImageStorage;
use crate::services::text_identity::text_content_hash;
use crate::services::text_kind::detect_text_kind;
use crate::{domain::error::AppError, services::normalize_service::space_cjk_text};

const CURRENT_SCHEMA_VERSION: i32 = 11;
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
/// `db_path` 用于推导图片目录（与运行时 ImageStorage 同为数据库所在目录），
/// 供迁移重算存量图片条目的内容身份。
///
/// 兼容历史上未记录 `user_version` 的已有数据库：当检测到 media 列缺失但
/// `user_version` 已是最新时，仍会补齐 media 列迁移。
pub(super) fn initialize_database(connection: &Connection, db_path: &Path) -> Result<(), AppError> {
    // 0001 含 PRAGMA journal_mode 赋值，不能包进事务；幂等靠 IF NOT EXISTS
    connection.execute_batch(include_str!("../../migrations/0001_init.sql"))?;

    let schema_version = current_schema_version(connection)?;
    if schema_version < CURRENT_SCHEMA_VERSION {
        apply_schema_upgrades(connection, schema_version, db_path)?;
    } else if !clip_items_has_media_columns(connection)? {
        // 补列迁移同样整体原子：中途崩溃不会留下"列已加但版本未写"的半迁移状态
        let transaction = connection.unchecked_transaction()?;
        apply_media_columns_migration(&transaction)?;
        set_schema_version(&transaction, CURRENT_SCHEMA_VERSION)?;
        transaction.commit()?;
    }

    Ok(())
}

fn apply_schema_upgrades(
    connection: &Connection,
    current_version: i32,
    db_path: &Path,
) -> Result<(), AppError> {
    // 全部增量迁移在一个事务内原子完成：表结构与 user_version 要么都生效要么都不生效，
    // 避免重建 FTS 等多步骤迁移中途崩溃后留下半迁移库
    let transaction = connection.unchecked_transaction()?;

    if current_version < 2 {
        apply_media_columns_migration(&transaction)?;
    }

    if current_version < 3 {
        apply_directory_count_migration(&transaction)?;
    }

    if current_version < 4 {
        apply_fts_cjk_index_migration(&transaction)?;
    }

    if current_version < 5 {
        apply_tags_migration(&transaction)?;
    }

    if current_version < 7 {
        apply_text_kind_migration(&transaction)?;
    }

    if current_version < 8 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS clip_text_formats(
            item_id TEXT PRIMARY KEY REFERENCES clip_items(id) ON DELETE CASCADE,
            html_bytes BLOB NOT NULL
        );",
        )?;
        let mut statement = transaction.prepare("SELECT ci.id, ci.full_text, f.html_bytes FROM clip_items ci LEFT JOIN clip_text_formats f ON f.item_id = ci.id WHERE ci.type = 'text'")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, text, html) = row?;
            transaction.execute(
                "UPDATE clip_items SET hash = ?2 WHERE id = ?1",
                params![
                    id,
                    crate::services::text_identity::text_formats_hash(&text, html.as_deref())
                ],
            )?;
        }
    }

    if current_version < 9 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_image_deletions (image_path TEXT PRIMARY KEY);",
        )?;
    }

    // v7 已按运行时函数重算摘要，更旧版本升级到这里无需再刷一遍
    if current_version >= 7 && current_version < 10 {
        apply_preview_refresh_migration(&transaction)?;
    }

    if current_version < 11 {
        apply_image_hash_migration(&transaction, db_path)?;
    }

    set_schema_version(&transaction, CURRENT_SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

/// v11：图片条目身份改为像素内容（v10 及之前把 file_size 等编码元数据混入
/// hash，目标应用粘贴后以自己的 PNG/DIB 编码回写剪贴板时，同一图片会因
/// 编码字节不同被判为新内容、重复入库）。逐行读取保存的 PNG 重算 hash；
/// 单行失败保留原 hash 并继续——文件已缺失或损坏的条目无法还原原始像素，
/// 不能因此让整个迁移失败阻塞启动。不合并历史条目，软删行一并刷新。
/// 当前全量载入待更新行；数据量大时再引入分批处理。
fn apply_image_hash_migration(connection: &Connection, db_path: &Path) -> Result<(), AppError> {
    let image_rows: Vec<(String, String)> = {
        let mut statement = connection.prepare(
            "SELECT id, image_path FROM clip_items WHERE type = 'image' AND image_path IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    // 无图片行时直接跳过：ImageStorage 构造会创建 images 目录，
    // 内存库或相对路径库（如测试）不能因此在工作目录落下空目录
    if image_rows.is_empty() {
        return Ok(());
    }
    let data_dir = db_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let image_storage = ImageStorage::new(data_dir)?;
    for (id, image_path) in image_rows {
        let Some(hash) = image_storage.content_hash_of_stored_image(&image_path) else {
            tracing::warn!(%id, %image_path, "迁移 v11：读取图片失败，保留原 hash");
            continue;
        };
        connection.execute(
            "UPDATE clip_items SET hash = ?2 WHERE id = ?1",
            params![id, hash],
        )?;
    }
    Ok(())
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
         );",
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
         );",
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
                source_app
                    .as_deref()
                    .map(space_cjk_text)
                    .unwrap_or_default(),
                space_cjk_text(&tags),
            ],
        )?;
    }
    Ok(())
}

/// v6 引入文本用途与原文身份；v7 按校验后的用途规则重建用途与摘要。
/// 从任意旧版本升级只扫描一次；已有 v6 数据也需刷新派生字段。
///
/// - 用途在 Rust 侧按保守规则从保存的原文识别，图片与文件行保持默认值；
/// - hash 按原文 UTF-8 字节重算，不改 ID、标签与收藏，也不合并历史条目
///   （此前被空白归一化合并掉的原文差异无法还原）；
/// - 重算在事务内进行，hash 索引非唯一，同原文的软删行与存活行各自更新。
/// - 简化: 全量载入待更新行；数据量大时再引入分批处理。
fn apply_text_kind_migration(connection: &Connection) -> Result<(), AppError> {
    // 建列需幂等：全新库先执行 0001_init.sql（已含该列）再跑增量迁移
    if !column_exists(connection, "clip_items", "text_kind")? {
        connection.execute(
            "ALTER TABLE clip_items ADD COLUMN text_kind TEXT NOT NULL DEFAULT 'plain'",
            [],
        )?;
    }

    let updates: Vec<(String, String, String, String)> = {
        let mut statement =
            connection.prepare("SELECT id, full_text FROM clip_items WHERE type = 'text'")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut updates = Vec::new();
        for row in rows {
            let (id, full_text) = row?;
            let kind = detect_text_kind(&full_text);
            updates.push((
                id,
                kind.as_str().to_string(),
                text_content_hash(&full_text),
                crate::services::clip_display::text_preview(&full_text, kind),
            ));
        }
        updates
    };

    for (id, kind, hash, preview) in updates {
        connection.execute(
            "UPDATE clip_items SET text_kind = ?2, hash = ?3, preview_text = ?4 WHERE id = ?1",
            params![id, kind, hash, preview],
        )?;
    }
    Ok(())
}

/// v10：列表摘要改为原文直显（链接去 userinfo、路径首条原样），按行内
/// 记录的用途重算全部文本行的 `preview_text`，与徽章、筛选取自同一列。
/// 识别规则与内容身份未变，不动 text_kind、hash 与 FTS；软删行一并刷新，
/// 保留期内恢复显示口径一致。仅对 v7–v9 库执行：更旧版本由 v7 迁移用同
/// 一个运行时函数写入摘要。
/// - 简化: 全量载入待更新行；数据量大时再引入分批处理。
fn apply_preview_refresh_migration(connection: &Connection) -> Result<(), AppError> {
    let updates: Vec<(String, String)> = {
        let mut statement = connection
            .prepare("SELECT id, full_text, text_kind FROM clip_items WHERE type = 'text'")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut updates = Vec::new();
        for row in rows {
            let (id, full_text, kind) = row?;
            updates.push((
                id,
                crate::services::clip_display::text_preview(&full_text, TextKind::parse(&kind)),
            ));
        }
        updates
    };

    for (id, preview) in updates {
        connection.execute(
            "UPDATE clip_items SET preview_text = ?2 WHERE id = ?1",
            params![id, preview],
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

#[cfg(test)]
mod tests {
    use super::CURRENT_SCHEMA_VERSION;
    use crate::{
        repository::sqlite_repository::SqliteRepository, services::image_storage::ImageStorage,
    };

    /// v11：存量图片行按保存的 PNG 像素重算 hash（此前 file_size 等编码元
    /// 数据混入身份，目标应用以不同编码回写同一图片会重复入库）；
    /// 文件缺失或损坏的行保留原 hash，软删行一并刷新。
    #[test]
    fn v11_migration_recomputes_image_hash_from_stored_png() {
        let data_dir = std::env::temp_dir().join(format!(
            "floatpaste-v11-migration-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let image_storage = ImageStorage::new(data_dir.clone()).unwrap();
        let mut rgba = Vec::with_capacity(8 * 8 * 4);
        for _ in 0..(8 * 8) {
            rgba.extend_from_slice(&[200, 30, 40, 255]);
        }
        let prepared = image_storage.prepare_image(&rgba, 8, 8, None).unwrap();
        let stored = image_storage.store_prepared_image(&prepared).unwrap();

        let db_path = data_dir.join("floatpaste.db");
        {
            let connection = rusqlite::Connection::open(&db_path).unwrap();
            connection
                .execute_batch(include_str!("../../migrations/0001_init.sql"))
                .unwrap();
            connection.pragma_update(None, "user_version", 10).unwrap();
            connection
                .execute_batch(&format!(
                    "INSERT INTO clip_items(id, type, full_text, preview_text, search_text, hash, created_at, updated_at, image_path, image_width, image_height, image_format, file_size)
                     VALUES
                       ('img-live',  'image', '', '', '', 'legacy-live',  1, 1, '{stored}', 8, 8, 'png', 1234),
                       ('img-del',   'image', '', '', '', 'legacy-del',   2, 2, '{stored}', 8, 8, 'png', 1234),
                       ('img-rown',  'image', '', '', '', 'legacy-rown',  3, 3, 'images/missing.png', 8, 8, 'png', 1234);
                    UPDATE clip_items SET deleted_at = 99 WHERE id = 'img-del';",
                    stored = stored.image_path
                ))
                .unwrap();
        }

        let repository = SqliteRepository::new(&db_path).unwrap();
        let read_hash = |repository: &SqliteRepository, id: &str| {
            repository
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT hash FROM clip_items WHERE id = ?1", [id], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap()
        };

        assert_eq!(read_hash(&repository, "img-live"), prepared.content_hash);
        assert_eq!(read_hash(&repository, "img-del"), prepared.content_hash);
        // 文件缺失的行无法还原像素，保留原 hash，不阻断迁移
        assert_eq!(read_hash(&repository, "img-rown"), "legacy-rown");

        let version: i32 = repository
            .connection
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);

        let _ = std::fs::remove_dir_all(data_dir);
    }
}
