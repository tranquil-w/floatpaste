use serde::{Deserialize, Deserializer, Serialize};

use super::clip_item::{
    PasteMode, FILE_PASTE_MODES, IMAGE_PASTE_MODES, PLAIN_TEXT_PASTE_MODES, RICH_TEXT_PASTE_MODES,
};

pub const DEFAULT_MAIN_SHORTCUT: &str = "Alt+`";
const DEFAULT_SEARCH_SHORTCUT: &str = "Alt+Q";
const SESSION_KEYS_VERSION: u32 = 1;
const GLOBAL_SHORTCUTS_VERSION: u32 = 2;
/// 仅在旧配置首次加载时迁移，不能覆盖用户升级后重新录制的组合。
const LEGACY_MAIN_SHORTCUTS: [&str; 3] = ["Ctrl+Q", "Alt+Q", "Ctrl+`"];
const LEGACY_SEARCH_SHORTCUTS: [&str; 4] = ["Alt+S", "Win+F", "Super+F", "Ctrl+Q"];
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

/// 窗口背景材质档位（docs/adr-0005），全窗口生效：
/// - 无：全部窗口不透明实底
/// - Mica：可激活窗走 DWM Mica；速贴为无焦点窗同样挂 DWM Mica，
///   失活时 DWM 不渲染 backdrop 则退化为均匀 tint（已接受）
/// - Acrylic：可激活窗走 DWM Acrylic；速贴为唤起时自绘烘焙毛玻璃
///   （失活 DWM Acrylic 无模糊，烘焙更优）
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowMaterial {
    None,
    Mica,
    Acrylic,
}

impl Default for WindowMaterial {
    fn default() -> Self {
        Self::Acrylic
    }
}

impl<'de> Deserialize<'de> for WindowMaterial {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        Ok(match value.as_str() {
            "none" => Self::None,
            "mica" => Self::Mica,
            _ => Self::Acrylic,
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
/// "Enter"、"Ctrl+Enter"、"Ctrl+D"），键名与修饰键大小写不敏感；
/// 非法或缺失的值按字段回退默认，见 [SessionKeys::sanitized]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionKeys {
    /// 上屏
    pub confirm: String,
    /// 次级上屏：执行该类别配置的默认动作（见 SecondaryPasteDefaults）。
    /// 字段名是持久化配置键，不可更名；默认 Ctrl+Enter
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
    /// 删除条目（速贴与搜索窗口，两段式确认）
    pub delete_entry: String,
}

impl Default for SessionKeys {
    fn default() -> Self {
        Self {
            confirm: "Enter".to_string(),
            confirm_as_file: "Ctrl+Enter".to_string(),
            open_editor: "Ctrl+E".to_string(),
            toggle_favorite: "Ctrl+D".to_string(),
            dismiss: "Escape".to_string(),
            navigate_up: "Up".to_string(),
            navigate_down: "Down".to_string(),
            delete_entry: "Delete".to_string(),
        }
    }
}

/// 仅用于带旧版本标记的配置；日常保存不迁移用户重新录制的旧键位。
const LEGACY_SESSION_KEY_MIGRATIONS: [(usize, &str, &str); 3] = [
    (1, "Shift+Enter", "Ctrl+Enter"),
    (2, "Ctrl+Enter", "Ctrl+E"),
    (3, "Ctrl+Space", "Ctrl+D"),
];

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

    fn conflicts_with_shortcuts(&self, main: &str, search: Option<&str>) -> bool {
        let values: Vec<String> = (0..8)
            .map(|action| normalize_shortcut_for_compare(self.field(action)))
            .collect();
        let main = normalize_shortcut_for_compare(main);
        let search = search.map(normalize_shortcut_for_compare);
        values.iter().enumerate().any(|(index, value)| {
            values[..index].contains(value) || *value == main || search.as_ref() == Some(value)
        })
    }
}

/// 各类别条目在次级入口（次级会话键 / 次级按钮）执行的默认动作。
/// 文本按是否带 HTML 选择基础动作；链接/路径可单独覆盖，None 表示沿用。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryPasteDefaults {
    #[serde(default)]
    pub plain_text: PasteMode,
    /// 链接与路径只允许对应外部动作，缺省沿用普通文本/富文本设置。
    #[serde(default)]
    pub link: Option<PasteMode>,
    #[serde(default)]
    pub path: Option<PasteMode>,
    /// 富文本（带 HTML 的文本）
    #[serde(default = "default_rich_text_secondary")]
    pub rich_text: PasteMode,
    #[serde(default = "default_image_secondary")]
    pub image: PasteMode,
    #[serde(default = "default_file_secondary")]
    pub file: PasteMode,
}

