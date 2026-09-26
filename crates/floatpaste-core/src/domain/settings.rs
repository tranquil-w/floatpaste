use serde::{Deserialize, Deserializer, Serialize};

const DEFAULT_MAIN_SHORTCUT: &str = "Ctrl+Q";
const DEFAULT_SEARCH_SHORTCUT: &str = "Alt+S";
/// 历史默认值：sanitized 时迁移到当前默认，未改过快捷键的用户无感升级
const LEGACY_MAIN_SHORTCUTS: [&str; 2] = ["Ctrl+`", "Alt+Q"];
const LEGACY_SEARCH_SHORTCUTS: [&str; 2] = ["Win+F", "Super+F"];
const DEFAULT_THEME_PRESET: &str = "default";
const DEFAULT_THEME_ACCENT: &str = "default";
/// 旧版自定义颜色的默认强调色；迁移时仅保留与默认不同的自定义值
const LEGACY_DEFAULT_LIGHT_ACCENT: &str = "#0969DA";
const LEGACY_DEFAULT_DARK_ACCENT: &str = "#478BE6";

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

/// 速贴/搜索条目的鼠标上屏触发方式
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PasteTrigger {
    /// 单击条目立即上屏（默认）
    Click,
    /// 单击选中，双击上屏
    DoubleClick,
}

impl Default for PasteTrigger {
    fn default() -> Self {
        Self::Click
    }
}

impl<'de> Deserialize<'de> for PasteTrigger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        Ok(match value.as_str() {
            "doubleClick" => Self::DoubleClick,
            _ => Self::Click,
        })
    }
}

/// 速贴/搜索会话期的动作键位（数字 1-9 直达除外，见
/// `picker_digit_shortcuts_enabled`）。值为「修饰键+键名」串（如
/// "Enter"、"Shift+Enter"、"Ctrl+Space"），键名与修饰键大小写不敏感；
/// 非法或缺失的值按字段回退默认，见 [SessionKeys::sanitized]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionKeys {
    /// 上屏
    pub confirm: String,
    /// 次级上屏：粘贴为图片路径 / 文件路径（按条目类型）。字段名是
    /// 持久化配置键，不可更名
    pub confirm_as_file: String,
    /// 打开编辑器
    pub open_editor: String,
    /// 收藏/取消收藏
    pub toggle_favorite: String,
    /// 关闭会话
    pub dismiss: String,
    /// 向上选择
    pub navigate_up: String,
    /// 向下选择
    pub navigate_down: String,
    /// 删除条目（仅搜索窗口）
    pub delete_entry: String,
}

impl Default for SessionKeys {
    fn default() -> Self {
        Self {
            confirm: "Enter".to_string(),
            confirm_as_file: "Shift+Enter".to_string(),
            open_editor: "Ctrl+Enter".to_string(),
            toggle_favorite: "Ctrl+Space".to_string(),
            dismiss: "Escape".to_string(),
            navigate_up: "Up".to_string(),
            navigate_down: "Down".to_string(),
            delete_entry: "Delete".to_string(),
        }
    }
}

impl SessionKeys {
    /// 动作序号 → 字段的统一读写入口（设置界面按行模型存取用）
    pub fn field(&self, action: usize) -> &str {
        match action {
            0 => &self.confirm,
            1 => &self.confirm_as_file,
            2 => &self.open_editor,
            3 => &self.toggle_favorite,
            4 => &self.dismiss,
            5 => &self.navigate_up,
            6 => &self.navigate_down,
            7 => &self.delete_entry,
            _ => "",
        }
    }

    pub fn set_field(&mut self, action: usize, value: String) {
        match action {
            0 => self.confirm = value,
            1 => self.confirm_as_file = value,
            2 => self.open_editor = value,
            3 => self.toggle_favorite = value,
            4 => self.dismiss = value,
            5 => self.navigate_up = value,
            6 => self.navigate_down = value,
            7 => self.delete_entry = value,
            _ => {}
        }
    }

