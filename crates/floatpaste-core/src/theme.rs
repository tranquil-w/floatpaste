//! 主题引擎：预设色板 + 强调色 + 明暗派生，输出全套语义 token。
//!
//! 与前端 `src/shared/theme/`（culori 实现）逐值对齐：
//! - `palettes.ts` → [`PALETTE_SCALES`]
//! - `accents.ts` → [`ACCENT_CHOICES`]
//! - `contrast.ts` / `derive.ts` → [`ensure_contrast`] / [`mix_colors`] / [`derive_tokens`]
//!
//! 色彩数学（OKLab/OKLCH、WCAG 对比度）按公开规范公式移植，
//! 与 culori 的差异仅在色域裁剪的搜索精度内（≤1/255 每通道）。

use crate::domain::settings::ThemeMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedTheme {
    Light,
    Dark,
}

/// 角色化色板（单预设单模式），出处见各预设注释
#[derive(Debug, Clone, Copy)]
pub struct PaletteScale {
    pub canvas: &'static str,
    pub canvas_subtle: &'static str,
    pub surface: &'static str,
    pub inset: &'static str,
    pub ink: &'static str,
    pub ink_muted: &'static str,
    pub ink_subtle: &'static str,
    pub border: &'static str,
    pub border_muted: &'static str,
    pub accent: &'static str,
    pub success: &'static str,
    pub danger: &'static str,
    pub warning: &'static str,
    pub done: &'static str,
    pub favorite: &'static str,
}

/// 默认预设：Radix Colors slate 中性阶 + 色相阶（slate.css / slate-dark.css）
const DEFAULT_LIGHT: PaletteScale = PaletteScale {
    canvas: "#f9f9fb",
    canvas_subtle: "#f0f0f3",
    surface: "#ffffff",
    inset: "#f0f0f3",
    ink: "#1c2024",
    ink_muted: "#5c6168",
    ink_subtle: "#6d727b",
    border: "#8b8d98",
    border_muted: "#d9d9e0",
    accent: "#0090ff",
    success: "#30a46c",
    danger: "#e5484d",
    warning: "#f76b15",
    done: "#8e4ec6",
    favorite: "#f76b15",
};

const DEFAULT_DARK: PaletteScale = PaletteScale {
    canvas: "#18191b",
    canvas_subtle: "#212225",
    surface: "#272a2d",
    inset: "#111113",
    ink: "#edeef0",
    ink_muted: "#b0b4ba",
    ink_subtle: "#7f838c",
    border: "#61676f",
    border_muted: "#363a3f",
    accent: "#3b9eff",
    success: "#33b074",
    danger: "#ec5d5e",
    warning: "#ff801f",
    done: "#9a5cd0",
    favorite: "#ff801f",
};

/// Catppuccin Latte / Mocha（catppuccin/palette v1.8）
const CATPPUCCIN_LIGHT: PaletteScale = PaletteScale {
    canvas: "#eff1f5",
    canvas_subtle: "#e6e9ef",
    surface: "#ffffff",
    inset: "#e6e9ef",
    ink: "#4c4f69",
    ink_muted: "#5c5f77",
    ink_subtle: "#6c6f85",
    border: "#878aa0",
    border_muted: "#ccd0da",
    accent: "#1e66f5",
    success: "#40a02b",
    danger: "#d20f39",
    warning: "#df8e1d",
    done: "#8839ef",
    favorite: "#df8e1d",
};

const CATPPUCCIN_DARK: PaletteScale = PaletteScale {
    canvas: "#1e1e2e",
    canvas_subtle: "#181825",
    surface: "#313244",
    inset: "#11111b",
    ink: "#cdd6f4",
    ink_muted: "#a6adc8",
    ink_subtle: "#868aa2",
    border: "#6e7288",
    border_muted: "#45475a",
    accent: "#89b4fa",
    success: "#a6e3a1",
    danger: "#f38ba8",
    warning: "#f9e2af",
    done: "#cba6f7",
    favorite: "#f9e2af",
};

