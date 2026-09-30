//! 速贴观感诊断工具：无头渲染速贴窗口快照（不依赖桌面，不模拟输入）。
//!
//! 用法：`cargo run -p floatpaste-native --bin picker-snapshot -- <light|dark> <tag> [hover-y]`
//! - mode：light / dark（默认预设 token 全量写入）
//! - tag：输出文件名后缀（迭代对比用 before/after 等）
//! - hover-y：可选逻辑 y 坐标，dispatch PointerMoved 模拟行悬停
//!
//! 输出 `.artifacts/snapshot/picker-<mode>-<tag>.png`（不入 Git）。
//! 速贴面板为实底设计，快照按中灰蓝桌面色合成材质残透，近似真机观感。
//!
//! 维护约束：Theme token 写入集必须与 `theme_bridge::write_full_tokens`
//! 键集保持一致——漏写半透明 token 不报错，只静默透明（观感误判源头）。

use floatpaste_core::theme::{derive_tokens, ResolvedTheme};
use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::{Platform, PlatformError, WindowAdapter};
use slint::{ComponentHandle, SharedString};

slint::include_modules!();

struct SnapshotPlatform {
    window: std::rc::Rc<MinimalSoftwareWindow>,
}

impl Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<std::rc::Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
}

fn hex_color(hex: &str) -> slint::Color {
    let b = hex.as_bytes();
    let ch = |r: std::ops::Range<usize>| u8::from_str_radix(std::str::from_utf8(&b[r]).unwrap(), 16).unwrap();
    slint::Color::from_rgb_u8(ch(1..3), ch(3..5), ch(5..7))
}

fn rgba_color(rgb: [u8; 3], alpha: f32) -> slint::Color {
    slint::Color::from_argb_encoded(
        ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32) << 24
            | (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | (rgb[2] as u32),
    )
}

fn row(preview: &str, type_label: &str, source: &str, time: &str, digit: &str,
       favorited: bool, tags: Vec<&str>) -> ClipRow {
    ClipRow {
        id: "1".into(),
        preview: SharedString::from(preview),
        type_label: SharedString::from(type_label),
        source_app: SharedString::from(source),
        time_text: SharedString::from(time),
        favorited,
        digit: SharedString::from(digit),
        has_thumb: false,
        thumb: slint::Image::default(),
        tags: slint::ModelRc::new(slint::VecModel::from(
            tags.iter().map(|t| SharedString::from(*t)).collect::<Vec<_>>(),
        )),
        tag_more: if tags.len() > 2 { tags.len() as i32 - 2 } else { 0 },
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "light".into());
    let tag = std::env::args().nth(2).unwrap_or_else(|| "snap".into());
    let hover_y: f32 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(f32::NAN);
    let is_light = mode == "light";

    let tokens = derive_tokens(
        "default",
        "default",
        if is_light { ResolvedTheme::Light } else { ResolvedTheme::Dark },
    );

    let adapter = MinimalSoftwareWindow::new(slint::platform::software_renderer::RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SnapshotPlatform { window: adapter.clone() })).unwrap();

    let picker = QuickPasteWindow::new().unwrap();
    let theme = picker.global::<Theme>();
    theme.set_is_dark(!is_light);
    theme.set_canvas_default(hex_color(tokens.canvas_default));
    theme.set_canvas_subtle(hex_color(tokens.canvas_subtle));
    theme.set_canvas_inset(hex_color(tokens.canvas_inset));
    theme.set_surface(hex_color(tokens.surface));
    theme.set_card_layer(hex_color(tokens.card_layer));
    theme.set_card_face(hex_color(tokens.card_face));
    theme.set_fg_default(hex_color(&tokens.fg_default));
    theme.set_fg_muted(hex_color(&tokens.fg_muted));
    theme.set_fg_subtle(hex_color(&tokens.fg_subtle));
    theme.set_accent_fg(hex_color(&tokens.accent_fg));
    theme.set_accent_subtle(rgba_color(tokens.accent_subtle_rgb, tokens.accent_subtle_alpha));
    theme.set_selected_bg(hex_color(&tokens.selected_bg));
    theme.set_border_default(hex_color(&tokens.border_default));
    theme.set_border_window(hex_color(&tokens.border_window));
    theme.set_border_muted(hex_color(tokens.border_muted));
    theme.set_border_subtle(hex_color(&tokens.border_subtle));
    theme.set_danger_subtle(rgba_color(tokens.danger_subtle_rgb, tokens.danger_subtle_alpha));
    theme.set_favorite(hex_color(tokens.favorite));

    let rows: Vec<ClipRow> = vec![
        row("https://github.com/microsoft/PowerToys/blob/main/src/modules/launcher/PowerLauncher/MainWindow.xaml 深入浅出 WinUI", "链接", "Chrome", "刚刚", "1", false, vec![]),
        row("浅色模式下速贴面板的前景、背景与卡片层次对齐 WinUI 弹层列表语言：行默认透明、悬停仅底色变化", "文本", "微信", "2 分钟前", "2", true, vec!["工作", "常用"]),
        row("cargo build --release -p floatpaste-native", "文本", "Windows 终端", "8 分钟前", "3", false, vec![]),
        row("会议纪要要点：1. 速贴浅色观感走查 2. 搜索窗操作条对齐 PowerToys 3. 下个迭代排期待定", "文本", "Obsidian", "26 分钟前", "4", false, vec!["会议"]),
        row("let selected_bg = mix_colors(canvas, ink, 0.06);", "文本", "VS Code", "1 小时前", "5", false, vec![]),
        row("D:/repos/floatpaste/docs/theme-system.md", "文件", "资源管理器", "3 小时前", "6", false, vec!["文档", "主题", "规范"]),
    ];
    picker.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    picker.set_selected(1);

    picker.window().set_size(slint::PhysicalSize::new(360, 420));
    let _ = picker.show();
    if !hover_y.is_nan() {
        picker.window().dispatch_event(slint::platform::WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(180.0, hover_y),
        });
    }

    let snap = picker.window().take_snapshot().unwrap();
    let (w, h) = (snap.width(), snap.height());

    let desk = [122u8, 139, 153];
    let mut out = image::RgbaImage::new(w, h);
    for (px, src) in out.pixels_mut().zip(snap.as_slice().iter()) {
        let a = src.a as u32;
        let f = |c: u8, d: u8| ((c as u32 * a + d as u32 * (255 - a) + 127) / 255) as u8;
        *px = image::Rgba([f(src.r, desk[0]), f(src.g, desk[1]), f(src.b, desk[2]), 255]);
    }

    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.artifacts/snapshot");
    std::fs::create_dir_all(dir).unwrap();
    let path = format!("{}/picker-{}-{}.png", dir, mode, tag);
    out.save(&path).unwrap();
    println!("saved {}", path);
}
