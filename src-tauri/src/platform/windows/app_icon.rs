//! 应用图标的 Windows 原生加载。
//!
//! Tauri 上游把 ICO 只解码为单一 RGBA 位图，窗口与托盘所有场景复用一张图，
//! 由系统二次拉伸，在高 DPI 缩放下模糊（上游 tauri#14596，修复未发布）。
//! 这里绕开该路径：
//! - 窗口：从 exe 内嵌图标资源（tauri-build 固定资源 ID 32512）按当前 DPI
//!   的小/大图标尺寸分别加载 HICON 后 WM_SETICON，Windows 会从 ICO 组挑选
//!   最接近的条目，拉伸量最小；
//! - 托盘：把内嵌高分辨率 PNG 预乘缩放到托盘精确尺寸，HICON 原生尺寸即
//!   绘制尺寸，不再被系统拉伸。

use std::sync::OnceLock;

use tauri::image::Image;
use tauri::WebviewWindow;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SendMessageW, HICON, IMAGE_ICON, LR_DEFAULTSIZE, SM_CXICON,
    SM_CXSMICON, SM_CYICON, SM_CYSMICON, SYSTEM_METRICS_INDEX, WM_SETICON,
};

use crate::domain::error::AppError;

/// tauri-build 把窗口图标嵌入 exe 资源时固定使用的资源 ID。
const ICON_RESOURCE_ID: u16 = 32512;

/// 托盘图标源图：16px 小尺寸规格的 8x 超采样画布（无光斑/投影，与 ICO 小尺寸
/// 条目同一设计）。不用 512 母图——母图的左上光斑在托盘尺寸下会把贴片左半
/// 照亮，白色面板右边缘对比度过强，视觉上内部图形右偏。
const ICON_PNG_BYTES: &[u8] = include_bytes!("../../../icons/icon-tray.png");

/// 已加载的 (small, big) HICON（存 isize 以便放入静态缓存）。
/// HICON 被窗口引用期间必须保持有效，且各窗口（含销毁重建）可复用同一份，
/// 因此常驻缓存、从不销毁；句柄按系统 DPI 加载，系统缩放变更需重启应用刷新。
static CACHED_WINDOW_ICONS: OnceLock<(isize, isize)> = OnceLock::new();

pub fn apply_window_icon(window: &WebviewWindow) -> Result<(), AppError> {
    let Some((small, big)) = load_window_icons() else {
        return Ok(());
    };

    let tauri_hwnd = window
        .hwnd()
        .map_err(|error| AppError::Message(format!("获取窗口句柄失败: {error}")))?;
    let hwnd = HWND(tauri_hwnd.0 as isize as *mut _);

    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL_MESSAGE as usize)),
            Some(LPARAM(small)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG_MESSAGE as usize)),
            Some(LPARAM(big)),
        );
    }

    Ok(())
}

const ICON_SMALL_MESSAGE: u32 = 0; // WM_SETICON wParam：小图标（标题栏）
const ICON_BIG_MESSAGE: u32 = 1; // WM_SETICON wParam：大图标（任务栏）

/// 供托盘使用的图标：按系统托盘图标的 DPI 实际尺寸预乘缩放。
/// 加载失败时回退调用方自行使用 default_window_icon。
pub fn tray_icon_image() -> Result<Image<'static>, AppError> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    let size = u32::try_from(size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| AppError::Message("托盘图标尺寸异常".to_string()))?;

    let source = image::load_from_memory(ICON_PNG_BYTES)
        .map_err(|error| AppError::Message(format!("解码应用图标失败: {error}")))?
        .to_rgba8();
    let (width, height) = source.dimensions();

    let scaled = downscale_rgba_premultiplied(source.as_raw(), width, height, size, size);
    Ok(Image::new_owned(scaled, size, size))
}

/// 资源加载失败时返回 None（例如图标资源缺失），调用方保持 Tauri 默认行为。
fn load_window_icons() -> Option<(isize, isize)> {
    if let Some(icons) = CACHED_WINDOW_ICONS.get() {
        return Some(*icons);
    }

    let icons = unsafe {
        let module = GetModuleHandleW(PCWSTR::null()).ok()?;
        let instance = HINSTANCE(module.0);
        let load = |width: i32, height: i32| {
            let handle = LoadImageW(
                Some(instance),
                PCWSTR(ICON_RESOURCE_ID as usize as _),
                IMAGE_ICON,
                width,
                height,
                LR_DEFAULTSIZE,
            )
            .ok()?;
            Some(HICON(handle.0).0 as isize)
        };
        let small = load(
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
        )?;
        // 任务栏以 SM_CXICON 的 3/4（100% 缩放下 24px）绘制按钮图标，按该尺寸
        // 取档可 1:1 命中 ICO 中 24/30/36px 档位，避免 shell 二次缩放糊掉细线条
        let big = load(
            GetSystemMetrics(SM_CXICON) * 3 / 4,
            GetSystemMetrics(SM_CYICON) * 3 / 4,
        )?;
        (small, big)
    };

    if CACHED_WINDOW_ICONS.set(icons).is_err() {
        return CACHED_WINDOW_ICONS.get().copied();
    }
    Some(icons)
}

