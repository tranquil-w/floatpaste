//! 速贴观感诊断工具：无头渲染速贴窗口快照（不依赖桌面，不模拟输入）。
//!
//! 用法：`cargo run -p floatpaste-native --bin picker-snapshot -- <light|dark> <tag> [hover-y] [blur]`
//! - mode：light / dark（默认预设 token 全量写入）
//! - tag：输出文件名后缀（迭代对比用 before/after 等）
//! - hover-y：可选逻辑 y 坐标，dispatch PointerMoved 模拟行悬停
//! - blur：可选，合成彩底并走与 prepare_blur_backdrop 同一条烘焙管线
//!   （模糊→tint→圆角），预览自绘模糊底图与行卡半透观感
//! - mica[=alpha]：同 blur 但 tint 近不透明 + 行卡实卡（Mica 档观感），
//!   alpha 缺省取代码内 MICA_TINT_ALPHA
//! - backdrop=#RRGGBB：用指定纯色替代合成彩底，校准不同背景下的材质
//! - preset=<id>：使用指定风格预设（默认 default）
//! - actions=<行号>：展开该行的更多操作菜单（从 0 起）；frame=<毫秒> 取进场帧
//! - hover-x=<坐标>：与 hover-y 配合，检查行内更多按钮的悬停
//!
//! 输出 `.artifacts/snapshot/picker-<mode>-<tag>.png`（不入 Git）。
//! 实底形态快照按中灰蓝桌面色合成材质残透，近似真机观感。
//!
//! 维护约束：Theme token 写入集必须与 `theme_bridge::write_full_tokens`
//! 键集保持一致——漏写半透明 token 不报错，只静默透明（观感误判源头）。

