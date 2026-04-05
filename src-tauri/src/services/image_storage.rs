use std::{
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
};

use png::{BitDepth, ColorType, Decoder, Encoder, Transformations};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::error::AppError;

const IMAGE_DIR_NAME: &str = "images";
const PNG_EXTENSION: &str = "png";
const PNG_FORMAT: &str = "png";

#[derive(Debug, Clone)]
pub struct PreparedImage {
    pub png_bytes: Vec<u8>,
    pub content_hash: String,
    pub width: i32,
    pub height: i32,
    pub image_format: String,
    pub file_size: i64,
}

#[derive(Debug, Clone)]
pub struct StoredImage {
    pub image_path: String,
}

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub png_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ImageStorage {
    base_dir: PathBuf,
}

impl ImageStorage {
    pub fn new(base_dir: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(base_dir.join(IMAGE_DIR_NAME))?;
        Ok(Self { base_dir })
    }

    pub fn prepare_image(
        &self,
        rgba: &[u8],
        width: usize,
        height: usize,
        pre_encoded_png: Option<&[u8]>,
    ) -> Result<PreparedImage, AppError> {
        let png_bytes = match pre_encoded_png {
            Some(bytes) => bytes.to_vec(),
            None => encode_png(rgba, width, height)?,
        };
        let file_size = i64::try_from(png_bytes.len())
            .map_err(|_| AppError::Message("图片数据过大，无法保存".to_string()))?;
        let width = i32::try_from(width)
            .map_err(|_| AppError::Message("图片宽度超出支持范围".to_string()))?;
        let height = i32::try_from(height)
            .map_err(|_| AppError::Message("图片高度超出支持范围".to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(width.to_le_bytes());
        hasher.update(height.to_le_bytes());
        hasher.update(rgba);

        Ok(PreparedImage {
            png_bytes,
            content_hash: format!("{:x}", hasher.finalize()),
            width,
            height,
            image_format: PNG_FORMAT.to_string(),
            file_size,
        })
    }

    pub fn store_prepared_image(&self, prepared: &PreparedImage) -> Result<StoredImage, AppError> {
        let file_name = format!("{}.{}", Uuid::new_v4(), PNG_EXTENSION);
        let relative_path = Path::new(IMAGE_DIR_NAME).join(file_name);
        let absolute_path = self.base_dir.join(&relative_path);
        fs::write(&absolute_path, &prepared.png_bytes)?;

        Ok(StoredImage {
            image_path: relative_path.to_string_lossy().to_string(),
        })
    }

    pub fn load_image(&self, image_path: &str) -> Result<DecodedImage, AppError> {
        let png_bytes = fs::read(self.resolve_existing_image_path(image_path)?)?;
        decode_png_bytes(&png_bytes)
    }

    pub fn delete_image(&self, image_path: &str) -> Result<(), AppError> {
        match fs::remove_file(self.resolve_image_path(image_path)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn resolve_existing_image_path(&self, image_path: &str) -> Result<PathBuf, AppError> {
        let absolute_path = self.resolve_image_path(image_path)?;
        if !absolute_path.is_file() {
            return Err(AppError::Message("图片文件不存在".to_string()));
        }
        Ok(absolute_path)
    }

    pub(crate) fn resolve_image_path(&self, image_path: &str) -> Result<PathBuf, AppError> {
        let relative_path = Path::new(image_path);
        if relative_path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        }) {
            return Err(AppError::Message("图片路径无效".to_string()));
        }

        Ok(self.base_dir.join(relative_path))
    }
}

fn decode_png_bytes(png_bytes: &[u8]) -> Result<DecodedImage, AppError> {
    let mut decoder = Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| AppError::Message(format!("读取 PNG 图片失败: {error}")))?;

    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut rgba)
        .map_err(|error| AppError::Message(format!("解码 PNG 图片失败: {error}")))?;
    rgba.truncate(info.buffer_size());

    if info.color_type != ColorType::Rgba || info.bit_depth != BitDepth::Eight {
        return Err(AppError::Message("PNG 图片像素格式不受支持".to_string()));
    }

    Ok(DecodedImage {
        rgba,
        width: usize::try_from(info.width)
            .map_err(|_| AppError::Message("图片宽度超出支持范围".to_string()))?,
        height: usize::try_from(info.height)
            .map_err(|_| AppError::Message("图片高度超出支持范围".to_string()))?,
        png_bytes: png_bytes.to_vec(),
    })
}

fn encode_png(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, AppError> {
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AppError::Message("图片尺寸超出支持范围".to_string()))?;
    if rgba.len() != expected_len {
        return Err(AppError::Message(
            "图片像素数据长度与尺寸不匹配".to_string(),
        ));
    }

    let mut output = Vec::new();
    let width =
        u32::try_from(width).map_err(|_| AppError::Message("图片宽度超出支持范围".to_string()))?;
    let height =
        u32::try_from(height).map_err(|_| AppError::Message("图片高度超出支持范围".to_string()))?;

    {
        let mut encoder = Encoder::new(&mut output, width, height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| AppError::Message(format!("编码 PNG 图片失败: {error}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| AppError::Message(format!("写入 PNG 图片失败: {error}")))?;
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::ImageStorage;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "floatpaste-image-storage-test-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn prepared_and_stored_png_round_trip_preserves_rgba_pixels() {
        let base_dir = temp_dir();
        let storage = ImageStorage::new(base_dir.clone()).unwrap();
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for _ in 0..(16 * 16) {
            rgba.extend_from_slice(&[255, 0, 0, 255]);
        }

        let prepared = storage.prepare_image(&rgba, 16, 16, None).unwrap();
        assert_eq!(prepared.image_format, "png");
        assert!(prepared.file_size > 0);
        assert_eq!(prepared.width, 16);
        assert_eq!(prepared.height, 16);

        let stored = storage.store_prepared_image(&prepared).unwrap();
        let decoded = storage.load_image(&stored.image_path).unwrap();

        assert_eq!(decoded.width, 16);
        assert_eq!(decoded.height, 16);
        assert_eq!(decoded.rgba, rgba);
        assert_eq!(decoded.png_bytes, prepared.png_bytes);

        storage.delete_image(&stored.image_path).unwrap();
        assert!(!base_dir.join(&stored.image_path).exists());

        fs::remove_dir_all(base_dir).unwrap();
    }

    #[test]
    fn image_storage_rejects_invalid_paths_and_resolves_existing_files() {
        let base_dir = temp_dir();
        let storage = ImageStorage::new(base_dir.clone()).unwrap();
        let existing_relative_path = "images/existing.png";
        let existing_absolute_path = base_dir.join(existing_relative_path);
        fs::write(&existing_absolute_path, b"png").unwrap();

        let resolved = storage
            .resolve_existing_image_path(existing_relative_path)
            .unwrap();
        assert_eq!(resolved, existing_absolute_path);

        let invalid = storage.resolve_existing_image_path("../secret.png");
        assert!(invalid.is_err());

        let missing = storage.resolve_existing_image_path("images/missing.png");
        assert!(missing.is_err());

        fs::remove_dir_all(base_dir).unwrap();
    }
}
