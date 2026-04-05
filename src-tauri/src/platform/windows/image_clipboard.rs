use std::{io::Cursor, ptr::copy_nonoverlapping, slice};

use image::{
    codecs::bmp::{BmpDecoder, BmpEncoder},
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder,
};
use tracing::warn;
use windows::{
    core::{w, Error as WindowsError},
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetPriorityClipboardFormat,
                OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT},
        },
    },
};

use crate::domain::error::AppError;

const CF_DIB_FORMAT: u32 = 8;
const CF_DIBV5_FORMAT: u32 = 17;

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

pub fn read_image_from_clipboard() -> Result<Option<ClipboardImageData>, AppError> {
    unsafe {
        OpenClipboard(None).map_err(map_clipboard_error)?;
        let result = (|| {
            let png_format = png_clipboard_format()?;
            if let Some(format) = preferred_image_clipboard_format(png_format)? {
                let data = read_clipboard_bytes(format)?;
                if format == png_format {
                    return decode_png_bytes(&data).map(Some);
                }
                return decode_dib_bytes(&data).map(Some);
            }

            Ok(None)
        })();
        let _ = CloseClipboard();
        result
    }
}

fn preferred_image_clipboard_format(png_format: u32) -> Result<Option<u32>, AppError> {
    unsafe {
        match GetPriorityClipboardFormat(&[png_format, CF_DIBV5_FORMAT, CF_DIB_FORMAT]) {
            value if value == png_format as i32 => Ok(Some(png_format)),
            value if value == CF_DIBV5_FORMAT as i32 => Ok(Some(CF_DIBV5_FORMAT)),
            value if value == CF_DIB_FORMAT as i32 => Ok(Some(CF_DIB_FORMAT)),
            -1 => Ok(None),
            0 => Err(AppError::Clipboard(format!(
                "检查图片剪贴板格式失败: {}",
                WindowsError::from_win32()
            ))),
            other => Err(AppError::Clipboard(format!(
                "检查图片剪贴板格式返回了未知结果: {other}"
            ))),
        }
    }
}

fn png_clipboard_format() -> Result<u32, AppError> {
    let format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    if format == 0 {
        return Err(AppError::Clipboard(format!(
            "注册 PNG 剪贴板格式失败: {}",
            WindowsError::from_win32()
        )));
    }

    Ok(format)
}

unsafe fn read_clipboard_bytes(format: u32) -> Result<Vec<u8>, AppError> {
    let handle = GetClipboardData(format).map_err(map_clipboard_error)?;
    let handle = HGLOBAL(handle.0);
    if handle.is_invalid() {
        return Err(AppError::Clipboard("剪贴板图片句柄无效".to_string()));
    }

    let size = GlobalSize(handle);
    if size == 0 {
        return Err(AppError::Clipboard("剪贴板图片数据为空".to_string()));
    }

    let pointer = GlobalLock(handle).cast::<u8>();
    if pointer.is_null() {
        return Err(AppError::Clipboard("锁定剪贴板图片数据失败".to_string()));
    }

    let bytes = slice::from_raw_parts(pointer, size).to_vec();
    let _ = GlobalUnlock(handle);
    Ok(bytes)
}

fn decode_png_bytes(data: &[u8]) -> Result<ClipboardImageData, AppError> {
    let decoder = image::codecs::png::PngDecoder::new(Cursor::new(data))
        .map_err(|error| AppError::Message(format!("解码剪贴板 PNG 图片失败: {error}")))?;
    decode_image(decoder, Some(data.to_vec()))
}

fn decode_dib_bytes(data: &[u8]) -> Result<ClipboardImageData, AppError> {
    let decoder = BmpDecoder::new_without_file_header(Cursor::new(data))
        .map_err(|error| AppError::Message(format!("解码剪贴板 DIB 图片失败: {error}")))?;
    decode_image(decoder, None)
}

