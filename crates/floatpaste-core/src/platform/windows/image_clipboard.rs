use std::io::Cursor;

use image::{
    codecs::bmp::{BmpDecoder, BmpEncoder},
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder,
};
use windows::core::w;

use crate::domain::error::AppError;

use super::clipboard_formats::{registered_format, write_formats, ClipboardSnapshot};

const CF_DIB_FORMAT: u32 = 8;

#[derive(Debug, Clone)]
pub struct ClipboardImageData {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
    /// 剪贴板原始 PNG 字节。当剪贴板格式为 PNG 时直接透传，避免解码→重编码开销。
    pub png_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClipboardImagePayload {
    dib_bytes: Vec<u8>,
    png_bytes: Option<Vec<u8>>,
}

/// 接收剪贴板字节所有权：PNG 透传时原始字节直接存入条目，不再拷贝一份
/// （大截图 PNG 数 MB，录入峰值减半）
pub(super) fn decode_png_bytes(data: Vec<u8>) -> Result<ClipboardImageData, AppError> {
    let decoder = image::codecs::png::PngDecoder::new(Cursor::new(&data))
        .map_err(|error| AppError::Message(format!("解码剪贴板 PNG 图片失败: {error}")))?;
    let (width, height, rgba) = decode_image_pixels(decoder)?;
    Ok(ClipboardImageData {
        rgba,
        width,
        height,
        png_bytes: Some(data),
    })
}

pub(super) fn decode_dib_bytes(data: &[u8]) -> Result<ClipboardImageData, AppError> {
    let decoder = BmpDecoder::new_without_file_header(Cursor::new(data))
        .map_err(|error| AppError::Message(format!("解码剪贴板 DIB 图片失败: {error}")))?;
    let (width, height, rgba) = decode_image_pixels(decoder)?;
    Ok(ClipboardImageData {
        rgba,
        width,
        height,
        png_bytes: None,
    })
}

/// 解码像素（消费 decoder，借用随之结束，调用方随后可回收原始字节）
fn decode_image_pixels<D>(mut decoder: D) -> Result<(usize, usize, Vec<u8>), AppError>
where
    D: ImageDecoder,
{
    // 收紧解码内存：异常超大图按解码失败拒绝，防止瞬时分配上 GB
    decoder
        .set_limits(crate::services::image_decode::decode_limits())
        .map_err(|error| AppError::Message(format!("剪贴板图片尺寸超出支持范围: {error}")))?;
    let (width, height) = decoder.dimensions();
    let image = DynamicImage::from_decoder(decoder)
        .map_err(|error| AppError::Message(format!("读取剪贴板图片像素失败: {error}")))?;

    let width = usize::try_from(width)
        .map_err(|_| AppError::Message("剪贴板图片宽度超出支持范围".to_string()))?;
    let height = usize::try_from(height)
        .map_err(|_| AppError::Message("剪贴板图片高度超出支持范围".to_string()))?;
    Ok((width, height, image.into_rgba8().into_raw()))
}

pub fn write_image_to_clipboard(
    owner: Option<isize>,
    image: &ClipboardImageData,
    rollback: Option<&ClipboardSnapshot>,
) -> Result<u32, AppError> {
    let payload = build_clipboard_image_payload(
        &image.rgba,
        image.width,
        image.height,
        image.png_bytes.clone(),
    )?;
    let mut formats = Vec::new();
    if let Some(png) = payload.png_bytes {
        formats.push((registered_format(w!("PNG"))?, png));
    }
    formats.push((CF_DIB_FORMAT, payload.dib_bytes));
    write_formats(&formats, owner, None, rollback)?
        .ok_or_else(|| AppError::Clipboard("发布后剪贴板发生变化，已取消上屏".into()))
}

fn build_clipboard_image_payload(
    rgba: &[u8],
    width: usize,
    height: usize,
    png_bytes: Option<Vec<u8>>,
) -> Result<ClipboardImagePayload, AppError> {
    validate_rgba_buffer(rgba, width, height)?;

    Ok(ClipboardImagePayload {
        dib_bytes: encode_dib_bytes(rgba, width, height)?,
        png_bytes,
    })
}

fn encode_dib_bytes(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, AppError> {
    validate_rgba_buffer(rgba, width, height)?;

    let mut bmp_bytes = Vec::new();
    BmpEncoder::new(&mut bmp_bytes)
        .write_image(
            rgba,
            u32::try_from(width)
                .map_err(|_| AppError::Message("图片宽度超出支持范围".to_string()))?,
            u32::try_from(height)
                .map_err(|_| AppError::Message("图片高度超出支持范围".to_string()))?,
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| AppError::Message(format!("编码标准 DIB 图片失败: {error}")))?;

    let file_header_size = 14usize;
    if bmp_bytes.len() <= file_header_size {
        return Err(AppError::Message("编码后的 BMP 数据无效".to_string()));
    }

    Ok(bmp_bytes.split_off(file_header_size))
}

fn validate_rgba_buffer(rgba: &[u8], width: usize, height: usize) -> Result<(), AppError> {
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AppError::Message("图片尺寸超出支持范围".to_string()))?;
    if rgba.len() != expected_len {
        return Err(AppError::Message(
            "图片像素数据长度与尺寸不匹配".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use image::{
        codecs::{bmp::BmpEncoder, png::PngEncoder},
        ExtendedColorType, ImageEncoder,
    };

    use super::{build_clipboard_image_payload, decode_dib_bytes, decode_png_bytes};
    use crate::services::image_decode::MAX_IMAGE_EDGE;

    #[test]
    fn decode_png_bytes_rejects_oversized_dimensions() {
        // 篡改 IHDR 尺寸为超大值：必须因解码上限被拒，而不是解码瞬间分配巨量像素
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(&rgba, 2, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let oversized = MAX_IMAGE_EDGE + 1;
        encoded[16..20].copy_from_slice(&oversized.to_be_bytes());
        encoded[20..24].copy_from_slice(&oversized.to_be_bytes());

        assert!(decode_png_bytes(encoded).is_err());
    }

    #[test]
    fn decode_png_bytes_round_trips_rgba_pixels() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(&rgba, 2, 1, ExtendedColorType::Rgba8)
            .unwrap();

        let decoded = decode_png_bytes(encoded).unwrap();

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.rgba, rgba);
    }

    #[test]
    fn decode_dib_bytes_without_file_header_round_trips_rgba_pixels() {
        let rgb = vec![255, 0, 0, 0, 255, 0];
        let mut encoded = Vec::new();
        BmpEncoder::new(&mut encoded)
            .write_image(&rgb, 2, 1, ExtendedColorType::Rgb8)
            .unwrap();

        let decoded = decode_dib_bytes(&encoded[14..]).unwrap();

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.rgba, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn build_clipboard_image_payload_generates_standard_dib_header() {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 255, 0,
        ];

        let payload = build_clipboard_image_payload(&rgba, 2, 2, None).unwrap();

        assert_eq!(
            u32::from_le_bytes(payload.dib_bytes[0..4].try_into().unwrap()),
            108
        );
        assert_eq!(
            i32::from_le_bytes(payload.dib_bytes[4..8].try_into().unwrap()),
            2
        );
        assert_eq!(
            i32::from_le_bytes(payload.dib_bytes[8..12].try_into().unwrap()),
            2
        );
        assert_eq!(
            u16::from_le_bytes(payload.dib_bytes[14..16].try_into().unwrap()),
            32
        );
        assert_eq!(payload.dib_bytes.len(), 108 + rgba.len());
        assert!(payload.png_bytes.is_none());
    }

    #[test]
    fn build_clipboard_image_payload_keeps_original_png_bytes_for_fast_path() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let mut encoded_png = Vec::new();
        PngEncoder::new(&mut encoded_png)
            .write_image(&rgba, 2, 1, ExtendedColorType::Rgba8)
            .unwrap();

        let payload =
            build_clipboard_image_payload(&rgba, 2, 1, Some(encoded_png.clone())).unwrap();

        assert_eq!(payload.png_bytes, Some(encoded_png));
    }
}
