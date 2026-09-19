//! 图片加载与缩放：解码在工作线程执行，跨线程只传原始 RGBA 像素
//! （slint::Image 内部是 Rc 引用计数，不能跨线程），
//! 事件循环侧用 [`image_from_rgba`] 构造共享像素缓冲。
//!
//! 列表缩略图缓存（id → 图像）按窗口共享：速贴与搜索展示同一批条目，
//! 共用一份解码结果与失败哨兵，避免双份内存与重复解码。
//! 缓存带容量上限（LRU 淘汰）与删除清理，防止重度图片使用下无界增长。

use floatpaste_core::domain::clip_item::ClipItemSummary;
use floatpaste_core::services::image_decode::decode_limits;
use floatpaste_core::state::CoreState;
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::picker::App;

/// 缓存容量上限（条）。单张 144×144 RGBA 约 81KB，512 条 ≈ 42MB 封顶
const MAX_CACHE_ENTRIES: usize = 512;

thread_local! {
    /// 缩略图缓存：id -> (Some(图像) | None(解码失败哨兵，避免反复重试), 最近使用序号)
    static CACHE: std::cell::RefCell<std::collections::HashMap<String, (Option<slint::Image>, u64)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// 最近使用序号：命中/写入时递增，容量超限时淘汰序号最小项
    static USE_TICK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// 列表版本号：异步缩略图回来时校验列表是否已变
    static LIST_VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn next_tick() -> u64 {
    USE_TICK.with(|tick| {
        tick.set(tick.get() + 1);
        tick.get()
    })
}

/// 读取缓存的缩略图（未解码/失败返回 None），命中时刷新最近使用序号
pub fn cached(id: &str) -> Option<slint::Image> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let entry = cache.get_mut(id)?;
        entry.1 = next_tick();
        entry.0.clone()
    })
}

/// 是否已在缓存中（含失败哨兵）
pub fn contains(id: &str) -> bool {
    CACHE.with(|cache| cache.borrow().contains_key(id))
}

/// 写入缓存（事件循环线程）；超容量时淘汰最久未用项
pub fn insert(id: String, image: Option<slint::Image>) {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.insert(id, (image, next_tick()));
        if cache.len() > MAX_CACHE_ENTRIES {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (_, tick))| *tick)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
    });
}

/// 条目删除后同步清缓存，避免已删条目的缩略图滞留内存
pub fn evict(id: &str) {
    CACHE.with(|cache| {
        cache.borrow_mut().remove(id);
    });
}

/// 列表版本号：递增并返回新值（重建行模型时调用）
pub fn bump_list_version() -> u64 {
    LIST_VERSION.with(|version| {
        version.set(version.get() + 1);
        version.get()
    })
}

/// 读取当前列表版本号（异步回填前记录，回来时校验）
pub fn list_version() -> u64 {
    LIST_VERSION.with(|version| version.get())
}

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

/// 带解码上限读取：超限（异常声明尺寸/巨量像素分配）按解码失败处理
fn decode(core: &CoreState, image_path: &str, max_size: Option<(u32, u32)>) -> Option<RawImage> {
    let path = core
        .image_storage
        .resolve_existing_image_path(image_path)
        .ok()?;
    let metadata = std::fs::metadata(&path).ok()?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return None;
    }

    let file = std::fs::File::open(&path).ok()?;
    let mut reader = image::ImageReader::new(std::io::BufReader::new(file));
    reader.limits(decode_limits());
    let mut image = reader
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    if let Some((max_width, max_height)) = max_size {
        image = image.thumbnail(max_width, max_height);
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
    decode(core, image_path, Some((THUMB_SIZE, THUMB_SIZE)))
}

/// tooltip 预览：缩放到显示上限内（物理像素）。显示按逻辑尺寸缩放渲染，
/// 超出显示上限的原像素不可见，无需解码到全尺寸
pub fn load_preview_image_raw(
    core: &CoreState,
    image_path: &str,
    max_width: u32,
    max_height: u32,
) -> Option<RawImage> {
    decode(core, image_path, Some((max_width, max_height)))
}

/// 编辑器查看用原图（不缩放；仍受解码上限保护）
pub fn load_full_image_raw(core: &CoreState, image_path: &str) -> Option<RawImage> {
    decode(core, image_path, None)
}

/// 事件循环线程：原始像素 → Slint 图像
pub fn image_from_rgba(raw: RawImage) -> slint::Image {
    let buffer =
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&raw.pixels, raw.width, raw.height);
    slint::Image::from_rgba8(buffer)
}

/// 异步补齐列表缺失的图片缩略图（速贴 / 搜索共用）：
/// 工作线程解码缺失项，回到事件循环后校验列表版本未变再写缓存，
/// 最后由 `rebuild` 重建各自窗口的行模型。
pub(crate) fn ensure(app: &App, items: &[ClipItemSummary], rebuild: impl FnOnce(&App) + Send + 'static) {
    let missing: Vec<(String, String)> = items
        .iter()
        .filter(|item| item.r#type == "image" && item.image_path.is_some())
        .filter(|item| !contains(&item.id))
        .filter_map(|item| {
            let path = item.image_path.clone()?;
            Some((item.id.clone(), path))
        })
        .collect();
    if missing.is_empty() {
        return;
    }

    let version = list_version();
    let core = app.core().clone();
    let app_for_cb = app.clone();
    std::thread::spawn(move || {
        // 跨线程只传原始像素；slint::Image 非 Send，事件循环侧再构造
        let mut decoded: Vec<(String, Option<RawImage>)> = Vec::new();
        for (id, path) in missing {
            let raw = load_thumbnail_raw(&core, &path);
            decoded.push((id, raw));
        }

        let _ = slint::invoke_from_event_loop(move || {
            if list_version() != version {
                return; // 列表已刷新，等待下一轮 ensure
            }
            for (id, raw) in decoded {
                insert(id, raw.map(image_from_rgba));
            }
            rebuild(&app_for_cb);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_evicts_least_recently_used_beyond_capacity() {
        for index in 0..MAX_CACHE_ENTRIES {
            insert(format!("cap-{index}"), None);
        }
        // 触达最早写入项使其保活，再插入一项：被淘汰的应是最久未用的第二项
        assert!(cached("cap-0").is_none());
        insert("cap-new".to_string(), None);

        assert!(contains("cap-0"));
        assert!(!contains("cap-1"));
        assert!(contains("cap-new"));
        evict("cap-0");
        assert!(!contains("cap-0"));
    }

    #[test]
    fn insert_without_touch_evicts_oldest() {
        for index in 0..=MAX_CACHE_ENTRIES {
            insert(format!("old-{index}"), None);
        }
        assert!(!contains("old-0"));
        assert!(contains(&format!("old-{MAX_CACHE_ENTRIES}")));
    }
}
