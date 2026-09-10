#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Normal,
    SilentStartup,
}

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