fn default_rich_text_secondary() -> PasteMode {
    PasteMode::PlainText
}

fn default_image_secondary() -> PasteMode {
    PasteMode::PathText
}

fn default_file_secondary() -> PasteMode {
    PasteMode::PathText
}

impl Default for SecondaryPasteDefaults {
    fn default() -> Self {
        Self {
            plain_text: PasteMode::Original,
            link: None,
            path: None,
            rich_text: default_rich_text_secondary(),
            image: default_image_secondary(),
            file: default_file_secondary(),
        }
    }
}

impl SecondaryPasteDefaults {
    /// 类别可用目录外的值回退内置默认（与条目侧解析的回退语义一致，
    /// 防旧库/手改配置残留进入界面草稿）
    pub fn sanitized(self) -> Self {
        fn in_catalog_or(mode: PasteMode, catalog: &[PasteMode], fallback: PasteMode) -> PasteMode {
            if catalog.contains(&mode) {
                mode
            } else {
                fallback
            }
        }
        Self {
            plain_text: in_catalog_or(self.plain_text, PLAIN_TEXT_PASTE_MODES, PasteMode::Original),
            link: self.link.filter(|mode| *mode == PasteMode::OpenInBrowser),
            path: self.path.filter(|mode| *mode == PasteMode::RevealInFolder),
            rich_text: in_catalog_or(
                self.rich_text,
                RICH_TEXT_PASTE_MODES,
                default_rich_text_secondary(),
            ),
            image: in_catalog_or(self.image, IMAGE_PASTE_MODES, default_image_secondary()),
            file: in_catalog_or(self.file, FILE_PASTE_MODES, default_file_secondary()),
        }
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
/// # 快捷键
///
/// - **主快捷键**：用于打开速贴
/// - **搜索窗口快捷键**：用于全局打开搜索窗口
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
    /// 未收藏历史按最后复制/使用时间保留天数；0 表示不按时间清理。
    pub history_retention_days: u32,
    /// 普通删除的缓冲天数；0 表示下一轮清理即可物理回收。
    pub deleted_retention_days: u32,
    pub picker_record_limit: u32,
    /// 速贴行卡片总高（逻辑像素），允许 80–160；预览区高度、缩略图尺寸
    /// 与预览行数预算由其派生（native 的 PanelGeometry 与
    /// picker::apply_card_height），默认 104 对应 60px 预览与三行裁排
    pub picker_card_height: u32,
    pub picker_position_mode: PickerPositionMode,
    /// 速贴/搜索条目的鼠标上屏触发方式，见 [PasteTrigger]
    pub paste_trigger: PasteTrigger,
    pub excluded_apps: Vec<String>,
    pub restore_clipboard_after_paste: bool,
    pub pause_monitoring: bool,
    pub theme_mode: ThemeMode,
    /// 窗口背景材质档位，见 [WindowMaterial]
    pub window_material: WindowMaterial,
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
    /// 会话键迁移版本；旧配置缺失时为 0，新设置直接使用当前版本。
    #[serde(default)]
    pub session_keys_version: u32,
    /// 全局快捷键迁移版本；旧配置缺失时为 0。
    #[serde(default)]
    pub global_shortcuts_version: u32,
    /// 各类别默认次级动作（次级会话键 / 次级按钮立即执行），见
    /// [SecondaryPasteDefaults]
    #[serde(default)]
    pub secondary_paste: SecondaryPasteDefaults,
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
            history_retention_days: 0,
            deleted_retention_days: 30,
            picker_record_limit: 50,
            picker_card_height: 104,
            picker_position_mode: PickerPositionMode::Mouse,
            paste_trigger: PasteTrigger::Click,
            excluded_apps: vec![
                "KeePass.exe".to_string(),
                "Bitwarden.exe".to_string(),
                "WindowsTerminal.exe".to_string(),
            ],
            restore_clipboard_after_paste: false,
            pause_monitoring: false,
            theme_mode: ThemeMode::System,
            window_material: WindowMaterial::Acrylic,
            search_shortcut: DEFAULT_SEARCH_SHORTCUT.to_string(),
            search_shortcut_enabled: true,
            picker_digit_shortcuts_enabled: true,
            takeover_winv: false,
            session_keys: SessionKeys::default(),
            session_keys_version: SESSION_KEYS_VERSION,
            global_shortcuts_version: GLOBAL_SHORTCUTS_VERSION,
            secondary_paste: SecondaryPasteDefaults::default(),
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
        }

