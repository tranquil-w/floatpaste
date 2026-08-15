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
  deleted_at INTEGER NULL,
  image_path TEXT NULL,
  image_width INTEGER NULL,
  image_height INTEGER NULL,
  image_format TEXT NULL,
  file_size INTEGER NULL,
  file_paths TEXT NOT NULL DEFAULT '[]',
  file_count INTEGER NOT NULL DEFAULT 0,
  total_size INTEGER NULL,
  directory_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_clip_items_created_at ON clip_items(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_clip_items_last_used_at ON clip_items(last_used_at DESC);
CREATE INDEX IF NOT EXISTS idx_clip_items_hash ON clip_items(hash);
CREATE INDEX IF NOT EXISTS idx_clip_items_source_app ON clip_items(source_app);

CREATE VIRTUAL TABLE IF NOT EXISTS clip_items_fts USING fts5(
  item_id UNINDEXED,
  full_text,
  search_text,
  source_app,
  tags
);

CREATE TABLE IF NOT EXISTS tags (
  name TEXT PRIMARY KEY COLLATE NOCASE,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS clip_item_tags (
  item_id TEXT NOT NULL REFERENCES clip_items(id) ON DELETE CASCADE,
  tag_name TEXT NOT NULL COLLATE NOCASE REFERENCES tags(name) ON DELETE CASCADE,
  PRIMARY KEY (item_id, tag_name)
);

CREATE INDEX IF NOT EXISTS idx_clip_item_tags_tag ON clip_item_tags(tag_name);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS excluded_apps (
  executable_name TEXT PRIMARY KEY
);
