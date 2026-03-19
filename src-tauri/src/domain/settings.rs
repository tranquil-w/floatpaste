use serde::{Deserialize, Deserializer, Serialize};

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
    pub workbench_shortcut: String,
    pub workbench_shortcut_enabled: bool,
}

impl Default for UserSetting {
    fn default() -> Self {
        Self {
            shortcut: "Ctrl+`".to_string(),
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
            workbench_shortcut: "Ctrl+Shift+F".to_string(),
            workbench_shortcut_enabled: true,
        }
    }
}

impl UserSetting {
    pub fn sanitized(mut self) -> Self {
        self.shortcut = self.shortcut.trim().to_string();
        if self.shortcut.is_empty() {
            self.shortcut = "Ctrl+`".to_string();
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

        self.workbench_shortcut = self.workbench_shortcut.trim().to_string();
        if self.workbench_shortcut.is_empty() {
            self.workbench_shortcut = "Ctrl+Shift+F".to_string();
        }
        if self.workbench_shortcut_enabled && self.workbench_shortcut == self.shortcut {
            self.workbench_shortcut = "Ctrl+Shift+F".to_string();
            // 如果默认值与主快捷键也相同，则禁用工作窗快捷键
            if self.workbench_shortcut == self.shortcut {
                self.workbench_shortcut_enabled = false;
            }
        }

        self
    }
}

#[cfg(test)]
mod tests {
    use super::{PickerPositionMode, ThemeMode, UserSetting};

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
    fn deserialize_old_settings_without_silent_on_startup_field() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Ctrl+`",
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
                "shortcut":"Ctrl+`",
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
                "shortcut":"Ctrl+`",
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
                "shortcut":"Ctrl+`",
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
                "shortcut":"Ctrl+`",
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
    fn workbench_shortcut_defaults_to_ctrl_shift_f() {
        let settings = UserSetting::default();
        assert_eq!(settings.workbench_shortcut, "Ctrl+Shift+F");
        assert!(settings.workbench_shortcut_enabled);
    }

    #[test]
    fn deserialize_old_settings_without_workbench_shortcut_uses_defaults() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Ctrl+`",
                "launchOnStartup":false,
                "historyLimit":1000,
                "pickerRecordLimit":50,
                "excludedApps":[],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false
            }"#,
        )
        .unwrap();
        assert_eq!(settings.workbench_shortcut, "Ctrl+Shift+F");
        assert!(settings.workbench_shortcut_enabled);
    }

    #[test]
    fn workbench_shortcut_resets_to_default_when_conflicts_with_main_shortcut() {
        let settings = UserSetting {
            shortcut: "Ctrl+`".to_string(),
            workbench_shortcut: "Ctrl+`".to_string(),
            workbench_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();
        // 冲突时应重置为默认值，而不是报错
        assert_eq!(settings.workbench_shortcut, "Ctrl+Shift+F");
    }
}
