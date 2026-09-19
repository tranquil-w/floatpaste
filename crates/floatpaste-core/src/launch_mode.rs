#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Normal,
    SilentStartup,
}

/// 提权重启标记（管理员上屏提示条的「以管理员身份重启」）：新实例经 UAC
/// 启动后先等旧实例释放单实例互斥量，再以普通模式（重新弹出速贴）接管
pub const ELEVATED_RELAUNCH_ARG: &str = "--elevated-relaunch";

/// 提权重入自身：注册管理员自启任务后以退出码 0/1 报告结果
pub const SETUP_ELEVATED_AUTOSTART_ARG: &str = "--setup-elevated-autostart";

/// 提权重入自身：卸载管理员自启任务后以退出码 0/1 报告结果
pub const REMOVE_ELEVATED_AUTOSTART_ARG: &str = "--remove-elevated-autostart";

impl LaunchMode {
    pub fn from_env() -> Self {
        let is_silent = std::env::args_os().any(|arg| arg == "--silent");
        if is_silent {
            Self::SilentStartup
        } else {
            Self::Normal
        }
    }

    pub fn is_silent(self) -> bool {
        matches!(self, Self::SilentStartup)
    }
}

#[cfg(test)]
mod tests {
    use super::LaunchMode;

    fn parse(args: &[&str]) -> LaunchMode {
        if args.iter().any(|arg| *arg == "--silent") {
            LaunchMode::SilentStartup
        } else {
            LaunchMode::Normal
        }
    }

    #[test]
    fn defaults_to_normal_without_silent_flag() {
        assert_eq!(parse(&["floatpaste.exe"]), LaunchMode::Normal);
    }

    #[test]
    fn parses_silent_flag() {
        assert_eq!(
            parse(&["floatpaste.exe", "--silent"]),
            LaunchMode::SilentStartup
        );
    }
}
