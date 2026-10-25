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
            .as_deref()
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

        let previous_version = (
            setting.session_keys_version,
            setting.global_shortcuts_version,
        );
        let setting = setting.sanitized();
        if settings_json.is_some()
            && (
                setting.session_keys_version,
                setting.global_shortcuts_version,
            ) != previous_version
        {
            connection.execute(
                "UPDATE settings SET value = ?1 WHERE key = ?2",
                params![serde_json::to_string(&setting)?, USER_SETTINGS_KEY],
            )?;
        }
        Ok(setting)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::SessionKeys;

    #[test]
    fn legacy_shortcuts_are_migrated_and_persisted_without_overwriting_later_edits() {
        let repository = SqliteRepository::new(std::path::Path::new(":memory:")).unwrap();
        let legacy = SessionKeys {
            confirm_as_file: "Shift+Enter".into(),
            open_editor: "Ctrl+Enter".into(),
            toggle_favorite: "Ctrl+Space".into(),
            ..SessionKeys::default()
        };
        let mut raw = serde_json::to_value(UserSetting {
            shortcut: "Ctrl+Q".into(),
            search_shortcut: "Alt+S".into(),
            session_keys: legacy.clone(),
            ..UserSetting::default()
        })
        .unwrap();
        raw.as_object_mut().unwrap().remove("sessionKeysVersion");
        raw.as_object_mut()
            .unwrap()
            .remove("globalShortcutsVersion");
        repository
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2)",
                params![USER_SETTINGS_KEY, raw.to_string()],
            )
            .unwrap();

        let mut loaded = repository.load_settings().unwrap();
        assert_eq!(loaded.session_keys, SessionKeys::default());
        assert_eq!(loaded.shortcut, "Alt+`");
        assert_eq!(loaded.search_shortcut, "Alt+Q");
        let persisted: String = repository
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [USER_SETTINGS_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<UserSetting>(&persisted)
                .unwrap()
                .session_keys_version,
            loaded.session_keys_version
        );
        assert_eq!(
            serde_json::from_str::<UserSetting>(&persisted)
                .unwrap()
                .global_shortcuts_version,
            loaded.global_shortcuts_version
        );

        // 升级后用户主动恢复整套旧键位，保存和后续加载均不得再次迁移。
        loaded.session_keys = legacy.clone();
        loaded.shortcut = "Ctrl+Q".into();
        loaded.search_shortcut = "Alt+S".into();
        repository.save_settings(&loaded.sanitized()).unwrap();
        let reloaded = repository.load_settings().unwrap();
        assert_eq!(reloaded.session_keys, legacy);
        assert_eq!(reloaded.shortcut, "Ctrl+Q");
        assert_eq!(reloaded.search_shortcut, "Alt+S");
    }
}