/// 盒式滤波缩小，在预乘 alpha 空间累积后还原，透明区（RGB=0）不参与混色，
/// 避免降采样把透明黑拉进边缘产生暗晕（与 scripts/make_icon.py 策略一致）。
/// 仅支持缩小（dw <= sw 且 dh <= sh）。
fn downscale_rgba_premultiplied(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    debug_assert!(dw > 0 && dh > 0 && dw <= sw && dh <= sh);
    debug_assert_eq!(src.len(), (sw * sh * 4) as usize);

    let mut dst = vec![0u8; (dw * dh * 4) as usize];
    for dy in 0..dh {
        let src_y0 = dy as f64 * sh as f64 / dh as f64;
        let src_y1 = (dy + 1) as f64 * sh as f64 / dh as f64;
        for dx in 0..dw {
            let src_x0 = dx as f64 * sw as f64 / dw as f64;
            let src_x1 = (dx + 1) as f64 * sw as f64 / dw as f64;

            let mut weighted_alpha = 0.0;
            let mut weighted_rgb = [0.0f64; 3];
            let mut coverage = 0.0;

            let y_end = (src_y1.ceil() as u32).min(sh);
            let x_end = (src_x1.ceil() as u32).min(sw);
            for sy in (src_y0.floor() as u32)..y_end {
                let wy = (src_y1.min(sy as f64 + 1.0) - src_y0.max(sy as f64)).clamp(0.0, 1.0);
                for sx in (src_x0.floor() as u32)..x_end {
                    let wx = (src_x1.min(sx as f64 + 1.0) - src_x0.max(sx as f64)).clamp(0.0, 1.0);
                    let weight = wx * wy;
                    if weight <= 0.0 {
                        continue;
                    }

                    let pixel = ((sy * sw + sx) * 4) as usize;
                    let alpha = f64::from(src[pixel + 3]) / 255.0;
                    for channel in 0..3 {
                        weighted_rgb[channel] += f64::from(src[pixel + channel]) * alpha * weight;
                    }
                    weighted_alpha += alpha * weight;
                    coverage += weight;
                }
            }

            let out = &mut dst[((dy * dw + dx) * 4) as usize..][..4];
            if coverage <= 0.0 {
                continue;
            }
            let alpha = weighted_alpha / coverage;
            for channel in 0..3 {
                // 预乘还原后 value 已回到 0-255 色域，不能再乘 255
                let value = if weighted_alpha > 0.0 {
                    weighted_rgb[channel] / weighted_alpha
                } else {
                    0.0
                };
                out[channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            out[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::{downscale_rgba_premultiplied, tray_icon_image, ICON_PNG_BYTES};

    #[test]
    fn uniform_opaque_image_downscales_to_same_color() {
        // 2×2 全红不透明 → 1×1 应原色原 alpha
        let src = vec![
            255u8, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];

        let dst = downscale_rgba_premultiplied(&src, 2, 2, 1, 1);

        assert_eq!(dst, vec![255, 0, 0, 255]);
    }

    #[test]
    fn transparent_region_does_not_darken_edge_mix() {
        // 左半红色不透明 + 右半透明黑 → 1×1 的 RGB 应保持红色不被拉暗，
        // alpha 为 128（0.5 覆盖）。非预乘缩放会得到 (128, 0, 0) 暗边。
        let src = vec![255u8, 0, 0, 255, 0, 0, 0, 0];

        let dst = downscale_rgba_premultiplied(&src, 2, 1, 1, 1);

        assert_eq!(dst, vec![255, 0, 0, 128]);
    }

    #[test]
    fn downscaled_alpha_accumulates_across_rows_and_columns() {
        // 2×2：仅左上不透明（绿），其余全透明 → 1×1 alpha=64，RGB 保持绿色
        let src = vec![0u8, 255, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        let dst = downscale_rgba_premultiplied(&src, 2, 2, 1, 1);

        assert_eq!(dst, vec![0, 255, 0, 64]);
    }

    #[test]
    fn mid_tone_colors_survive_downscale_without_rescaling() {
        // 回归：预乘还原后的色值已在 0-255 色域，误再乘 255 会被钳制成纯白
        //（全部极值色 0/255 的用例无法暴露该问题，必须用中间调验证）。
        let src = vec![
            180u8, 120, 60, 255, 180, 120, 60, 255, 180, 120, 60, 255, 180, 120, 60, 255,
        ];

        let dst = downscale_rgba_premultiplied(&src, 2, 2, 1, 1);

        assert_eq!(dst, vec![180, 120, 60, 255]);
    }

    #[test]
    fn embedded_icon_decodes_and_tray_image_matches_dpi_size() {
        let decoded = image::load_from_memory(ICON_PNG_BYTES).expect("内置图标应为合法 PNG");
        assert_eq!((decoded.width(), decoded.height()), (128, 128));

        // 测试环境无法保证托盘系统度量可用，仅验证函数在可获取尺寸时行为一致
        if let Ok(image) = tray_icon_image() {
            assert_eq!(image.width(), image.height());
            assert!(image.width() >= 16 && image.width() <= 96);
        }
    }
}