fn decode_image<D>(
    decoder: D,
    png_bytes: Option<Vec<u8>>,
) -> Result<ClipboardImageData, AppError>
where
    D: ImageDecoder,
{
    let (width, height) = decoder.dimensions();
    let image = DynamicImage::from_decoder(decoder)
        .map_err(|error| AppError::Message(format!("读取剪贴板图片像素失败: {error}")))?;

    Ok(ClipboardImageData {
        rgba: image.into_rgba8().into_raw(),
        width: usize::try_from(width)
            .map_err(|_| AppError::Message("剪贴板图片宽度超出支持范围".to_string()))?,
        height: usize::try_from(height)
            .map_err(|_| AppError::Message("剪贴板图片高度超出支持范围".to_string()))?,
        png_bytes,
    })
}

pub fn write_image_to_clipboard(
    owner_window: isize,
    image: &ClipboardImageData,
) -> Result<(), AppError> {
    let payload = build_clipboard_image_payload(
        &image.rgba,
        image.width,
        image.height,
        image.png_bytes.clone(),
    )?;
    let owner = HWND(owner_window as *mut _);
    if owner.0.is_null() {
        return Err(AppError::Clipboard("剪贴板宿主窗口句柄无效".to_string()));
    }

    unsafe {
        OpenClipboard(Some(owner)).map_err(map_clipboard_error)?;
        let result = (|| {
            EmptyClipboard().map_err(map_clipboard_error)?;

            if let Some(png_bytes) = payload.png_bytes.as_deref() {
                let png_format = png_clipboard_format()?;
                if let Err(error) = set_clipboard_bytes(png_format, png_bytes) {
                    warn!("写入补充 PNG 剪贴板格式失败，将仅保留标准 DIB: {error}");
                }
            }

            set_clipboard_bytes(CF_DIB_FORMAT, &payload.dib_bytes)?;

            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
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
            u32::try_from(width).map_err(|_| AppError::Message("图片宽度超出支持范围".to_string()))?,
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

unsafe fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), AppError> {
    let handle = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len())
        .map_err(|e| AppError::Clipboard(format!("分配剪贴板内存失败: {e}")))?;
    if handle.is_invalid() {
        return Err(AppError::Clipboard("分配剪贴板内存失败".to_string()));
    }

    let pointer = GlobalLock(handle).cast::<u8>();
    if pointer.is_null() {
        let _ = free_global_memory(handle);
        return Err(AppError::Clipboard("锁定剪贴板内存失败".to_string()));
    }

    copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
    let _ = GlobalUnlock(handle);

    if let Err(error) = SetClipboardData(format, Some(HANDLE(handle.0))) {
        let _ = free_global_memory(handle);
        return Err(AppError::Clipboard(format!("写入剪贴板数据失败: {error}")));
    }

    Ok(())
}

fn free_global_memory(handle: HGLOBAL) -> Result<(), AppError> {
    unsafe {
        GlobalFree(Some(handle))
            .map(|_| ())
            .map_err(map_clipboard_error)
    }
}

fn map_clipboard_error(error: windows::core::Error) -> AppError {
    AppError::Clipboard(error.to_string())
}

#[cfg(test)]
mod tests {
    use image::{
        codecs::{bmp::BmpEncoder, png::PngEncoder},
        ExtendedColorType, ImageEncoder,
    };

    use super::{build_clipboard_image_payload, decode_dib_bytes, decode_png_bytes};

    #[test]
    fn decode_png_bytes_round_trips_rgba_pixels() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(&rgba, 2, 1, ExtendedColorType::Rgba8)
            .unwrap();

        let decoded = decode_png_bytes(&encoded).unwrap();

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

        assert_eq!(u32::from_le_bytes(payload.dib_bytes[0..4].try_into().unwrap()), 108);
        assert_eq!(i32::from_le_bytes(payload.dib_bytes[4..8].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(payload.dib_bytes[8..12].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(payload.dib_bytes[14..16].try_into().unwrap()), 32);
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
