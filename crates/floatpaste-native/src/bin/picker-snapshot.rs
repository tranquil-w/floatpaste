//! 速贴观感诊断工具：无头渲染速贴窗口快照（不依赖桌面，不模拟输入）。
//!
//! 用法：`cargo run -p floatpaste-native --bin picker-snapshot -- <light|dark> <tag> [hover-y] [blur]`
//! - mode：light / dark（默认预设 token 全量写入）
//! - tag：输出文件名后缀（迭代对比用 before/after 等）
//! - hover-y：可选逻辑 y 坐标，dispatch PointerMoved 模拟行悬停
//! - blur：可选，合成彩底并走与 prepare_blur_backdrop 同一条烘焙管线
//!   （模糊→tint→圆角），预览自绘模糊底图与行卡半透观感
//!
//! 输出 `.artifacts/snapshot/picker-<mode>-<tag>.png`（不入 Git）。
//! 实底形态快照按中灰蓝桌面色合成材质残透，近似真机观感。
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
    let ch = |r: std::ops::Range<usize>| {
        u8::from_str_radix(std::str::from_utf8(&b[r]).unwrap(), 16).unwrap()
    };
    slint::Color::from_rgb_u8(ch(1..3), ch(3..5), ch(5..7))
}

fn rgba_color(rgb: [u8; 3], alpha: f32) -> slint::Color {
    slint::Color::from_argb_encoded(
        ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32) << 24
            | (rgb[0] as u32) << 16
            | (rgb[1] as u32) << 8
            | (rgb[2] as u32),
    )
}

fn row(
    preview: &str,
    type_label: &str,
    source: &str,
    time: &str,
    digit: &str,
    favorited: bool,
    tags: Vec<&str>,
) -> ClipRow {
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
            tags.iter()
                .map(|t| SharedString::from(*t))
                .collect::<Vec<_>>(),
        )),
        tag_more: if tags.len() > 2 {
            tags.len() as i32 - 2
        } else {
            0
        },
    }
}

