use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
use tauri_plugin_global_shortcut::Shortcut;

const DEFAULT_MAIN_SHORTCUT: &str = "Alt+Q";
const DEFAULT_SEARCH_SHORTCUT: &str = "Alt+S";
const LEGACY_MAIN_SHORTCUT: &str = "Ctrl+`";
const LEGACY_SEARCH_SHORTCUTS: [&str; 2] = ["Win+F", "Super+F"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PickerPositionMode {
    Mouse,
    LastPosition,
    Caret,
}

impl Default for PickerPositionMode {
    fn default() -> Self {
        Self::Mouse
    }
}

impl<'de> Deserialize<'de> for PickerPositionMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        Ok(match value.as_str() {
            "lastPosition" => Self::LastPosition,
            "caret" => Self::Caret,
            _ => Self::Mouse,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl Default for ThemeMode {
    fn default() -> Self {
        Self::System
    }
}

impl<'de> Deserialize<'de> for ThemeMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        Ok(match value.as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredWindowPosition {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// 用户设置结构
///
/// # 快捷键格式说明
///
/// - **主快捷键**：用于打开 Picker 窗口
/// - **搜索窗口快捷键**：用于全局打开搜索窗口
///
/// # 示例
///
/// ```ignore
/// use crate::domain::settings::UserSetting;
///
/// let settings = UserSetting::default();
/// assert_eq!(settings.shortcut, "Alt+Q");
/// assert_eq!(settings.search_shortcut, "Alt+S");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct UserSetting {
    pub shortcut: String,
    pub launch_on_startup: bool,
    pub silent_on_startup: bool,
    pub history_limit: u32,
    pub picker_record_limit: u32,
    pub picker_position_mode: PickerPositionMode,
    pub excluded_apps: Vec<String>,
    pub restore_clipboard_after_paste: bool,
    pub pause_monitoring: bool,
    pub theme_mode: ThemeMode,
    #[serde(alias = "workbench_shortcut")]
    pub search_shortcut: String,
    #[serde(alias = "workbench_shortcut_enabled")]
    pub search_shortcut_enabled: bool,
}

impl Default for UserSetting {
    fn default() -> Self {
        Self {
            shortcut: DEFAULT_MAIN_SHORTCUT.to_string(),
            launch_on_startup: false,
            silent_on_startup: false,
            history_limit: 1_000,
            picker_record_limit: 50,
            picker_position_mode: PickerPositionMode::Mouse,
            excluded_apps: vec![
                "KeePass.exe".to_string(),
                "Bitwarden.exe".to_string(),
                "WindowsTerminal.exe".to_string(),
            ],
            restore_clipboard_after_paste: true,
            pause_monitoring: false,
            theme_mode: ThemeMode::System,
            search_shortcut: DEFAULT_SEARCH_SHORTCUT.to_string(),
            search_shortcut_enabled: true,
        }
    }
}

impl UserSetting {
    pub fn sanitized(mut self) -> Self {
        self.shortcut = self.shortcut.trim().to_string();
        if self.shortcut.is_empty() {
            self.shortcut = DEFAULT_MAIN_SHORTCUT.to_string();
        } else if normalize_shortcut_for_compare(&self.shortcut)
            == normalize_shortcut_for_compare(LEGACY_MAIN_SHORTCUT)
        {
            self.shortcut = DEFAULT_MAIN_SHORTCUT.to_string();
        }

        if !self.launch_on_startup {
            self.silent_on_startup = false;
        }

        self.history_limit = self.history_limit.clamp(100, 10_000);
        self.picker_record_limit = self.picker_record_limit.clamp(9, 1_000);
        self.excluded_apps = self
            .excluded_apps
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();

        self.search_shortcut = self.search_shortcut.trim().to_string();
        if self.search_shortcut.is_empty() {
            self.search_shortcut = DEFAULT_SEARCH_SHORTCUT.to_string();
        } else if LEGACY_SEARCH_SHORTCUTS.iter().any(|legacy| {
            normalize_shortcut_for_compare(&self.search_shortcut)
                == normalize_shortcut_for_compare(legacy)
        }) {
            self.search_shortcut = DEFAULT_SEARCH_SHORTCUT.to_string();
        }

        self.resolve_search_shortcut_conflict();
        self
    }

    fn resolve_search_shortcut_conflict(&mut self) {
        if !self.search_shortcut_enabled
            || normalize_shortcut_for_compare(&self.search_shortcut)
                != normalize_shortcut_for_compare(&self.shortcut)
        {
            return;
        }

        self.search_shortcut = DEFAULT_SEARCH_SHORTCUT.to_string();
        if normalize_shortcut_for_compare(&self.search_shortcut)
            == normalize_shortcut_for_compare(&self.shortcut)
        {
            self.search_shortcut_enabled = false;
        }
    }
}

fn normalize_shortcut_for_compare(shortcut: &str) -> String {
    let trimmed = shortcut.trim();
    let registerable = normalize_shortcut_for_registration(trimmed);
    Shortcut::from_str(&registerable)
        .map(|value| value.into_string().to_lowercase())
        .unwrap_or_else(|_| registerable.to_lowercase())
}

pub(crate) fn normalize_shortcut_for_registration(shortcut: &str) -> String {
    shortcut
        .trim()
        .split('+')
        .map(|token| {
            let trimmed = token.trim();
            if trimmed.eq_ignore_ascii_case("win") || trimmed.eq_ignore_ascii_case("windows") {
                "Super".to_string()
            } else {
                trimmed.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::{
        PickerPositionMode, ThemeMode, UserSetting, normalize_shortcut_for_compare,
    };

    #[test]
    fn sanitized_turns_off_silent_when_launch_on_startup_is_disabled() {
        let settings = UserSetting {
            launch_on_startup: false,
            silent_on_startup: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert!(!settings.silent_on_startup);
    }

    #[test]
    fn shortcut_defaults_to_alt_q() {
        let settings = UserSetting::default();
        assert_eq!(settings.shortcut, "Alt+Q");
    }

    #[test]
    fn deserialize_old_settings_without_silent_on_startup_field() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":true,
                "historyLimit":1000,
                "excludedApps":["KeePass.exe"],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false
            }"#,
        )
        .unwrap();

        assert!(settings.launch_on_startup);
        assert!(!settings.silent_on_startup);
        assert_eq!(settings.picker_record_limit, 50);
        assert_eq!(settings.picker_position_mode, PickerPositionMode::Mouse);
        assert_eq!(settings.theme_mode, ThemeMode::System);
    }

    #[test]
    fn deserialize_picker_position_mode() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":false,
                "silentOnStartup":false,
                "historyLimit":1000,
                "pickerRecordLimit":50,
                "pickerPositionMode":"lastPosition",
                "excludedApps":["KeePass.exe"],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false
            }"#,
        )
        .unwrap();

        assert_eq!(
            settings.picker_position_mode,
            PickerPositionMode::LastPosition
        );
    }

    #[test]
    fn deserialize_invalid_picker_position_mode_falls_back_to_mouse() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":false,
                "silentOnStartup":false,
                "historyLimit":1000,
                "pickerRecordLimit":50,
                "pickerPositionMode":"somewhereElse",
                "excludedApps":["KeePass.exe"],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false
            }"#,
        )
        .unwrap();

        assert_eq!(settings.picker_position_mode, PickerPositionMode::Mouse);
    }

    #[test]
    fn deserialize_theme_mode() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":false,
                "silentOnStartup":false,
                "historyLimit":1000,
                "pickerRecordLimit":50,
                "pickerPositionMode":"mouse",
                "excludedApps":["KeePass.exe"],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false,
                "themeMode":"dark"
            }"#,
        )
        .unwrap();

        assert_eq!(settings.theme_mode, ThemeMode::Dark);
    }

    #[test]
    fn deserialize_invalid_theme_mode_falls_back_to_system() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":false,
                "silentOnStartup":false,
                "historyLimit":1000,
                "pickerRecordLimit":50,
                "pickerPositionMode":"mouse",
                "excludedApps":["KeePass.exe"],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false,
                "themeMode":"sepia"
            }"#,
        )
        .unwrap();

        assert_eq!(settings.theme_mode, ThemeMode::System);
    }

    #[test]
    fn search_shortcut_defaults_to_alt_s() {
        let settings = UserSetting::default();
        assert_eq!(settings.search_shortcut, "Alt+S");
        assert!(settings.search_shortcut_enabled);
    }

    #[test]
    fn normalize_shortcut_for_registration_uses_super_alias() {
        use super::normalize_shortcut_for_registration;

        assert_eq!(normalize_shortcut_for_registration("Win+F"), "Super+F");
        assert_eq!(normalize_shortcut_for_registration("win+f"), "Super+f");
        assert_eq!(normalize_shortcut_for_registration("Windows+Shift+F"), "Super+Shift+F");
        assert_eq!(normalize_shortcut_for_registration("Super+F"), "Super+F");
        assert_eq!(normalize_shortcut_for_registration("Ctrl+F"), "Ctrl+F");
    }

    #[test]
    fn sanitized_migrates_legacy_ctrl_backtick_to_alt_q() {
        let settings = UserSetting {
            shortcut: "Ctrl+`".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.shortcut, "Alt+Q");
    }

    #[test]
    fn sanitized_preserves_alt_s_as_display_value() {
        let settings = UserSetting {
            search_shortcut: "Alt+S".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.search_shortcut, "Alt+S");
    }

    #[test]
    fn sanitized_migrates_legacy_win_f_to_alt_s() {
        let settings = UserSetting {
            search_shortcut: "Win+F".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.search_shortcut, "Alt+S");
    }

    #[test]
    fn sanitized_migrates_legacy_super_f_to_alt_s() {
        let settings = UserSetting {
            search_shortcut: "Super+F".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.search_shortcut, "Alt+S");
    }

    #[test]
    fn win_and_super_shortcuts_are_treated_as_the_same_combination() {
        assert_eq!(
            normalize_shortcut_for_compare("Win+F"),
            normalize_shortcut_for_compare("Super+F")
        );
    }


    #[test]
    fn deserialize_old_settings_without_search_shortcut_uses_defaults() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":false,
                "historyLimit":1000,
                "pickerRecordLimit":50,
                "excludedApps":[],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false
            }"#,
        )
        .unwrap();

        assert_eq!(settings.search_shortcut, "Alt+S");
        assert!(settings.search_shortcut_enabled);
    }

    #[test]
    fn search_shortcut_resets_to_default_when_conflicts_with_main_shortcut() {
        let settings = UserSetting {
            shortcut: "Alt+Q".to_string(),
            search_shortcut: "Alt+Q".to_string(),
            search_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.search_shortcut, "Alt+S");
    }

    #[test]
    fn search_shortcut_gets_disabled_when_default_value_also_conflicts_with_main_shortcut() {
        let settings = UserSetting {
            shortcut: "Alt+S".to_string(),
            search_shortcut: "Alt+S".to_string(),
            search_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert!(!settings.search_shortcut_enabled);
    }

    #[test]
    fn search_shortcut_conflict_detection_uses_normalized_shortcuts() {
        let settings = UserSetting {
            shortcut: "ALT+S".to_string(),
            search_shortcut: "alt+s".to_string(),
            search_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert!(!settings.search_shortcut_enabled);
    }
}
