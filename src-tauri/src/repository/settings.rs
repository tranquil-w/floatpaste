use rusqlite::{params, OptionalExtension};

use crate::domain::{
    error::AppError,
    settings::{StoredWindowPosition, UserSetting},
};

use super::sqlite_repository::SqliteRepository;

const USER_SETTINGS_KEY: &str = "user_settings";
// 继续复用旧 key，兼容已落盘的“仅位置”状态。
pub(super) const PICKER_WINDOW_STATE_KEY: &str = "picker_last_position";

impl SqliteRepository {
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
}