/// 合成模糊底图（blur 模式）：手工画一张「桌面感」彩底，走与
/// picker::prepare_blur_backdrop 同一条烘焙管线（模糊→tint→圆角）。
/// 不抓真实桌面，只预览底图与行卡半透的合成观感；深色态用暗桌面
/// 底色（亮底会让 5% 白墨玻璃卡失真）
fn synthetic_backdrop(
    picker: &QuickPasteWindow,
    tint_rgb: [u8; 3],
    tint_alpha: f32,
    is_light: bool,
    w: u32,
    h: u32,
) -> slint::Image {
    let (w, h) = (w as usize, h as usize);
    // 上->下渐变首末色 + 亮窗/红块/绿块三个色斑（模糊后成色晕）
    let (top, bot, win, red, green) = if is_light {
        (
            (58, 96, 158),
            (142, 120, 96),
            (235, 240, 250),
            (210, 90, 70),
            (80, 170, 90),
        )
    } else {
        (
            (26, 38, 66),
            (62, 50, 40),
            (150, 155, 165),
            (110, 50, 40),
            (45, 90, 55),
        )
    };
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        let t = y as f32 / h as f32;
        let grad = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        for x in 0..w {
            let i = (y * w + x) * 4;
            px[i] = grad(top.0, bot.0);
            px[i + 1] = grad(top.1, bot.1);
            px[i + 2] = grad(top.2, bot.2);
            // 模拟桌面上的一扇亮窗与两块高饱和色块，模糊后成色晕
            if (60..180).contains(&x) && (80..200).contains(&y) {
                px[i] = win.0;
                px[i + 1] = win.1;
                px[i + 2] = win.2;
            }
            if (w - 140..w - 40).contains(&x) && (h - 220..h - 120).contains(&y) {
                px[i] = red.0;
                px[i + 1] = red.1;
                px[i + 2] = red.2;
            }
            if (w - 160..w - 110).contains(&x) && (40..90).contains(&y) {
                px[i] = green.0;
                px[i + 1] = green.1;
                px[i + 2] = green.2;
            }
        }
    }
    let radius = (picker.global::<PanelGeometry>().get_card_radius()
        * picker.window().scale_factor())
    .round() as usize;
    floatpaste_core::backdrop::blur_rgba(&mut px, w, h, 12);
    floatpaste_core::backdrop::tint_opaque(&mut px, tint_rgb, tint_alpha);
    floatpaste_core::backdrop::bake_round_corners(&mut px, w, h, radius);
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w as u32, h as u32);
    buf.make_mut_bytes().copy_from_slice(&px);
    slint::Image::from_rgba8(buf)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "light".into());
    let tag = std::env::args().nth(2).unwrap_or_else(|| "snap".into());
    let hover_y: f32 = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(f32::NAN);
    let is_light = mode == "light";

    let tokens = derive_tokens(
        "default",
        "default",
        if is_light {
            ResolvedTheme::Light
        } else {
            ResolvedTheme::Dark
        },
    );

    let adapter = MinimalSoftwareWindow::new(
        slint::platform::software_renderer::RepaintBufferType::NewBuffer,
    );
    slint::platform::set_platform(Box::new(SnapshotPlatform {
        window: adapter.clone(),
    }))
    .unwrap();

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
    theme.set_accent_subtle(rgba_color(
        tokens.accent_subtle_rgb,
        tokens.accent_subtle_alpha,
    ));
    theme.set_selected_bg(rgba_color(tokens.selected_bg_rgb, tokens.selected_bg_alpha));
    theme.set_border_default(hex_color(&tokens.border_default));
    theme.set_border_window(hex_color(&tokens.border_window));
    theme.set_border_muted(hex_color(tokens.border_muted));
    theme.set_border_subtle(hex_color(&tokens.border_subtle));
    theme.set_danger_subtle(rgba_color(
        tokens.danger_subtle_rgb,
        tokens.danger_subtle_alpha,
    ));
    theme.set_favorite(hex_color(tokens.favorite));
    theme.set_shadow_color(rgba_color(tokens.shadow_color, 1.0));

    let rows: Vec<ClipRow> = vec![
        row("https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getcaretpos 深入浅出插入符定位", "链接", "Chrome", "刚刚", "1", false, vec![]),
        row("浅色模式下速贴面板的前景、背景与卡片层次：行默认透明、悬停仅底色变化，玻璃底图与行卡半透叠加", "文本", "微信", "2 分钟前", "2", true, vec!["工作", "常用"]),
        row("cargo build --release -p floatpaste-native", "文本", "Windows 终端", "8 分钟前", "3", false, vec![]),
        row("会议纪要要点：1. 速贴浅色观感走查 2. 搜索窗操作条交互细节 3. 下个迭代排期待定", "文本", "Obsidian", "26 分钟前", "4", false, vec!["会议"]),
        row("let selected_bg = ink.with_alpha(is_light ? 0.12 : 0.16);", "文本", "VS Code", "1 小时前", "5", false, vec![]),
        row("D:/repos/floatpaste/docs/theme-system.md", "文件", "资源管理器", "3 小时前", "6", false, vec!["文档", "主题", "规范"]),
    ];
    picker.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    picker.set_selected(1);

    if std::env::args().any(|a| a == "blur") {
        picker.set_blur_backdrop(synthetic_backdrop(
            &picker,
            tokens.material_layer_rgb,
            tokens.material_layer_alpha,
            is_light,
            360,
            420,
        ));
        picker.set_blur_active(true);
    }

    picker.window().set_size(slint::PhysicalSize::new(360, 420));
    let _ = picker.show();
    if !hover_y.is_nan() {
        picker
            .window()
            .dispatch_event(slint::platform::WindowEvent::PointerMoved {
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
