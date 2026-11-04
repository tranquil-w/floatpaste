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
    crate::services::image_decode::validate_pixels(width as usize, height as usize)?;
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

    use super::{
        build_clipboard_image_payload, decode_dib_bytes, decode_png_bytes, encode_dib_bytes,
    };
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

    /// 回归：粘贴图片后监听重采集发布的载荷，重算 hash 必须与库中条目一致，
    /// 且 3 秒抑制窗内命中自写抑制——否则会新增一条相同图片的重复记录。
    /// 链路：监听采集入库 → 粘贴发布载荷 → 按监听链路重采集。
    #[test]
    fn republished_image_payload_reingests_under_the_same_identity() {
        use std::time::Duration;

        use crate::{
            repository::sqlite_repository::SqliteRepository,
            services::{
                dedup_service::{DedupDecision, DedupService},
                history_service::HistoryService,
                image_storage::ImageStorage,
                normalize_service::NormalizeService,
                privacy_service::PrivacyService,
            },
            state::CoreState,
        };

        let dir = std::env::temp_dir().join(format!(
            "floatpaste-image-resuppress-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let core = CoreState::new(
            SqliteRepository::new(&dir.join("db.sqlite")).unwrap(),
            ImageStorage::new(dir.clone()).unwrap(),
            Default::default(),
        );

        // 1) 模拟监听采集入库：剪贴板 PNG 直传 → 解码 → 准备 → 公共入口入库。
        //    像素用伪随机纹理，模拟真实截图：不同压缩级别的编码字节长度才有区分
        let side = 64usize;
        let mut rgba = Vec::with_capacity(side * side * 4);
        let mut seed = 0x9E3779B9u64;
        for _ in 0..side * side {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            rgba.extend_from_slice(&[
                (seed >> 33) as u8,
                (seed >> 41) as u8,
                (seed >> 49) as u8,
                255,
            ]);
        }
        let mut clipboard_png = Vec::new();
        PngEncoder::new(&mut clipboard_png)
            .write_image(&rgba, side as u32, side as u32, ExtendedColorType::Rgba8)
            .unwrap();
        let captured = decode_png_bytes(clipboard_png).unwrap();
        let prepared = core
            .image_storage
            .prepare_image(
                &captured.rgba,
                captured.width,
                captured.height,
                captured.png_bytes.as_deref(),
            )
            .unwrap();
        let detail = HistoryService::ingest_image(&core, prepared, None)
            .unwrap()
            .expect("首次入库应新增记录");
        let image_path = detail.image_path.clone().expect("图片条目应带存储路径");
        let stored_file_size = detail.file_size.expect("图片条目应记录 PNG 字节长度");

        // 2) 模拟粘贴发布：与 write_item_to_clipboard 图片本体分支一致——
        //    先登记 detail.hash 抑制，再从 load_image 构建发布载荷
        core.self_write_guard()
            .suppress_hash(detail.hash.clone(), Duration::from_secs(3))
            .unwrap();
        let loaded = core.image_storage.load_image(&image_path).unwrap();
        let payload = build_clipboard_image_payload(
            &loaded.rgba,
            loaded.width,
            loaded.height,
            Some(loaded.png_bytes),
        )
        .unwrap();
        let published_png = payload.png_bytes.expect("PNG 直传必须保留原始字节");

        // 3) 模拟粘贴后监听重采集：发布的 PNG 直传字节按监听链路重算身份
        let republished = decode_png_bytes(published_png).unwrap();
        let prepared_again = core
            .image_storage
            .prepare_image(
                &republished.rgba,
                republished.width,
                republished.height,
                republished.png_bytes.as_deref(),
            )
            .unwrap();
        let normalized = NormalizeService::normalize_image(
            None,
            Some(prepared_again.width),
            Some(prepared_again.height),
            Some(prepared_again.image_format),
            Some(prepared_again.file_size),
            Some(prepared_again.content_hash),
            None,
        )
        .unwrap();

        assert_eq!(
            normalized.normalized.hash, detail.hash,
            "发布的图片载荷重算身份必须与库中条目一致，否则去重也拦不住重复记录"
        );
        let settings = core.current_settings().unwrap();
        let should_capture = PrivacyService::should_capture_image(
            &settings,
            &normalized.normalized,
            None,
            &core.self_write_guard(),
        )
        .unwrap();
        assert!(
            !should_capture,
            "粘贴发布后抑制窗内重采集必须被自写抑制跳过"
        );

        // 4) 目标应用回写场景：接收方粘贴后常以自己的 PNG 编码器回写剪贴板，
        //    同一像素、不同压缩字节。按监听链路重采集必须仍是同一身份，
        //    且抑制窗外去重也只置顶既有条目——修复前 file_size 混入身份，
        //    编码字节长度不同会新增重复记录
        let mut rewritten_png = Vec::new();
        PngEncoder::new_with_quality(
            &mut rewritten_png,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::default(),
        )
        .write_image(&rgba, side as u32, side as u32, ExtendedColorType::Rgba8)
        .unwrap();
        let rewritten = decode_png_bytes(rewritten_png).unwrap();
        let prepared_rewritten = core
            .image_storage
            .prepare_image(
                &rewritten.rgba,
                rewritten.width,
                rewritten.height,
                rewritten.png_bytes.as_deref(),
            )
            .unwrap();
        assert_ne!(
            prepared_rewritten.file_size, stored_file_size,
            "场景前提：回写重编码的 PNG 字节长度应与原始条目不同"
        );
        let normalized_rewritten = NormalizeService::normalize_image(
            None,
            Some(prepared_rewritten.width),
            Some(prepared_rewritten.height),
            Some(prepared_rewritten.image_format),
            Some(prepared_rewritten.file_size),
            Some(prepared_rewritten.content_hash),
            None,
        )
        .unwrap();
        assert_eq!(
            normalized_rewritten.normalized.hash, detail.hash,
            "同像素不同编码的回写必须保持同一身份"
        );
        match DedupService::default()
            .decide(&core.repository, &normalized_rewritten.normalized.hash)
            .unwrap()
        {
            DedupDecision::BumpExisting(id) => assert_eq!(id, detail.id),
            other => panic!("相同像素回写必须置顶既有条目，实际: {other:?}"),
        }

        drop(core);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
