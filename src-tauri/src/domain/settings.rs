use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSetting {
    pub shortcut: String,
    pub launch_on_startup: bool,
    pub history_limit: u32,
    pub excluded_apps: Vec<String>,
    pub restore_clipboard_after_paste: bool,
    pub pause_monitoring: bool,
}

impl Default for UserSetting {
    fn default() -> Self {
        Self {
            shortcut: "Ctrl+`".to_string(),
            launch_on_startup: false,
            history_limit: 1_000,
            excluded_apps: vec![
                "KeePass.exe".to_string(),
                "Bitwarden.exe".to_string(),
                "WindowsTerminal.exe".to_string(),
            ],
            restore_clipboard_after_paste: true,
            pause_monitoring: false,
        }
    }
}

impl UserSetting {
    pub fn sanitized(mut self) -> Self {
        self.shortcut = self.shortcut.trim().to_string();
        if self.shortcut.is_empty() {
            self.shortcut = "Ctrl+`".to_string();
        }

        self.history_limit = self.history_limit.clamp(100, 10_000);
        self.excluded_apps = self
            .excluded_apps
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self
    }
}