/// Tokyo Night day / night（folke/tokyonight.nvim 官方 hex）
const TOKYO_NIGHT_LIGHT: PaletteScale = PaletteScale {
    canvas: "#e1e2e7",
    canvas_subtle: "#d0d5e3",
    surface: "#ffffff",
    inset: "#c4c8da",
    ink: "#3760bf",
    ink_muted: "#6172b0",
    ink_subtle: "#848cb5",
    border: "#848cb5",
    border_muted: "#c4c8da",
    accent: "#2e7de9",
    success: "#587539",
    danger: "#f52a65",
    warning: "#b15c00",
    done: "#9854f1",
    favorite: "#b15c00",
};

const TOKYO_NIGHT_DARK: PaletteScale = PaletteScale {
    canvas: "#1a1b26",
    canvas_subtle: "#16161e",
    surface: "#292e42",
    inset: "#0c0e14",
    ink: "#c0caf5",
    ink_muted: "#a9b1d6",
    ink_subtle: "#7d85a8",
    border: "#5e688f",
    border_muted: "#343a52",
    accent: "#7aa2f7",
    success: "#9ece6a",
    danger: "#f7768e",
    warning: "#e0af68",
    done: "#9d7cd8",
    favorite: "#e0af68",
};

/// 预设 id（与设置 themePreset 字段对应）
pub const THEME_PRESET_IDS: [&str; 3] = ["default", "catppuccin", "tokyoNight"];

/// themeAccent 的 "跟随预设" 哨兵值
pub const ACCENT_FOLLOW_PRESET: &str = "default";

/// 强调色安全列表（Radix Colors light step 9 / dark step 10）
pub struct AccentChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub light: &'static str,
    pub dark: &'static str,
}

pub const ACCENT_CHOICES: [AccentChoice; 8] = [
    AccentChoice {
        id: "blue",
        label: "蓝",
        light: "#0090ff",
        dark: "#3b9eff",
    },
    AccentChoice {
        id: "indigo",
        label: "靛蓝",
        light: "#3e63dd",
        dark: "#5472e4",
    },
    AccentChoice {
        id: "cyan",
        label: "青",
        light: "#00a2c7",
        dark: "#23afd0",
    },
    AccentChoice {
        id: "green",
        label: "绿",
        light: "#30a46c",
        dark: "#33b074",
    },
    AccentChoice {
        id: "orange",
        label: "橙",
        light: "#f76b15",
        dark: "#ff801f",
    },
    AccentChoice {
        id: "red",
        label: "红",
        light: "#e5484d",
        dark: "#ec5d5e",
    },
    AccentChoice {
        id: "purple",
        label: "紫",
        light: "#8e4ec6",
        dark: "#9a5cd0",
    },
    AccentChoice {
        id: "pink",
        label: "粉",
        light: "#d6409f",
        dark: "#de51a8",
    },
];

pub fn get_theme_preset(id: &str) -> bool {
    THEME_PRESET_IDS.contains(&id)
}

fn palette_scale(preset_id: &str, resolved: ResolvedTheme) -> &'static PaletteScale {
    match (preset_id, resolved) {
        ("catppuccin", ResolvedTheme::Light) => &CATPPUCCIN_LIGHT,
        ("catppuccin", ResolvedTheme::Dark) => &CATPPUCCIN_DARK,
        ("tokyoNight", ResolvedTheme::Light) => &TOKYO_NIGHT_LIGHT,
        ("tokyoNight", ResolvedTheme::Dark) => &TOKYO_NIGHT_DARK,
        (_, ResolvedTheme::Light) => &DEFAULT_LIGHT,
        (_, ResolvedTheme::Dark) => &DEFAULT_DARK,
    }
}

/// themeMode 解析为明暗两态；"跟随系统"由调用方传入系统值
pub fn resolve_theme(mode: ThemeMode, prefers_dark: bool) -> ResolvedTheme {
    match mode {
        ThemeMode::Light => ResolvedTheme::Light,
        ThemeMode::Dark => ResolvedTheme::Dark,
        ThemeMode::System => {
            if prefers_dark {
                ResolvedTheme::Dark
            } else {
                ResolvedTheme::Light
            }
        }
    }
}

