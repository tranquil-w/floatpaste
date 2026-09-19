//! 共享图片解码上限：剪贴板录入与 UI 缩略图/预览解码共用。
//!
//! image crate 的解码器默认不限制像素内存，一张压缩后仅几 MB 的 PNG
//! 可声明超大尺寸，解码瞬间分配上 GB 的 RGBA。统一在此收紧，超限按
//! 解码失败处理（UI 走占位/失败态，录入走既有错误路径）。

use image::Limits;

/// 单边像素上限：覆盖 8K 屏截图（7680×4320）留足余量
pub const MAX_IMAGE_EDGE: u32 = 16384;
/// 解码期像素分配上限（字节）
pub const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;

pub fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_limits_caps_edges_and_alloc() {
        let limits = decode_limits();
        assert_eq!(limits.max_image_width, Some(MAX_IMAGE_EDGE));
        assert_eq!(limits.max_image_height, Some(MAX_IMAGE_EDGE));
        assert_eq!(limits.max_alloc, Some(MAX_DECODE_ALLOC));
    }
}
