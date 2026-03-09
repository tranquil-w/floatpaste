use std::path::Path;

use crate::domain::{error::AppError, settings::UserSetting};

const STARTUP_ENTRY_NAME: &str = "FloatPaste";

pub struct StartupService;

impl StartupService {
    pub fn sync_from_settings(settings: &UserSetting) -> Result<(), AppError> {
        #[cfg(target_os = "windows")]
        {
            let value = if settings.launch_on_startup {
                let executable = std::env::current_exe()
                    .map_err(|error| AppError::Message(error.to_string()))?;
                let arguments = if settings.silent_on_startup {
                    vec!["--silent"]
                } else {
                    Vec::new()
                };
                Some(Self::build_command_line(&executable, &arguments))
            } else {
                None
            };

            crate::platform::windows::startup::sync_run_entry(STARTUP_ENTRY_NAME, value.as_deref())
                .map_err(AppError::Message)?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = settings;
        }

        Ok(())
    }

    fn build_command_line(executable: &Path, arguments: &[&str]) -> String {
        let mut parts = vec![quote_argument(
            executable.as_os_str().to_string_lossy().as_ref(),
        )];
        parts.extend(arguments.iter().map(|argument| quote_argument(argument)));
        parts.join(" ")
    }
}

fn quote_argument(value: &str) -> String {
    if value.contains([' ', '\t', '"']) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::StartupService;

    #[test]
    fn build_command_line_quotes_executable_and_appends_silent_flag() {
        let command = StartupService::build_command_line(
            Path::new(r"C:\Program Files\FloatPaste\floatpaste.exe"),
            &["--silent"],
        );

        assert_eq!(
            command,
            "\"C:\\Program Files\\FloatPaste\\floatpaste.exe\" --silent"
        );
    }
}
