//! 主题桥接：core 派生的语义 token 写入各窗口的 Theme 全局单例。

use slint::{Color, ComponentHandle};

use floatpaste_core::theme::ThemeTokens;

use crate::{QuickPasteWindow, Theme, TooltipWindow};

fn hex_color(hex: &str) -> Color {
    let bytes = hex.as_bytes();
    if bytes.len() == 7 && bytes[0] == b'#' {
        let channel = |range: std::ops::Range<usize>| -> u8 {
            u8::from_str_radix(std::str::from_utf8(&bytes[range]).unwrap_or("0"), 16).unwrap_or(0)
        };
        return Color::from_rgb_u8(channel(1..3), channel(3..5), channel(5..7));
    }
    Color::from_rgb_u8(0, 0, 0)
}

fn rgba_color(rgb: [u8; 3], alpha: f32) -> Color {
    Color::from_argb_encoded(
        ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32) << 24
            | (rgb[0] as u32) << 16
            | (rgb[1] as u32) << 8
            | rgb[2] as u32,
    )
}

/// 把 token 应用到速贴窗口（以及已创建的 tooltip 窗口）
pub fn apply_theme(
    picker: &QuickPasteWindow,
    tooltip: Option<&TooltipWindow>,
    tokens: &ThemeTokens,
) {
    let theme = picker.global::<Theme>();
    theme.set_canvas_default(hex_color(tokens.canvas_default));
    theme.set_canvas_subtle(hex_color(tokens.canvas_subtle));
    theme.set_canvas_inset(hex_color(tokens.canvas_inset));
    theme.set_fg_default(hex_color(&tokens.fg_default));
    theme.set_fg_muted(hex_color(&tokens.fg_muted));
    theme.set_fg_subtle(hex_color(&tokens.fg_subtle));
    theme.set_fg_on_emphasis(hex_color(&tokens.fg_on_emphasis));
    theme.set_accent_fg(hex_color(&tokens.accent_fg));
    theme.set_accent_emphasis(hex_color(&tokens.accent_emphasis));
    theme.set_accent_hover(hex_color(&tokens.accent_hover));
    theme.set_accent_subtle(rgba_color(
        tokens.accent_subtle_rgb,
        tokens.accent_subtle_alpha,
    ));
    theme.set_border_default(hex_color(&tokens.border_default));
    theme.set_border_window(hex_color(&tokens.border_window));
    theme.set_border_muted(hex_color(tokens.border_muted));
    theme.set_border_subtle(hex_color(&tokens.border_subtle));
    theme.set_success_fg(hex_color(&tokens.success_fg));
    theme.set_success_subtle(rgba_color(
        tokens.success_subtle_rgb,
        tokens.success_subtle_alpha,
    ));
    theme.set_danger_fg(hex_color(&tokens.danger_fg));
    theme.set_danger_subtle(rgba_color(
        tokens.danger_subtle_rgb,
        tokens.danger_subtle_alpha,
    ));
    theme.set_warning_fg(hex_color(&tokens.warning_fg));
    theme.set_warning_subtle(rgba_color(
        tokens.warning_subtle_rgb,
        tokens.warning_subtle_alpha,
    ));
    theme.set_done_fg(hex_color(&tokens.done_fg));
    theme.set_done_subtle(rgba_color(tokens.done_subtle_rgb, tokens.done_subtle_alpha));
    theme.set_favorite(hex_color(tokens.favorite));
    theme.set_shadow_color(rgba_color(tokens.shadow_color, 1.0));

    if let Some(tooltip) = tooltip {
        let theme = tooltip.global::<Theme>();
        theme.set_canvas_subtle(hex_color(tokens.canvas_subtle));
        theme.set_canvas_inset(hex_color(tokens.canvas_inset));
        theme.set_fg_default(hex_color(&tokens.fg_default));
        theme.set_fg_muted(hex_color(&tokens.fg_muted));
        theme.set_fg_subtle(hex_color(&tokens.fg_subtle));
        theme.set_border_muted(hex_color(tokens.border_muted));
        theme.set_shadow_color(rgba_color(tokens.shadow_color, 1.0));
    }
}