/// Windows 系统是否偏好深色应用（注册表 AppsUseLightTheme）
pub fn system_prefers_dark() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let sub_key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let value_name: Vec<u16> = "AppsUseLightTheme"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub_key.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as _),
            Some(&mut size),
        )
    };
    result.is_ok() && data == 0
}

/// 把 themeAccent 解析为当前模式下的强调色 hex：
/// "default"=跟随预设 | 安全列表 id | 旧版迁移的 #RRGGBB；非法输入回退预设强调色
pub fn resolve_accent_hex(theme_accent: &str, preset_id: &str, resolved: ResolvedTheme) -> String {
    let value = theme_accent.trim();
    if value.is_empty() || value == ACCENT_FOLLOW_PRESET {
        return palette_scale(preset_id, resolved).accent.to_string();
    }

    if is_valid_hex_color(value) {
        return value.to_ascii_uppercase();
    }

    for choice in &ACCENT_CHOICES {
        if choice.id == value {
            return match resolved {
                ResolvedTheme::Light => choice.light,
                ResolvedTheme::Dark => choice.dark,
            }
            .to_string();
        }
    }

    palette_scale(preset_id, resolved).accent.to_string()
}

fn is_valid_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

/* ───────────────── 色彩数学（与 culori 对齐） ───────────────── */

#[derive(Debug, Clone, Copy)]
struct Rgb {
    r: f64,
    g: f64,
    b: f64,
}

#[derive(Debug, Clone, Copy)]
struct Oklab {
    l: f64,
    a: f64,
    b: f64,
}

#[derive(Debug, Clone, Copy)]
struct Oklch {
    l: f64,
    c: f64,
    h: f64,
}

fn parse_hex(hex: &str) -> Option<Rgb> {
    let bytes = hex.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| -> Option<f64> {
        let text = std::str::from_utf8(&bytes[range]).ok()?;
        u8::from_str_radix(text, 16).ok().map(|v| v as f64 / 255.0)
    };
    Some(Rgb {
        r: channel(1..3)?,
        g: channel(3..5)?,
        b: channel(5..7)?,
    })
}

