//! 图片加载与缩放：解码在工作线程执行，跨线程只传原始 RGBA 像素
//! （slint::Image 内部是 Rc 引用计数，不能跨线程），
//! 事件循环侧用 [`image_from_rgba`] 构造共享像素缓冲。

use floatpaste_core::state::CoreState;
use slint::{Rgba8Pixel, SharedPixelBuffer};

/// 原始 RGBA 数据与尺寸
pub struct RawImage {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// 缩略图边长（物理像素，供 2x DPI 下的 72px 逻辑显示）
const THUMB_SIZE: u32 = 144;
/// 超过该字节数的源图跳过解码（防御异常大文件拖慢工作线程）
const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;

fn decode(core: &CoreState, image_path: &str, thumb: bool) -> Option<RawImage> {
    let path = core
        .image_storage
        .resolve_existing_image_path(image_path)
        .ok()?;
    let metadata = std::fs::metadata(&path).ok()?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return None;
    }

    let mut image = image::open(&path).ok()?;
    if thumb {
        image = image.thumbnail(THUMB_SIZE, THUMB_SIZE);
    }
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return None;
    }

    let rgba = image.to_rgba8().into_raw();
    Some(RawImage {
        pixels: rgba,
        width,
        height,
    })
}

/// 列表缩略图（144×144 内缩放）
pub fn load_thumbnail_raw(core: &CoreState, image_path: &str) -> Option<RawImage> {
    decode(core, image_path, true)
}

/// tooltip 预览用原图（不缩放；显示尺寸由元数据计算）
pub fn load_full_image_raw(core: &CoreState, image_path: &str) -> Option<RawImage> {
    decode(core, image_path, false)
}

/// 事件循环线程：原始像素 → Slint 图像
pub fn image_from_rgba(raw: RawImage) -> slint::Image {
    let buffer =
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&raw.pixels, raw.width, raw.height);
    slint::Image::from_rgba8(buffer)
}