    pub fn sanitized(mut self) -> Self {
        let default = Self::default();
        for action in 0..8 {
            let trimmed = self.field(action).trim().to_string();
            let value = if trimmed.is_empty() {
                default.field(action).to_string()
            } else {
                trimmed
            };
            self.set_field(action, value);
        }
        self
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

/// 旧版自定义三色（窗口底/卡片底/强调色）。
/// 仅用于反序列化旧配置文件以迁移强调色，字段不再写回磁盘；
/// 底色一律由主题预设接管，不再持久化。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColorPalette {
    pub window_bg: String,
    pub card_bg: String,
    pub accent: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomThemeColors {
    #[serde(default)]
    pub light: Option<ThemeColorPalette>,
    #[serde(default)]
    pub dark: Option<ThemeColorPalette>,
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
/// assert_eq!(settings.shortcut, "Ctrl+Q");
/// assert_eq!(settings.search_shortcut, "Alt+S");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct UserSetting {
    pub shortcut: String,
    pub launch_on_startup: bool,
    pub silent_on_startup: bool,
    /// 以管理员权限启动：登录自启经任务计划程序「最高权限」任务接管
    /// （Run 键无法提权；asInvoker manifest 不触发 UAC）；其余启动路径
    /// （手动双击图标等）由壳层启动期自检经 UAC 重入自身提权，UAC 取消
    /// 则本次普通权限运行并托盘气泡说明。任务注册/卸载需要 UAC 确认，
    /// 由壳层经 runas 重入自身完成
    #[serde(default)]
    pub always_run_elevated: bool,
    pub history_limit: u32,
    pub picker_record_limit: u32,
    pub picker_position_mode: PickerPositionMode,
    /// 速贴/搜索条目的鼠标上屏触发方式，见 [PasteTrigger]
    pub paste_trigger: PasteTrigger,
    pub excluded_apps: Vec<String>,
    pub restore_clipboard_after_paste: bool,
    pub pause_monitoring: bool,
    pub theme_mode: ThemeMode,
    #[serde(alias = "workbench_shortcut")]
    pub search_shortcut: String,
    #[serde(alias = "workbench_shortcut_enabled")]
    pub search_shortcut_enabled: bool,
    /// Picker 会话期间的数字键 1-9 直达选择。数字键是无修饰全局热键，
    /// 与其他应用的快捷键冲突面大，允许用户关闭后仅保留方向/回车/Escape 等核心键。
    #[serde(default = "default_true")]
    pub picker_digit_shortcuts_enabled: bool,
    /// 接管系统 Win+V（docs/adr-0001）：写入 DisabledHotkeys 释放组合后
    /// 由本应用注册为速贴唤起；需重启资源管理器生效，关闭开关即回退
    #[serde(default)]
    pub takeover_winv: bool,
    /// 会话期动作键位（上屏/编辑/收藏/关闭/导航/删除），见 [SessionKeys]
    #[serde(default)]
    pub session_keys: SessionKeys,
    #[serde(default = "default_theme_preset")]
    pub theme_preset: String,
    /// "default"=跟随预设 | 安全列表 id | 旧版迁移保留的 #RRGGBB；
    /// 合法性由前端派生层回退兜底，这里只做存储
    #[serde(default = "default_theme_accent")]
    pub theme_accent: String,
    /// 旧版自定义三色，仅反序列化旧配置时读取，见 [CustomThemeColors]
    #[serde(default, skip_serializing)]
    pub custom_theme_colors: CustomThemeColors,
}

impl Default for UserSetting {
    fn default() -> Self {
        Self {
            shortcut: DEFAULT_MAIN_SHORTCUT.to_string(),
            launch_on_startup: false,
            silent_on_startup: false,
            always_run_elevated: false,
            history_limit: 1_000,
            picker_record_limit: 50,
            picker_position_mode: PickerPositionMode::Mouse,
            paste_trigger: PasteTrigger::Click,
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
            picker_digit_shortcuts_enabled: true,
            takeover_winv: false,
            session_keys: SessionKeys::default(),
            theme_preset: default_theme_preset(),
            theme_accent: default_theme_accent(),
            custom_theme_colors: CustomThemeColors::default(),
        }
    }
}

impl UserSetting {
    pub fn sanitized(mut self) -> Self {
        self.shortcut = self.shortcut.trim().to_string();
        if self.shortcut.is_empty() {
            self.shortcut = DEFAULT_MAIN_SHORTCUT.to_string();
        } else if LEGACY_MAIN_SHORTCUTS.iter().any(|legacy| {
            normalize_shortcut_for_compare(&self.shortcut) == normalize_shortcut_for_compare(legacy)
        }) {
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

        self.theme_preset = default_theme_preset_if_blank(self.theme_preset);
        self.theme_accent = default_theme_accent_if_blank(self.theme_accent);
        if self.theme_accent == DEFAULT_THEME_ACCENT {
            self.theme_accent = migrate_legacy_theme_accent(&self.custom_theme_colors);
        }
        self.resolve_search_shortcut_conflict();
        self.session_keys = std::mem::take(&mut self.session_keys).sanitized();
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
    // 与 GUI 快捷键插件解耦后的等价规范化：先做注册名归一（win→Super 等）
    // 再统一小写。原实现先经插件解析成规范串，解析失败时同样回退到小写，
    // 比较语义（大小写不敏感的冲突检测）保持一致。
    normalize_shortcut_for_registration(shortcut).to_lowercase()
}

/// 把用户书写习惯归一为可注册形式：token 去空白、`win`/`windows` 映射为 `Super`。
pub fn normalize_shortcut_for_registration(shortcut: &str) -> String {
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

fn sanitize_hex_color(value: String, fallback: &str) -> String {
    let trimmed = value.trim();
    if is_valid_hex_color(trimmed) {
        trimmed.to_ascii_uppercase()
    } else {
        fallback.to_string()
    }
}

fn default_true() -> bool {
    true
}

fn default_theme_preset() -> String {
    DEFAULT_THEME_PRESET.to_string()
}

fn default_theme_accent() -> String {
    DEFAULT_THEME_ACCENT.to_string()
}

fn default_theme_preset_if_blank(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default_theme_preset()
    } else {
        trimmed.to_string()
    }
}

fn default_theme_accent_if_blank(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default_theme_accent()
    } else {
        trimmed.to_string()
    }
}

/// 旧 customThemeColors -> themeAccent 的迁移：底色由预设接管，
/// 仅当亮/暗强调色与旧默认不同（合法 hex）时保留一个作为自定义值。
fn migrate_legacy_theme_accent(legacy: &CustomThemeColors) -> String {
    let light = legacy.light.as_ref();
    let dark = legacy.dark.as_ref();

    if let Some(light_palette) = light {
        let accent = sanitize_hex_color(light_palette.accent.clone(), "");
        if !accent.is_empty() && accent != LEGACY_DEFAULT_LIGHT_ACCENT {
            return accent;
        }
    }
    if let Some(dark_palette) = dark {
        let accent = sanitize_hex_color(dark_palette.accent.clone(), "");
        if !accent.is_empty() && accent != LEGACY_DEFAULT_DARK_ACCENT {
            return accent;
        }
    }
    DEFAULT_THEME_ACCENT.to_string()
}

fn is_valid_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.chars().skip(1).all(|char| char.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{normalize_shortcut_for_compare, PickerPositionMode, ThemeMode, UserSetting};

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
    fn shortcut_defaults_to_ctrl_q() {
        let settings = UserSetting::default();
        assert_eq!(settings.shortcut, "Ctrl+Q");
    }

    #[test]
    fn always_run_elevated_defaults_to_false() {
        assert!(!UserSetting::default().always_run_elevated);

        // 两开关独立（对齐 PowerToys 行为逻辑）：管理员启动不牵动开机自启
        let settings = UserSetting {
            always_run_elevated: true,
            launch_on_startup: true,
            silent_on_startup: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert!(settings.always_run_elevated);
        assert!(settings.launch_on_startup);
        assert!(settings.silent_on_startup);
    }

    #[test]
    fn paste_trigger_defaults_to_click() {
        let settings = UserSetting::default();
        assert_eq!(settings.paste_trigger, super::PasteTrigger::Click);
    }

    #[test]
    fn deserialize_unknown_paste_trigger_falls_back_to_click() {
        let settings: UserSetting =
            serde_json::from_str(r#"{"pasteTrigger":"tripleClick"}"#).unwrap();
        assert_eq!(settings.paste_trigger, super::PasteTrigger::Click);

        let double: UserSetting =
            serde_json::from_str(r#"{"pasteTrigger":"doubleClick"}"#).unwrap();
        assert_eq!(double.paste_trigger, super::PasteTrigger::DoubleClick);
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
        assert_eq!(
            normalize_shortcut_for_registration("Windows+Shift+F"),
            "Super+Shift+F"
        );
        assert_eq!(normalize_shortcut_for_registration("Super+F"), "Super+F");
        assert_eq!(normalize_shortcut_for_registration("Ctrl+F"), "Ctrl+F");
    }

    #[test]
    fn sanitized_migrates_legacy_ctrl_backtick_to_default() {
        let settings = UserSetting {
            shortcut: "Ctrl+`".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.shortcut, "Ctrl+Q");
    }

    #[test]
    fn sanitized_migrates_legacy_alt_q_to_default() {
        let settings = UserSetting {
            shortcut: "Alt+Q".to_string(),
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.shortcut, "Ctrl+Q");
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
    fn deserialize_old_settings_defaults_picker_digit_shortcuts_to_enabled() {
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

        assert!(settings.picker_digit_shortcuts_enabled);

        let disabled: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "pickerDigitShortcutsEnabled":false
            }"#,
        )
        .unwrap();
        assert!(!disabled.picker_digit_shortcuts_enabled);
    }

    #[test]
    fn deserialize_old_settings_defaults_takeover_winv_to_disabled() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":false,
                "historyLimit":1000,
                "excludedApps":[],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false
            }"#,
        )
        .unwrap();

        assert!(!settings.takeover_winv);

        let enabled: UserSetting = serde_json::from_str(r#"{"takeoverWinv":true}"#).unwrap();
        assert!(enabled.takeover_winv);
    }

    #[test]
    fn session_keys_default_to_classic_combinations() {
        use super::SessionKeys;

        let keys = SessionKeys::default();
        assert_eq!(keys.confirm, "Enter");
        assert_eq!(keys.confirm_as_file, "Shift+Enter");
        assert_eq!(keys.open_editor, "Ctrl+Enter");
        assert_eq!(keys.toggle_favorite, "Ctrl+Space");
        assert_eq!(keys.dismiss, "Escape");
        assert_eq!(keys.navigate_up, "Up");
        assert_eq!(keys.navigate_down, "Down");
        assert_eq!(keys.delete_entry, "Delete");
    }

    #[test]
    fn deserialize_old_settings_defaults_session_keys() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "shortcut":"Alt+Q",
                "launchOnStartup":false,
                "historyLimit":1000,
                "excludedApps":[],
                "restoreClipboardAfterPaste":true,
                "pauseMonitoring":false
            }"#,
        )
        .unwrap();

        assert_eq!(settings.session_keys, super::SessionKeys::default());
    }

