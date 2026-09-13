//! 主题桥接：core 派生的语义 token 写入各窗口的 Theme 全局。
//!
//! Slint 全局按组件实例各存一份，四个窗口都要显式写入。速贴/搜索/编辑
//! 消费同一完整键集（[`write_full_tokens`]），tooltip 只消费卡片所需
//! 子集（[`write_tooltip_tokens`]）——新增 token 只改这两个函数，
//! 不要在调用点散落增量写入，否则漏掉的窗口会静默落入默认色。

use slint::{Color, ComponentHandle};

use floatpaste_core::theme::ThemeTokens;

use crate::{EditorWindow, QuickPasteWindow, SearchWindow, Theme, TooltipWindow};

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

/// 完整 token 集：速贴 / 搜索 / 编辑窗口共用同一键集
fn write_full_tokens(theme: &Theme, tokens: &ThemeTokens) {
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
    theme.set_border_accent(hex_color(tokens.border_accent));
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
    theme.set_warning_emphasis(hex_color(tokens.warning_emphasis));
    theme.set_done_fg(hex_color(&tokens.done_fg));
    theme.set_done_subtle(rgba_color(tokens.done_subtle_rgb, tokens.done_subtle_alpha));
    theme.set_favorite(hex_color(tokens.favorite));
    theme.set_shadow_color(rgba_color(tokens.shadow_color, 1.0));
    theme.set_selection_fg(hex_color(&tokens.selection_fg));
    theme.set_selection_bg(hex_color(&tokens.selection_bg));
}

/// tooltip 子集：卡片底/内嵌底、三级前景、分隔边框与阴影
fn write_tooltip_tokens(theme: &Theme, tokens: &ThemeTokens) {
    theme.set_canvas_subtle(hex_color(tokens.canvas_subtle));
    theme.set_canvas_inset(hex_color(tokens.canvas_inset));
    theme.set_fg_default(hex_color(&tokens.fg_default));
    theme.set_fg_muted(hex_color(&tokens.fg_muted));
    theme.set_fg_subtle(hex_color(&tokens.fg_subtle));
    theme.set_border_muted(hex_color(tokens.border_muted));
    theme.set_shadow_color(rgba_color(tokens.shadow_color, 1.0));
}

/// 把 token 应用到速贴窗口（以及已创建的 tooltip / 搜索 / 编辑 / 设置窗口）
pub fn apply_theme(
    picker: &QuickPasteWindow,
    tooltip: Option<&TooltipWindow>,
    search: Option<&SearchWindow>,
    editor: Option<&EditorWindow>,
    settings: Option<&crate::SettingsWindow>,
    tokens: &ThemeTokens,
) {
    write_full_tokens(&picker.global::<Theme>(), tokens);
    if let Some(tooltip) = tooltip {
        write_tooltip_tokens(&tooltip.global::<Theme>(), tokens);
    }
    if let Some(search) = search {
        write_full_tokens(&search.global::<Theme>(), tokens);
    }
    if let Some(editor) = editor {
        write_full_tokens(&editor.global::<Theme>(), tokens);
    }
    if let Some(settings) = settings {
        write_full_tokens(&settings.global::<Theme>(), tokens);
    }
}

/// 运行时主题重应用（设置保存后的联动路径）：从 App 弱引用升级全部窗口。
/// 任一窗口升级失败则跳过该窗口（未就绪时不强求）。
pub fn reapply_theme(app: &crate::picker::App, tokens: &ThemeTokens) {
    let picker = app.picker.upgrade();
    let tooltip = app.tooltip.upgrade();
    let search = app.search.upgrade();
    let editor = app.editor.upgrade();
    let settings = app.settings.upgrade();
    if let Some(picker) = picker.as_ref() {
        write_full_tokens(&picker.global::<Theme>(), tokens);
    }
    if let Some(tooltip) = tooltip.as_ref() {
        write_tooltip_tokens(&tooltip.global::<Theme>(), tokens);
    }
    if let Some(search) = search.as_ref() {
        write_full_tokens(&search.global::<Theme>(), tokens);
    }
    if let Some(editor) = editor.as_ref() {
        write_full_tokens(&editor.global::<Theme>(), tokens);
    }
    if let Some(settings) = settings.as_ref() {
        write_full_tokens(&settings.global::<Theme>(), tokens);
    }
}
