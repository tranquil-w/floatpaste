use std::{io::Cursor, slice};

use image::{codecs::bmp::BmpDecoder, DynamicImage, ImageDecoder};
use windows::{
    core::{w, Error as WindowsError},
    Win32::{
        Foundation::HGLOBAL,
        System::{
            DataExchange::{
                CloseClipboard, GetClipboardData, GetPriorityClipboardFormat, OpenClipboard,
                RegisterClipboardFormatW,
            },
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
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
    decode_image(decoder)
}

fn decode_dib_bytes(data: &[u8]) -> Result<ClipboardImageData, AppError> {
    let decoder = BmpDecoder::new_without_file_header(Cursor::new(data))
        .map_err(|error| AppError::Message(format!("解码剪贴板 DIB 图片失败: {error}")))?;
    decode_image(decoder)
}

fn decode_image<D>(decoder: D) -> Result<ClipboardImageData, AppError>
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
    })
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

    use super::{decode_dib_bytes, decode_png_bytes};

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
}
