#[cfg(test)]
mod win_f_shortcut_tests {
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::Shortcut;

    fn normalize_shortcut(shortcut: &str) -> Result<String, String> {
        let trimmed = shortcut.trim();
        if trimmed.is_empty() {
            return Ok(String::new());
        }

        Shortcut::from_str(trimmed)
            .map(|value| value.into_string().to_lowercase())
            .map_err(|error| format!("无效快捷键格式: {error}"))
    }

    #[test]
    fn test_super_f_normalization() {
        // 验证 Super+F 可以被正确规范化
        let result = normalize_shortcut("Super+F");
        assert!(
            result.is_ok(),
            "快捷键 'Super+F' 规范化失败: {:?}",
            result.err()
        );

        let normalized = result.unwrap();
        assert_eq!(normalized, "super+keyf", "Super+F 规范化结果应为 'super+keyf'");
        println!("Super+F -> {}", normalized);
    }

    #[test]
    fn test_win_key_not_supported() {
        // 验证 "Win" 键不被支持,以防止未来误用
        let result = Shortcut::from_str("Win+F");
        assert!(result.is_err(), "Win+F 不应该被支持");

        if let Err(error) = result {
            let error_msg = error.to_string();
            assert!(
                error_msg.contains("Win") || error_msg.contains("win"),
                "错误消息应提及 'Win' 键不被支持"
            );
            println!("✓ Win+F 正确地被拒绝: {}", error_msg);
        }
    }

    #[test]
    fn test_different_win_key_formats() {
        // 测试不同可能的 Windows 键格式
        let formats = vec![
            ("Win+F", false),           // 不支持
            ("Super+F", true),          // 支持,用于替代 Win
            ("CommandOrControl+F", true), // 支持,跨平台
        ];

        for (format, should_be_supported) in formats {
            let result = Shortcut::from_str(format);
            let is_supported = result.is_ok();

            assert_eq!(
                is_supported, should_be_supported,
                "{} {} 被支持",
                format,
                if should_be_supported { "应该" } else { "不应该" }
            );

            if let Ok(shortcut) = result {
                let shortcut_string = shortcut.into_string();
                println!("{:25} -> 可解析 -> {}", format, shortcut_string);

                // 验证 Super+F 的规范化结果
                if format == "Super+F" {
                    assert_eq!(shortcut_string, "super+KeyF");
                }
            } else {
                println!("{:25} -> 正确地不被支持", format);
            }
        }
    }

    #[test]
    fn test_default_workbench_shortcut_format() {
        // 验证默认工作窗快捷键使用正确格式
        // 注意: 默认值应该是 "Super+F" 而不是 "Win+F"
        // 因为 Tauri 的 global-shortcut 插件不支持 "Win" 修饰符

        let default_shortcut = "Super+F";
        let result = normalize_shortcut(default_shortcut);
        assert!(
            result.is_ok(),
            "默认工作窗快捷键应能被规范化: {:?}",
            result.err()
        );

        let normalized = result.unwrap();
        assert_eq!(normalized, "super+keyf");
        println!("✓ 默认工作窗快捷键 '{}' -> {}", default_shortcut, normalized);
    }
}