        if !self.launch_on_startup {
            self.silent_on_startup = false;
        }

        self.history_limit = self.history_limit.clamp(100, 10_000);
        self.history_retention_days = self.history_retention_days.min(3650);
        self.deleted_retention_days = self.deleted_retention_days.min(365);
        self.picker_record_limit = self.picker_record_limit.clamp(9, 1_000);
        self.picker_card_height = self.picker_card_height.clamp(80, 160);
        self.excluded_apps = self
            .excluded_apps
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self.excluded_apps.sort_by_key(|name| name.to_lowercase());
        self.excluded_apps
            .dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        self.search_shortcut = self.search_shortcut.trim().to_string();
        if self.search_shortcut.is_empty() {
            self.search_shortcut = DEFAULT_SEARCH_SHORTCUT.to_string();
        }

        self.theme_preset = default_theme_preset_if_blank(self.theme_preset);
        self.theme_accent = default_theme_accent_if_blank(self.theme_accent);
        if self.theme_accent == DEFAULT_THEME_ACCENT {
            self.theme_accent = migrate_legacy_theme_accent(&self.custom_theme_colors);
        }
        self.session_keys = std::mem::take(&mut self.session_keys).sanitized();
        if self.global_shortcuts_version < GLOBAL_SHORTCUTS_VERSION {
            self.migrate_global_shortcuts();
            self.global_shortcuts_version = GLOBAL_SHORTCUTS_VERSION;
        }
        self.resolve_search_shortcut_conflict();
        if self.session_keys_version < SESSION_KEYS_VERSION {
            self.migrate_session_keys();
            self.session_keys_version = SESSION_KEYS_VERSION;
        }
        self.secondary_paste = self.secondary_paste.sanitized();
        self
    }

    fn migrate_global_shortcuts(&mut self) {
        // v1 已完成更早的迁移，只识别它的默认值，保留当时重新录制的旧组合。
        let (legacy_main, legacy_search): (&[&str], &[&str]) = if self.global_shortcuts_version == 0
        {
            (&LEGACY_MAIN_SHORTCUTS, &LEGACY_SEARCH_SHORTCUTS)
        } else {
            (&["Ctrl+`"], &["Ctrl+Q"])
        };
        let migrate = |value: &str, legacy: &[&str], default: &str| {
            if legacy.iter().any(|old| {
                normalize_shortcut_for_compare(value) == normalize_shortcut_for_compare(old)
            }) {
                default.to_string()
            } else {
                value.to_string()
            }
        };
        let main = migrate(&self.shortcut, legacy_main, DEFAULT_MAIN_SHORTCUT);
        let search = migrate(
            &self.search_shortcut,
            legacy_search,
            DEFAULT_SEARCH_SHORTCUT,
        );
        if (self.search_shortcut_enabled
            && normalize_shortcut_for_compare(&main) == normalize_shortcut_for_compare(&search))
            || self.session_keys.conflicts_with_shortcuts(
                &main,
                self.search_shortcut_enabled.then_some(search.as_str()),
            )
        {
            return;
        }
        self.shortcut = main;
        self.search_shortcut = search;
    }

    fn migrate_session_keys(&mut self) {
        let mut migrated = self.session_keys.clone();
        for (action, legacy, next) in LEGACY_SESSION_KEY_MIGRATIONS {
            if normalize_shortcut_for_compare(migrated.field(action))
                == normalize_shortcut_for_compare(legacy)
            {
                migrated.set_field(action, next.to_string());
            }
        }
        // Ctrl+Enter 同时涉及旧编辑键和新次级键，冲突时整组保留，避免级联占用。
        if !migrated.conflicts_with_shortcuts(
            &self.shortcut,
            self.search_shortcut_enabled
                .then_some(self.search_shortcut.as_str()),
        ) {
            self.session_keys = migrated;
        }
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

/// 快捷键比较口径：忽略大小写、修饰键顺序和平台支持的键名别名。
pub fn normalize_shortcut_for_compare(shortcut: &str) -> String {
    let mut parts: Vec<String> = shortcut
        .split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "control" => "ctrl".to_string(),
            "win" | "windows" | "super" => "win".to_string(),
            "return" => "enter".to_string(),
            "esc" => "escape".to_string(),
            "del" => "delete".to_string(),
            "backquote" => "`".to_string(),
            "arrowup" => "up".to_string(),
            "arrowdown" => "down".to_string(),
            "arrowleft" => "left".to_string(),
            "arrowright" => "right".to_string(),
            other => other.to_string(),
        })
        .filter(|part| !part.is_empty())
        .collect();
    parts.sort_unstable();
    parts.dedup();
    parts.join("+")
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
    use super::{
        normalize_shortcut_for_compare, PickerPositionMode, SecondaryPasteDefaults, ThemeMode,
        UserSetting, WindowMaterial,
    };
    use crate::domain::clip_item::PasteMode;

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
        assert_eq!(settings.picker_card_height, 104);
        assert_eq!(settings.picker_position_mode, PickerPositionMode::Mouse);
        assert_eq!(settings.theme_mode, ThemeMode::System);
    }

    #[test]
    fn sanitized_clamps_picker_card_height() {
        for (input, expected) in [(0, 80), (79, 80), (104, 104), (160, 160), (999, 160)] {
            let settings = UserSetting {
                picker_card_height: input,
                ..UserSetting::default()
            }
            .sanitized();
            assert_eq!(settings.picker_card_height, expected);
        }
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
    fn sanitized_migrates_legacy_global_defaults_once() {
        for (version, main, search) in [
            (0, "Ctrl+Q", "Alt+S"),
            (0, "Alt+Q", "Win+F"),
            (0, "Ctrl+`", "Super+F"),
            (1, "Ctrl+`", "Ctrl+Q"),
            (1, "Control+`", "q+control"),
        ] {
            let settings = UserSetting {
                shortcut: main.into(),
                search_shortcut: search.into(),
                global_shortcuts_version: version,
                ..UserSetting::default()
            }
            .sanitized();
            assert_eq!(settings.shortcut, "Alt+`");
            assert_eq!(settings.search_shortcut, "Alt+Q");
            assert_eq!(settings.global_shortcuts_version, 2);
        }
        let settings = UserSetting {
            shortcut: "Ctrl+Q".into(),
            search_shortcut: "Alt+S".into(),
            ..UserSetting::default()
        }
        .sanitized();
        assert_eq!(settings.shortcut, "Ctrl+Q", "新版主动恢复旧组合不再迁移");
        assert_eq!(settings.search_shortcut, "Alt+S");
    }

    #[test]
    fn global_shortcut_migration_preserves_customizations_and_conflicts() {
        let settings = UserSetting {
            shortcut: "Ctrl+R".into(),
            search_shortcut: "Alt+S".into(),
            global_shortcuts_version: 0,
            ..UserSetting::default()
        }
        .sanitized();
        assert_eq!(settings.shortcut, "Ctrl+R");
        assert_eq!(settings.search_shortcut, "Alt+Q");

        let settings = UserSetting {
            shortcut: "Ctrl+R".into(),
            search_shortcut: "Alt+S".into(),
            global_shortcuts_version: 1,
            ..UserSetting::default()
        }
        .sanitized();
        assert_eq!(settings.shortcut, "Ctrl+R");
        assert_eq!(
            settings.search_shortcut, "Alt+S",
            "v1 自定义旧组合不能重新迁移"
        );

        // 搜索已自定义为新速贴键，整组保留原组合，不能改成重复键或停用搜索。
        let settings = UserSetting {
            shortcut: "Ctrl+Q".into(),
            search_shortcut: "ALT+`".into(),
            global_shortcuts_version: 0,
            ..UserSetting::default()
        }
        .sanitized();
        assert_eq!(settings.shortcut, "Ctrl+Q");
        assert_eq!(settings.search_shortcut, "ALT+`");
        assert!(settings.search_shortcut_enabled);

        let mut settings = UserSetting {
            shortcut: "Ctrl+Q".into(),
            search_shortcut: "Alt+S".into(),
            global_shortcuts_version: 0,
            ..UserSetting::default()
        };
        settings.session_keys.delete_entry = "Alt+Backquote".into();
        let settings = settings.sanitized();
        assert_eq!(settings.shortcut, "Ctrl+Q");
        assert_eq!(settings.search_shortcut, "Alt+S");
        assert_eq!(settings.session_keys.delete_entry, "Alt+Backquote");
    }

    #[test]
    fn win_and_super_shortcuts_are_treated_as_the_same_combination() {
        assert_eq!(
            normalize_shortcut_for_compare("Win+F"),
            normalize_shortcut_for_compare("Super+F")
        );
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
    fn window_material_unknown_value_falls_back_to_acrylic_and_round_trips() {
        let unknown: UserSetting = serde_json::from_str(r#"{"windowMaterial":"frost"}"#).unwrap();
        assert_eq!(unknown.window_material, WindowMaterial::Acrylic);

        for material in [
            WindowMaterial::None,
            WindowMaterial::Mica,
            WindowMaterial::Acrylic,
        ] {
            let json = serde_json::to_string(&UserSetting {
                window_material: material,
                ..UserSetting::default()
            })
            .unwrap();
            let back: UserSetting = serde_json::from_str(&json).unwrap();
            assert_eq!(back.window_material, material);
        }
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
        assert_eq!(settings.session_keys.confirm_as_file, "Ctrl+Enter");
    }

    #[test]
    fn sanitized_migrates_legacy_session_key_defaults() {
        // 旧配置文件保存的是旧默认：升级加载时迁移到新默认
        let settings = UserSetting {
            session_keys_version: 0,
            session_keys: super::SessionKeys {
                confirm_as_file: "shift+enter".to_string(),
                open_editor: "Ctrl+Enter".to_string(),
                toggle_favorite: "Ctrl+Space".to_string(),
                ..super::SessionKeys::default()
            },
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.session_keys.confirm_as_file, "Ctrl+Enter");
        assert_eq!(settings.session_keys.open_editor, "Ctrl+E");
        assert_eq!(settings.session_keys.toggle_favorite, "Ctrl+D");

        // 用户自定义值不被迁移
        let settings = UserSetting {
            session_keys_version: 0,
            session_keys: super::SessionKeys {
                confirm_as_file: "Alt+Enter".to_string(),
                ..super::SessionKeys::default()
            },
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.session_keys.confirm_as_file, "Alt+Enter");
    }

    #[test]
    fn session_key_migration_preserves_conflicting_customizations_and_runs_once() {
        let legacy = super::SessionKeys {
            confirm_as_file: "Shift+Enter".into(),
            open_editor: "Ctrl+Enter".into(),
            toggle_favorite: "Ctrl+Space".into(),
            ..super::SessionKeys::default()
        };
        for (action, custom) in [(7, "D+Control"), (1, "Ctrl+E")] {
            let mut keys = legacy.clone();
            keys.set_field(action, custom.into());
            let migrated = UserSetting {
                session_keys: keys.clone(),
                session_keys_version: 0,
                ..UserSetting::default()
            }
            .sanitized();
            assert_eq!(migrated.session_keys, keys);
            assert_eq!(migrated.session_keys_version, super::SESSION_KEYS_VERSION);
        }
        for search_enabled in [false, true] {
            let settings = UserSetting {
                session_keys: legacy.clone(),
                session_keys_version: 0,
                shortcut: if search_enabled {
                    "Ctrl+Q"
                } else {
                    "E+Control"
                }
                .into(),
                search_shortcut: "Ctrl+D".into(),
                search_shortcut_enabled: search_enabled,
                ..UserSetting::default()
            }
            .sanitized();
            assert_eq!(settings.session_keys, legacy);
        }
        let mut upgraded = UserSetting {
            session_keys: legacy.clone(),
            session_keys_version: 0,
            ..UserSetting::default()
        }
        .sanitized();
        upgraded.session_keys = legacy.clone();
        let saved = upgraded.sanitized();
        assert_eq!(saved.session_keys, legacy);
        let reloaded: UserSetting =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(reloaded.sanitized().session_keys, legacy);
        assert_eq!(
            normalize_shortcut_for_compare("Control+Alt+Return"),
            normalize_shortcut_for_compare("Enter+ALT+Ctrl")
        );
    }

    #[test]
    fn search_shortcut_resets_to_default_when_conflicts_with_main_shortcut() {
        // 当前版本配置只处理冲突，不再按旧默认值重写用户组合。
        let settings = UserSetting {
            shortcut: "Ctrl+R".to_string(),
            search_shortcut: "Ctrl+R".to_string(),
            search_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.search_shortcut, "Alt+Q");
    }

    #[test]
    fn search_shortcut_conflict_detection_uses_normalized_shortcuts() {
        let settings = UserSetting {
            shortcut: "ALT+Q".to_string(),
            search_shortcut: "q+alt".to_string(),
            search_shortcut_enabled: true,
            ..UserSetting::default()
        }
        .sanitized();

        assert!(!settings.search_shortcut_enabled);
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

    #[test]
    fn secondary_paste_defaults_match_legacy_secondary_behavior() {
        let defaults = SecondaryPasteDefaults::default();
        // 次级动作可配置之前的既有行为：富文本纯文本、图片/文件路径文本
        assert_eq!(defaults.rich_text, PasteMode::PlainText);
        assert_eq!(defaults.image, PasteMode::PathText);
        assert_eq!(defaults.file, PasteMode::PathText);
    }

    #[test]
    fn secondary_paste_defaults_round_trip_and_missing_fields_fall_back() {
        let settings = UserSetting {
            secondary_paste: SecondaryPasteDefaults {
                rich_text: PasteMode::Original,
                image: PasteMode::ImageFile,
                file: PasteMode::FileNameText,
                plain_text: PasteMode::TextFile,
                link: Some(PasteMode::OpenInBrowser),
                path: Some(PasteMode::RevealInFolder),
            },
            ..UserSetting::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("secondaryPaste"));
        let parsed: UserSetting = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.secondary_paste, settings.secondary_paste);

        // 旧配置没有该字段：整体回退默认
        let legacy: UserSetting = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(legacy.secondary_paste, SecondaryPasteDefaults::default());

        // 部分字段缺失：仅缺失项回退默认
        let partial: UserSetting =
            serde_json::from_str(r#"{"secondaryPaste":{"image":"image_file"}}"#).unwrap();
        assert_eq!(partial.secondary_paste.image, PasteMode::ImageFile);
        assert_eq!(partial.secondary_paste.rich_text, PasteMode::PlainText);
        assert_eq!(partial.secondary_paste.plain_text, PasteMode::Original);
        assert_eq!(partial.secondary_paste.link, None);
        assert_eq!(partial.secondary_paste.path, None);
    }

    #[test]
    fn sanitized_resets_secondary_values_outside_category_catalog() {
        let settings = UserSetting {
            secondary_paste: SecondaryPasteDefaults {
                rich_text: PasteMode::PathText,
                image: PasteMode::FileNameText,
                file: PasteMode::PlainText,
                plain_text: PasteMode::PlainText,
                link: Some(PasteMode::RevealInFolder),
                path: Some(PasteMode::OpenInBrowser),
            },
            ..UserSetting::default()
        }
        .sanitized();

        assert_eq!(settings.secondary_paste.plain_text, PasteMode::Original);
        assert_eq!(settings.secondary_paste.link, None);
        assert_eq!(settings.secondary_paste.path, None);
        assert_eq!(settings.secondary_paste.rich_text, PasteMode::PlainText);
        assert_eq!(settings.secondary_paste.image, PasteMode::PathText);
        assert_eq!(settings.secondary_paste.file, PasteMode::PathText);
    }
}