    #[test]
    fn session_keys_round_trip_through_serialization() {
        let settings = UserSetting {
            session_keys: super::SessionKeys {
                confirm: "Space".to_string(),
                ..super::SessionKeys::default()
            },
            ..UserSetting::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("sessionKeys"));
        assert!(json.contains("\"confirm\":\"Space\""));

        let parsed: UserSetting = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_keys.confirm, "Space");
        assert_eq!(parsed.session_keys.dismiss, "Escape");
    }

    #[test]
    fn sanitized_fills_blank_session_key_fields_with_defaults() {
        let settings = UserSetting {
            session_keys: super::SessionKeys {
                confirm: "  ".to_string(),
                dismiss: String::new(),
                ..super::SessionKeys::default()
            },
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.session_keys.confirm, "Enter");
        assert_eq!(settings.session_keys.dismiss, "Escape");
        assert_eq!(settings.session_keys.confirm_as_file, "Shift+Enter");
    }

    #[test]
    fn session_keys_field_accessors_cover_all_actions() {
        use super::SessionKeys;

        let mut keys = SessionKeys::default();
        for action in 0..8 {
            assert!(!keys.field(action).is_empty());
            keys.set_field(action, format!("K{action}"));
        }
        for action in 0..8 {
            assert_eq!(keys.field(action), format!("K{action}"));
        }
        // 越界动作读写为空/忽略，不 panic
        assert_eq!(keys.field(99), "");
        keys.set_field(99, "ignored".to_string());
    }

    #[test]
    fn search_shortcut_resets_to_default_when_conflicts_with_main_shortcut() {
        // 用非遗留组合：遗留值（如 Alt+Q）会先迁移到新默认，冲突随之消失
        let settings = UserSetting {
            shortcut: "Ctrl+R".to_string(),
            search_shortcut: "Ctrl+R".to_string(),
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

    #[test]
    fn theme_fields_default_to_preset_following() {
        let settings = UserSetting::default();

        assert_eq!(settings.theme_preset, "default");
        assert_eq!(settings.theme_accent, "default");
    }

    #[test]
    fn deserialize_old_settings_without_theme_fields_uses_defaults() {
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

        assert_eq!(settings.theme_preset, "default");
        assert_eq!(settings.theme_accent, "default");
    }

    #[test]
    fn sanitized_fills_blank_theme_fields_with_defaults() {
        let settings: UserSetting = serde_json::from_str(
            r#"{
                "themePreset":"",
                "themeAccent":" "
            }"#,
        )
        .unwrap();

        let settings = settings.sanitized();
        assert_eq!(settings.theme_preset, "default");
        assert_eq!(settings.theme_accent, "default");
    }

    #[test]
    fn sanitized_migrates_custom_legacy_accent_to_theme_accent() {
        let settings: UserSetting = serde_json::from_str(
            r##"{
                "customThemeColors":{
                    "light":{"windowBg":"#EFF2F5","cardBg":"#E6EAEF","accent":"#8250DF"},
                    "dark":{"windowBg":"#282C34","cardBg":"#2E333C","accent":"#478BE6"}
                }
            }"##,
        )
        .unwrap();

        let settings = settings.sanitized();
        assert_eq!(settings.theme_accent, "#8250DF");
    }

    #[test]
    fn sanitized_keeps_default_when_legacy_accent_matches_old_defaults() {
        let settings: UserSetting = serde_json::from_str(
            r##"{
                "customThemeColors":{
                    "light":{"windowBg":"#EFF2F5","cardBg":"#E6EAEF","accent":"#0969DA"},
                    "dark":{"windowBg":"#282C34","cardBg":"#2E333C","accent":"#478BE6"}
                }
            }"##,
        )
        .unwrap();

        let settings = settings.sanitized();
        assert_eq!(settings.theme_accent, "default");
    }

    #[test]
    fn sanitized_ignores_invalid_legacy_accent_hex() {
        let settings: UserSetting = serde_json::from_str(
            r##"{
                "customThemeColors":{
                    "light":{"windowBg":"#EFF2F5","cardBg":"#E6EAEF","accent":"blue"},
                    "dark":{"windowBg":"#282C34","cardBg":"#2E333C","accent":"#22"}
                }
            }"##,
        )
        .unwrap();

        let settings = settings.sanitized();
        assert_eq!(settings.theme_accent, "default");
    }

    #[test]
    fn explicit_theme_accent_is_not_overridden_by_legacy_migration() {
        let settings: UserSetting = serde_json::from_str(
            r##"{
                "themeAccent":"purple",
                "customThemeColors":{
                    "light":{"windowBg":"#EFF2F5","cardBg":"#E6EAEF","accent":"#8250DF"}
                }
            }"##,
        )
        .unwrap();

        let settings = settings.sanitized();
        assert_eq!(settings.theme_accent, "purple");
    }

    #[test]
    fn serialized_settings_omit_legacy_custom_theme_colors() {
        let settings: UserSetting = serde_json::from_str(
            r##"{
                "customThemeColors":{
                    "light":{"windowBg":"#EFF2F5","cardBg":"#E6EAEF","accent":"#8250DF"}
                }
            }"##,
        )
        .unwrap();

        let serialized = serde_json::to_string(&settings).unwrap();
        assert!(!serialized.contains("customThemeColors"));
        assert!(serialized.contains("themeAccent"));
    }
}