use floatpaste_core::theme::{derive_tokens, ResolvedTheme};
use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::{Platform, PlatformError, WindowAdapter};
use slint::{ComponentHandle, Model, SharedString};

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
    let keys = floatpaste_core::domain::settings::SessionKeys::default();
    let primary = keys.confirm.as_str();
    let secondary = keys.confirm_as_file.as_str();
    ClipRow {
        id: "1".into(),
        preview: SharedString::from(preview),
        is_code: type_label == "代码",
        type_label: SharedString::from(type_label),
        source_app: SharedString::from(source),
        time_text: SharedString::from(time),
        favorited,
        digit: SharedString::from(digit),
        has_thumb: false,
        thumb: slint::Image::default(),
        tags: slint::ModelRc::new(slint::VecModel::from(
            tags.iter()
                .take(1)
                .map(|t| SharedString::from(*t))
                .collect::<Vec<_>>(),
        )),
        tag_more: tags.len().saturating_sub(1) as i32,
        actions: slint::ModelRc::new(slint::VecModel::from(
            match type_label {
                "图片" => vec![
                    ("粘贴为图片", primary),
                    ("粘贴为图片路径", secondary),
                    ("粘贴为图片文件", ""),
                ],
                "文件" => vec![
                    ("粘贴为文件", primary),
                    ("粘贴为完整路径", secondary),
                    ("粘贴为文件名", ""),
                ],
                "富文本" => vec![("粘贴为原格式", primary), ("粘贴为纯文本", secondary)],
                _ => vec![("粘贴为文本", primary)],
            }
            .into_iter()
            .map(|(label, shortcut)| PasteActionItem {
                label: label.into(),
                shortcut: shortcut.into(),
            })
            .collect::<Vec<_>>(),
        )),
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
    use_mica: bool,
    background: Option<[u8; 3]>,
    w: u32,
    h: u32,
) -> slint::Image {
    let (w, h) = (w as usize, h as usize);
    // 与 picker::prepare_blur_backdrop 同构：四周外扩一个模糊半径合成，
    // 模糊后裁回窗口尺寸（快照平台 scale=1，物理半径 = 逻辑半径）
    let m = (floatpaste_core::backdrop::BLUR_RADIUS_LOGICAL * picker.window().scale_factor())
        .round() as usize;
    let (full_w, full_h) = (w + 2 * m, h + 2 * m);
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
    let mut px = vec![0u8; full_w * full_h * 4];
    for y in 0..full_h {
        let t = ((y as isize - m as isize).max(0) as f32 / h as f32).min(1.0);
        let grad = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        for x in 0..full_w {
            let i = (y * full_w + x) * 4;
            px[i] = grad(top.0, bot.0);
            px[i + 1] = grad(top.1, bot.1);
            px[i + 2] = grad(top.2, bot.2);
            // 模拟桌面上的一扇亮窗与两块高饱和色块（窗口相对坐标），
            // 模糊后成色晕
            let (rx, ry) = (x as isize - m as isize, y as isize - m as isize);
            let spot = if (60..180).contains(&rx) && (80..200).contains(&ry) {
                Some(win)
            } else if (w as isize - 140..w as isize - 40).contains(&rx)
                && (h as isize - 220..h as isize - 120).contains(&ry)
            {
                Some(red)
            } else if (w as isize - 160..w as isize - 110).contains(&rx) && (40..90).contains(&ry) {
                Some(green)
            } else {
                None
            };
            if let Some((r, g, b)) = spot {
                px[i] = r;
                px[i + 1] = g;
                px[i + 2] = b;
            }
        }
    }
    if let Some(rgb) = background {
        for pixel in px.chunks_exact_mut(4) {
            pixel[..3].copy_from_slice(&rgb);
        }
    }
    let radius = (picker.global::<PanelGeometry>().get_card_radius()
        * picker.window().scale_factor())
    .round() as usize;
    floatpaste_core::backdrop::blur_rgba(&mut px, full_w, full_h, m);
    let mut px = floatpaste_core::backdrop::crop(&px, full_w, full_h, m, m, w, h);
    if use_mica {
        floatpaste_core::backdrop::tint_opaque(&mut px, tint_rgb, tint_alpha);
    } else {
        floatpaste_core::backdrop::tint_acrylic(&mut px, tint_rgb, !is_light);
    }
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
    let background = std::env::args().find_map(|a| {
        a.strip_prefix("backdrop=").map(|hex| {
            let color = hex_color(hex);
            [color.red(), color.green(), color.blue()]
        })
    });

    let preset = std::env::args().find_map(|a| a.strip_prefix("preset=").map(str::to_owned));
    let tokens = derive_tokens(
        preset.as_deref().unwrap_or("default"),
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
    let keys = floatpaste_core::domain::settings::SessionKeys::default();
    picker.set_delete_shortcut(keys.delete_entry.into());
    picker.set_dismiss_shortcut(keys.dismiss.into());
    let theme = picker.global::<Theme>();
    theme.set_is_dark(!is_light);
    theme.set_canvas_default(hex_color(tokens.canvas_default));
    theme.set_canvas_subtle(hex_color(tokens.canvas_subtle));
    theme.set_canvas_inset(hex_color(tokens.canvas_inset));
    theme.set_surface(hex_color(tokens.surface));
    theme.set_card_layer(hex_color(tokens.card_layer));
    theme.set_card_face(hex_color(&tokens.card_face));
    theme.set_fg_default(hex_color(&tokens.fg_default));
    theme.set_fg_muted(hex_color(&tokens.fg_muted));
    theme.set_fg_subtle(hex_color(&tokens.fg_subtle));
    theme.set_fg_metadata(hex_color(&tokens.fg_metadata));
    theme.set_accent_fg(hex_color(&tokens.accent_fg));
    theme.set_accent_subtle(rgba_color(
        tokens.accent_subtle_rgb,
        tokens.accent_subtle_alpha,
    ));
    theme.set_selected_bg(rgba_color(tokens.selected_bg_rgb, tokens.selected_bg_alpha));
    theme.set_hover_bg(rgba_color(tokens.hover_bg_rgb, tokens.hover_bg_alpha));
    theme.set_border_default(hex_color(&tokens.border_default));
    theme.set_border_window(hex_color(&tokens.border_window));
    theme.set_border_muted(hex_color(tokens.border_muted));
    theme.set_border_subtle(hex_color(&tokens.border_subtle));
    theme.set_danger_subtle(rgba_color(
        tokens.danger_subtle_rgb,
        tokens.danger_subtle_alpha,
    ));
    theme.set_danger_fg(hex_color(&tokens.danger_fg));
    theme.set_favorite(hex_color(&tokens.favorite));
    theme.set_shadow_color(rgba_color(tokens.shadow_color, 1.0));

    let mut rows: Vec<ClipRow> = vec![
        row("收到，谢谢。", "文本", "微信", "刚刚", "1", false, vec![]),
        row("浅色模式下速贴面板的前景、背景与卡片层次：行默认透明、悬停仅底色变化，玻璃底图与行卡半透叠加", "富文本", "微信", "2 分钟前", "2", true, vec!["工作", "常用"]),
        row("FloatPaste 图标\n512 × 512 · PNG", "图片", "资源管理器", "8 分钟前", "3", false, vec!["一个需要省略的长标签名称", "素材", "图标"]),
        row("会议纪要要点：1. 速贴浅色观感走查 2. 搜索窗操作条交互细节 3. 下个迭代排期待定", "文本", "Obsidian", "26 分钟前", "4", false, vec!["会议"]),
        row("let selected_bg = ink.with_alpha(is_light ? 0.12 : 0.16);", "文本", "VS Code", "1 小时前", "5", false, vec![]),
        row("D:/repos/floatpaste/docs/theme-system.md", "文件", "资源管理器", "3 小时前", "6", false, vec!["文档", "主题", "规范"]),
    ];
    rows[2].has_thumb = true;
    rows[2].thumb = slint::Image::load_from_path(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/icon-512.png"
    )))
    .unwrap();
    picker.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    picker.set_selected(1);

    // blur：Acrylic 档烘焙观感（tint 取 ACRYLIC_TINT_ALPHA）；
    // mica[=alpha]：Mica 档烘焙观感（tint 0.92 近不透明 + 行卡实卡，
    // 缺省取 MICA_TINT_ALPHA 档）
    let mica_alpha: Option<f32> =
        std::env::args().find_map(|a| a.strip_prefix("mica=").and_then(|v| v.parse::<f32>().ok()));
    let use_mica = mica_alpha.is_some() || std::env::args().any(|a| a == "mica");
    let (width, height) = (360, 420);
    let margin = picker.global::<PanelGeometry>().get_shadow_margin() as u32;
    if std::env::args().any(|a| a == "blur") || use_mica {
        picker.set_blur_backdrop(synthetic_backdrop(
            &picker,
            tokens.material_layer_rgb,
            mica_alpha.unwrap_or(if use_mica {
                floatpaste_core::backdrop::MICA_TINT_ALPHA
            } else {
                floatpaste_core::backdrop::ACRYLIC_TINT_ALPHA
            }),
            is_light,
            use_mica,
            background,
            width - 2 * margin,
            height - 2 * margin,
        ));
        picker.set_blur_active(true);
        picker.set_material_active(use_mica);
    }

    picker
        .window()
        .set_size(slint::PhysicalSize::new(width, height));
    let _ = picker.show();
    // 先渲染关闭态，让常驻浮层从 reveal=0 起算。
    let _ = picker.window().take_snapshot().unwrap();
    // 自定义平台没有事件循环：输入前也要推进时钟，避免首帧字体初始化
    // 的耗时被计入随后才触发的动画。
    slint::platform::update_timers_and_animations();
    if let Some(row) = std::env::args().find_map(|a| {
        a.strip_prefix("actions=")
            .and_then(|s| s.parse::<i32>().ok())
    }) {
        picker.set_selected(row);
        if let Some(item) = picker.get_rows().row_data(row as usize) {
            // 与 picker::open_actions 的管理项追加保持一致；bin 不依赖主程序
            // 模块树（无 lib target），暂不为这 3 项建共享层，改菜单结构时两处同步
            let mut actions: Vec<_> = item.actions.iter().collect();
            let keys = floatpaste_core::domain::settings::SessionKeys::default();
            for (label, shortcut) in [
                (
                    if item.favorited {
                        "取消收藏"
                    } else {
                        "收藏"
                    },
                    keys.toggle_favorite,
                ),
                ("编辑", keys.open_editor),
                ("删除", keys.delete_entry),
            ] {
                actions.push(PasteActionItem {
                    label: label.into(),
                    shortcut: shortcut.into(),
                });
            }
            picker.set_menu_actions(slint::ModelRc::new(slint::VecModel::from(actions)));
        }
        picker.set_actions_row(row);
    }
    if !hover_y.is_nan() {
        picker
            .window()
            .dispatch_event(slint::platform::WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(
                    std::env::args()
                        .find_map(|a| a.strip_prefix("hover-x=").and_then(|s| s.parse().ok()))
                        .unwrap_or(180.0),
                    hover_y + picker.global::<PanelGeometry>().get_shadow_margin(),
                ),
            });
    }

    let _ = picker.window().take_snapshot().unwrap();
    if let Some(ms) =
        std::env::args().find_map(|a| a.strip_prefix("frame=").and_then(|s| s.parse::<u64>().ok()))
    {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(1000)));
        slint::platform::update_timers_and_animations();
    }
    let snap = picker.window().take_snapshot().unwrap();
    let (w, h) = (snap.width(), snap.height());

    let desk = background.unwrap_or([122u8, 139, 153]);
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