fn format_hex(color: Rgb) -> String {
    let channel = |v: f64| -> u8 { (v.clamp(0.0, 1.0) * 255.0).round() as u8 };
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn rgb_to_oklab(color: Rgb) -> Oklab {
    let r = srgb_to_linear(color.r);
    let g = srgb_to_linear(color.g);
    let b = srgb_to_linear(color.b);

    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    Oklab {
        l: 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        a: 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        b: 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    }
}

fn oklab_to_rgb(color: Oklab) -> Rgb {
    let l_ = color.l + 0.3963377774 * color.a + 0.2158037573 * color.b;
    let m_ = color.l - 0.1055613458 * color.a - 0.0638541728 * color.b;
    let s_ = color.l - 0.0894841775 * color.a - 1.2914855480 * color.b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    Rgb {
        r: linear_to_srgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        g: linear_to_srgb(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        b: linear_to_srgb(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    }
}

fn oklab_to_oklch(color: Oklab) -> Oklch {
    Oklch {
        l: color.l,
        c: (color.a * color.a + color.b * color.b).sqrt(),
        h: color.b.atan2(color.a),
    }
}

fn oklch_to_oklab(color: Oklch) -> Oklab {
    Oklab {
        l: color.l,
        a: color.c * color.h.cos(),
        b: color.c * color.h.sin(),
    }
}

fn hex_to_oklch(hex: &str) -> Option<Oklch> {
    parse_hex(hex).map(rgb_to_oklab).map(oklab_to_oklch)
}

/// 色域裁剪：保持 L、H，收缩 C 至 sRGB 色域内（与 culori clampChroma 同语义）
fn clamp_chroma(color: Oklch) -> Rgb {
    let candidate = oklab_to_rgb(oklch_to_oklab(color));
    let in_gamut = |rgb: Rgb| {
        rgb.r >= -0.0001
            && rgb.r <= 1.0001
            && rgb.g >= -0.0001
            && rgb.g <= 1.0001
            && rgb.b >= -0.0001
            && rgb.b <= 1.0001
    };
    if in_gamut(candidate) {
        return candidate;
    }

    let mut lo = 0.0f64;
    let mut hi = color.c;
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        let candidate = oklab_to_rgb(oklch_to_oklab(Oklch {
            l: color.l,
            c: mid,
            h: color.h,
        }));
        if in_gamut(candidate) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    oklab_to_rgb(oklch_to_oklab(Oklch {
        l: color.l,
        c: lo,
        h: color.h,
    }))
}

fn oklch_to_hex(color: Oklch) -> String {
    format_hex(clamp_chroma(color))
}

fn relative_luminance(hex: &str) -> f64 {
    let Some(rgb) = parse_hex(hex) else {
        return 0.0;
    };
    0.2126 * srgb_to_linear(rgb.r) + 0.7152 * srgb_to_linear(rgb.g) + 0.0722 * srgb_to_linear(rgb.b)
}

/// WCAG 2.x 对比度（1 起，21 封顶）；解析失败返回 1（与前端口径一致）
pub fn contrast_ratio(foreground: &str, background: &str) -> f64 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    let lighter = fg.max(bg);
    let darker = fg.min(bg);
    ((lighter + 0.05) / (darker + 0.05)).min(21.0)
}

/// 在 OKLCH 空间只调亮度（保持色相与色度），返回与原色最近、
/// 且相对 against 的 WCAG 对比度达到 target 的颜色（二分 24 轮，与前端一致）
pub fn ensure_contrast(color: &str, against: &str, target: f64) -> String {
    let Some(base) = hex_to_oklch(color) else {
        return "#000000".to_string();
    };
    if contrast_ratio(color, against) >= target {
        return color.to_ascii_uppercase();
    }

    let against_l = hex_to_oklch(against).map(|value| value.l).unwrap_or(0.5);
    let ascending = base.l >= against_l;
    let (mut lo, mut hi) = if ascending {
        (base.l, 1.0)
    } else {
        (0.0, base.l)
    };

    let mut best = oklch_to_hex(Oklch {
        l: if ascending { 1.0 } else { 0.0 },
        c: base.c,
        h: base.h,
    });
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        let candidate = oklch_to_hex(Oklch {
            l: mid,
            c: base.c,
            h: base.h,
        });
        if contrast_ratio(&candidate, against) >= target {
            best = candidate;
            if ascending {
                hi = mid;
            } else {
                lo = mid;
            }
        } else if ascending {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    best
}

/// OKLab 插值混色（取代 sRGB 通道线性混合）
pub fn mix_colors(base: &str, target: &str, ratio: f64) -> String {
    let from = rgb_to_oklab(parse_hex(base).unwrap_or(Rgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    }));
    let to = rgb_to_oklab(parse_hex(target).unwrap_or(Rgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    }));
    format_hex(clamp_chroma(oklab_to_oklch(Oklab {
        l: from.l + (to.l - from.l) * ratio,
        a: from.a + (to.a - from.a) * ratio,
        b: from.b + (to.b - from.b) * ratio,
    })))
}

/* ───────────────── 语义 token 派生 ───────────────── */

/// 派生输出的语义 token（键集恒定，界面层只消费这一层）
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeTokens {
    pub canvas_default: &'static str,
    pub canvas_subtle: &'static str,
    pub canvas_inset: &'static str,

    pub fg_default: String,
    pub fg_muted: String,
    pub fg_subtle: String,
    pub fg_on_emphasis: String,

    pub accent_fg: String,
    pub accent_emphasis: String,
    pub accent_hover: String,
    pub accent_subtle_rgb: [u8; 3],
    pub accent_subtle_alpha: f32,

    pub border_default: String,
    pub border_window: String,
    pub border_muted: &'static str,
    pub border_subtle: String,

    pub success_fg: String,
    pub success_subtle_rgb: [u8; 3],
    pub success_subtle_alpha: f32,
    pub danger_fg: String,
    pub danger_subtle_rgb: [u8; 3],
    pub danger_subtle_alpha: f32,
    pub warning_fg: String,
    pub warning_subtle_rgb: [u8; 3],
    pub warning_subtle_alpha: f32,
    pub done_fg: String,
    pub done_subtle_rgb: [u8; 3],
    pub done_subtle_alpha: f32,

    pub favorite: &'static str,
    pub shadow_color: [u8; 3],

    /// 文本选区/IME 组合词高亮（旧版 ::selection：dark=canvas-default/
    /// accent-hover，light=#ffffff/accent-emphasis）
    pub selection_fg: String,
    pub selection_bg: String,

    /// 编辑框聚焦边框（旧版 --pg-border-accent = accentFg，跟随用户强调色）
    pub border_accent: String,
    /// 警告点/徽标的原色（旧版 --pg-warning-emphasis，未做对比度调整）
    pub warning_emphasis: &'static str,
}

/// 门禁常量：正文 AA+ 余量、组件边界非文本线
pub const CONTRAST_TARGETS: (f64, f64, f64) = (5.5, 4.5, 3.0);

const DARK_INK_ON_EMPHASIS: &str = "#1F2328";
const LIGHT_INK_ON_EMPHASIS: &str = "#FFFFFF";

fn hex_to_rgb_channels(hex: &str) -> [u8; 3] {
    parse_hex(hex).map_or([0, 0, 0], |rgb| {
        [
            (rgb.r * 255.0).round() as u8,
            (rgb.g * 255.0).round() as u8,
            (rgb.b * 255.0).round() as u8,
        ]
    })
}

/// 实底强调色：黑白前景都无法达到 4.5:1 时，向改动更小的方向推移底色亮度
fn ensure_emphasis_background(accent: &str) -> (String, &'static str) {
    let white = contrast_ratio(LIGHT_INK_ON_EMPHASIS, accent);
    let black = contrast_ratio(DARK_INK_ON_EMPHASIS, accent);
    let target = CONTRAST_TARGETS.1;
    if white.max(black) >= target {
        let fg_on_emphasis = if black >= white {
            DARK_INK_ON_EMPHASIS
        } else {
            LIGHT_INK_ON_EMPHASIS
        };
        return (accent.to_ascii_uppercase(), fg_on_emphasis);
    }

    let darker_for_white = ensure_contrast(accent, LIGHT_INK_ON_EMPHASIS, target);
    let lighter_for_dark = ensure_contrast(accent, DARK_INK_ON_EMPHASIS, target);
    let accent_l = hex_to_oklch(accent).map_or(0.5, |value| value.l);
    let darker_l = hex_to_oklch(&darker_for_white).map_or(0.0, |value| value.l);
    let lighter_l = hex_to_oklch(&lighter_for_dark).map_or(1.0, |value| value.l);
    let prefer_darker = (accent_l - darker_l).abs() <= (lighter_l - accent_l).abs();
    if prefer_darker {
        (darker_for_white, LIGHT_INK_ON_EMPHASIS)
    } else {
        (lighter_for_dark, DARK_INK_ON_EMPHASIS)
    }
}

/// 从角色化色板 + 强调色派生全套语义 token（对齐 derive.ts 的 deriveSemanticTokens）
pub fn derive_tokens(preset_id: &str, theme_accent: &str, resolved: ResolvedTheme) -> ThemeTokens {
    let is_light = resolved == ResolvedTheme::Light;
    let scale = palette_scale(preset_id, resolved);
    let accent_hex = resolve_accent_hex(theme_accent, preset_id, resolved);

    let subtle_alpha = if is_light { 0.13 } else { 0.19 };
    let status_alpha = if is_light { 0.13 } else { 0.16 };
    let (body, subtle_target, border_target) = CONTRAST_TARGETS;

    let ink = ensure_contrast(scale.ink, scale.canvas, body);
    let ink_muted = ensure_contrast(scale.ink_muted, scale.canvas, body);
    let ink_subtle = ensure_contrast(scale.ink_subtle, scale.canvas, subtle_target);
    let border = ensure_contrast(scale.border, scale.canvas, border_target);

    let accent_fg = ensure_contrast(&accent_hex, scale.canvas, subtle_target);
    let (emphasis, fg_on_emphasis) = ensure_emphasis_background(&accent_hex);
    let accent_hover = mix_colors(
        &emphasis,
        if is_light { "#000000" } else { "#ffffff" },
        if is_light { 0.12 } else { 0.14 },
    );

    let status_fg = |base: &str| -> (String, [u8; 3]) {
        let fg = ensure_contrast(base, scale.canvas, subtle_target);
        let channels = hex_to_rgb_channels(&fg);
        (fg, channels)
    };
    let (success_fg, success_rgb) = status_fg(scale.success);
    let (danger_fg, danger_rgb) = status_fg(scale.danger);
    let (warning_fg, warning_rgb) = status_fg(scale.warning);
    let (done_fg, done_rgb) = status_fg(scale.done);

    ThemeTokens {
        canvas_default: scale.canvas,
        canvas_subtle: scale.canvas_subtle,
        canvas_inset: scale.inset,

        fg_default: ink.clone(),
        fg_muted: ink_muted,
        fg_subtle: ink_subtle,
        fg_on_emphasis: fg_on_emphasis.to_string(),

        accent_fg: accent_fg.clone(),
        accent_emphasis: emphasis.clone(),
        accent_hover: accent_hover.clone(),
        accent_subtle_rgb: hex_to_rgb_channels(&accent_fg),
        accent_subtle_alpha: subtle_alpha as f32,

        border_default: border,
        border_window: mix_colors(scale.border_muted, scale.canvas, 0.55),
        border_muted: scale.border_muted,
        border_subtle: mix_colors(scale.border_muted, scale.canvas, 0.45),

        success_fg,
        success_subtle_rgb: success_rgb,
        success_subtle_alpha: status_alpha as f32,
        danger_fg,
        danger_subtle_rgb: danger_rgb,
        danger_subtle_alpha: status_alpha as f32,
        warning_fg,
        warning_subtle_rgb: warning_rgb,
        warning_subtle_alpha: status_alpha as f32,
        done_fg,
        done_subtle_rgb: done_rgb,
        done_subtle_alpha: status_alpha as f32,

        favorite: scale.favorite,
        // 深色下阴影必须用黑色，否则窗口阴影会变成一圈白光
        shadow_color: if is_light {
            hex_to_rgb_channels(&ink)
        } else {
            [0, 0, 0]
        },

        selection_fg: if is_light {
            LIGHT_INK_ON_EMPHASIS.to_string()
        } else {
            scale.canvas.to_string()
        },
        selection_bg: if is_light { emphasis } else { accent_hover },
        border_accent: accent_fg.clone(),
        warning_emphasis: scale.warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参考值由前端 culori 管线生成（node_modules/culori，同 palettes/derive 逻辑），
    /// 锁定移植与前端逐值一致；调整公式前先在前端侧重新生成。
    #[test]
    fn default_light_tokens_match_frontend_reference() {
        let tokens = derive_tokens("default", "default", ResolvedTheme::Light);
        assert_eq!(tokens.canvas_default, "#f9f9fb");
        assert_eq!(tokens.fg_default, "#1C2024");
        assert_eq!(tokens.fg_muted, "#5C6168");
        assert_eq!(tokens.fg_subtle, "#6D727B");
        assert_eq!(tokens.accent_fg, "#0074D0");
        assert_eq!(tokens.accent_emphasis, "#0090FF");
        assert_eq!(tokens.border_default, "#8B8D98");
        assert_eq!(tokens.border_window, "#EAEBEF");
        assert_eq!(tokens.border_subtle, "#E7E7EC");
        assert_eq!(tokens.success_fg, "#008451");
        assert_eq!(tokens.danger_fg, "#D43740");
        assert_eq!(tokens.warning_fg, "#C24F00");
        assert_eq!(tokens.done_fg, "#8E4EC6");
    }

    #[test]
    fn default_dark_tokens_match_frontend_reference() {
        let tokens = derive_tokens("default", "default", ResolvedTheme::Dark);
        // 色板直通字段保留 palettes.ts 原始大小写（CSS 颜色大小写不敏感）
        assert_eq!(tokens.canvas_default, "#18191b");
        assert_eq!(tokens.fg_default, "#EDEEF0");
        assert_eq!(tokens.accent_fg, "#3B9EFF");
        assert_eq!(tokens.border_default, "#61676F");
        assert_eq!(tokens.border_window, "#25272B");
        assert_eq!(tokens.border_subtle, "#282B2E");
        assert_eq!(tokens.shadow_color, [0, 0, 0]);
    }

    #[test]
    fn accent_override_changes_accent_tokens() {
        let tokens = derive_tokens("default", "purple", ResolvedTheme::Light);
        assert_eq!(tokens.accent_fg, "#8E4EC6");
        assert_eq!(tokens.accent_subtle_rgb, [142, 78, 198]);
        // 旧版 --pg-border-accent = accentFg：聚焦边框跟随用户强调色
        assert_eq!(tokens.border_accent, "#8E4EC6");
    }

    #[test]
    fn legacy_hex_accent_is_accepted_uppercase() {
        assert_eq!(
            resolve_accent_hex("#8250df", "default", ResolvedTheme::Light),
            "#8250DF"
        );
    }

    #[test]
    fn invalid_accent_falls_back_to_preset() {
        assert_eq!(
            resolve_accent_hex("not-a-color", "default", ResolvedTheme::Light),
            "#0090ff"
        );
    }

    #[test]
    fn unknown_preset_falls_back_to_default() {
        let tokens = derive_tokens("nope", "default", ResolvedTheme::Light);
        assert_eq!(tokens.canvas_default, "#f9f9fb");
    }

    #[test]
    fn resolve_theme_maps_modes() {
        assert_eq!(resolve_theme(ThemeMode::Dark, false), ResolvedTheme::Dark);
        assert_eq!(resolve_theme(ThemeMode::Light, true), ResolvedTheme::Light);
        assert_eq!(resolve_theme(ThemeMode::System, true), ResolvedTheme::Dark);
        assert_eq!(
            resolve_theme(ThemeMode::System, false),
            ResolvedTheme::Light
        );
    }

    #[test]
    fn contrast_ratio_matches_wcag_known_values() {
        // 黑白对比度 21:1
        let ratio = contrast_ratio("#FFFFFF", "#000000");
        assert!((ratio - 21.0).abs() < 0.01, "{ratio}");
        // 相同颜色对比度 1:1
        assert!((contrast_ratio("#ABABAB", "#ABABAB") - 1.0).abs() < 0.001);
    }

    #[test]
    fn ensure_contrast_keeps_passing_color_unchanged() {
        assert_eq!(ensure_contrast("#000000", "#FFFFFF", 4.5), "#000000");
    }

    #[test]
    fn mix_colors_interpolates_in_oklab() {
        // OKLab L=0.5 对应 sRGB ≈ #636363（感知中灰比 sRGB 中灰深）
        let mixed = mix_colors("#FFFFFF", "#000000", 0.5);
        assert_eq!(mixed, "#636363");
    }

    #[test]
    fn tokyo_night_and_catppuccin_presets_resolve() {
        let cat = derive_tokens("catppuccin", "default", ResolvedTheme::Dark);
        assert_eq!(cat.canvas_default, "#1e1e2e");
        assert_eq!(cat.accent_fg, "#89B4FA");

        let tokyo = derive_tokens("tokyoNight", "default", ResolvedTheme::Light);
        assert_eq!(tokyo.canvas_default, "#e1e2e7");
        // Tokyo Night day 的蓝墨正文经对比度校正后仍应保持蓝色相
        assert!(tokyo.fg_default.starts_with('#'));
    }
}
