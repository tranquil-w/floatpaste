use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
use tauri_plugin_global_shortcut::Shortcut;

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
/// - **主快捷键**: 用于打开 Picker 窗口
/// - **工作窗快捷键**: 用于全局打开搜索窗口
///
/// ## 重要: Windows 键的格式
///
/// Tauri 的 global-shortcut 插件使用跨平台的修饰符格式:
/// - **Windows**: 使用 `Super` 表示 Windows 键 (Win)
/// - **macOS**: 使用 `Super` 表示 Command 键 (Cmd)
/// - **Linux**: 使用 `Super` 表示 Super 键
///
/// ❌ **错误**: `Win+F`, `win+f`, `WIN+F`
/// ✅ **正确**: `Super+F`, `super+f`
///
/// > **原因**: Tauri 的 `Shortcut::from_str()` 不识别 "Win" 修饰符,
/// > 只识别 "Super" 作为跨平台的 Windows/Command 键表示。
///
/// # 示例
///
/// ```rust
/// use crate::domain::settings::UserSetting;
///
/// let settings = UserSetting::default();
/// assert_eq!(settings.shortcut, "Ctrl+`");
/// assert_eq!(settings.workbench_shortcut, "Super+F"); // ✅ 正确
/// // assert_eq!(settings.workbench_shortcut, "Win+F"); // ❌ 错误,无法注册
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
            workbench_shortcut: "Super+F".to_string(),
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
            self.workbench_shortcut = "Super+F".to_string();
        }

        // 迁移旧的 Win 键格式到 Super (向后兼容)
        // Tauri 的 global-shortcut 插件不支持 "Win" 修饰符,只支持 "Super"
        self.workbench_shortcut = migrate_win_to_super(&self.workbench_shortcut);

        self.resolve_workbench_shortcut_conflict();
        self
    }

    fn resolve_workbench_shortcut_conflict(&mut self) {
        if !self.workbench_shortcut_enabled
            || normalize_shortcut_for_compare(&self.workbench_shortcut)
                != normalize_shortcut_for_compare(&self.shortcut)
        {
            return;
        }

        self.workbench_shortcut = "Super+F".to_string();
        if normalize_shortcut_for_compare(&self.workbench_shortcut)
            == normalize_shortcut_for_compare(&self.shortcut)
        {
            self.workbench_shortcut_enabled = false;
        }
    }
}

fn normalize_shortcut_for_compare(shortcut: &str) -> String {
    let trimmed = shortcut.trim();
    Shortcut::from_str(trimmed)
        .map(|value| value.into_string().to_lowercase())
        .unwrap_or_else(|_| trimmed.to_lowercase())
}

/// 迁移旧的 Win 键格式到 Super 格式
///
/// Tauri 的 global-shortcut 插件使用跨平台的 "Super" 修饰符:
/// - Windows: Super = Windows 键 (Win)
/// - macOS: Super = Command 键 (Cmd)
/// - Linux: Super = Super 键
///
/// 这个函数将用户设置中的 "Win" 替换为 "Super",以支持向后兼容。
fn migrate_win_to_super(shortcut: &str) -> String {
    let lower = shortcut.to_lowercase();
    if lower.contains("win+") || lower.contains("windows+") {
        // 将 "Win" 替换为 "Super" (保留原始大小写)
        shortcut
            .replace("Win+", "Super+")
            .replace("win+", "super+")
            .replace("Windows+", "Super+")
            .replace("windows+", "super+")
    } else {
        shortcut.to_string()
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
    fn workbench_shortcut_defaults_to_super_f() {
        let settings = UserSetting::default();
        assert_eq!(settings.workbench_shortcut, "Super+F");
        assert!(settings.workbench_shortcut_enabled);
    }

    #[test]
    fn migrate_win_to_super_converts_old_format() {
        use super::migrate_win_to_super;

        assert_eq!(migrate_win_to_super("Win+F"), "Super+F");
        assert_eq!(migrate_win_to_super("win+f"), "super+f");
        assert_eq!(migrate_win_to_super("WIN+F"), "WIN+F".replace("Win", "Super"));
        assert_eq!(migrate_win_to_super("Windows+F"), "Super+F");
        assert_eq!(migrate_win_to_super("windows+f"), "super+f");
        assert_eq!(migrate_win_to_super("Super+F"), "Super+F");
        assert_eq!(migrate_win_to_super("Ctrl+F"), "Ctrl+F");
    }

    #[test]
    fn sanitized_migrates_win_to_super() {
        let settings = UserSetting {
            workbench_shortcut: "Win+F".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.workbench_shortcut, "Super+F");
    }

    #[test]
    fn sanitized_preserves_super_format() {
        let settings = UserSetting {
            workbench_shortcut: "Super+F".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.workbench_shortcut, "Super+F");
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

        assert_eq!(settings.workbench_shortcut, "Super+F");
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

        assert_eq!(settings.workbench_shortcut, "Super+F");
    }

    #[test]
    fn workbench_shortcut_gets_disabled_when_default_value_also_conflicts_with_main_shortcut() {
        let settings = UserSetting {
            shortcut: "Super+F".to_string(),
            workbench_shortcut: "Super+F".to_string(),
            workbench_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert!(!settings.workbench_shortcut_enabled);
    }

    #[test]
    fn workbench_shortcut_conflict_detection_uses_normalized_shortcuts() {
        let settings = UserSetting {
            shortcut: "SUPER+F".to_string(),
            workbench_shortcut: "super+f".to_string(),
            workbench_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert!(!settings.workbench_shortcut_enabled);
    }
}
