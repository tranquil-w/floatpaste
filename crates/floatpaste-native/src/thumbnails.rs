//! 图片加载与缩放：解码在工作线程执行，跨线程只传原始 RGBA 像素
//! （slint::Image 内部是 Rc 引用计数，不能跨线程），
//! 事件循环侧用 [`image_from_rgba`] 构造共享像素缓冲。
//!
//! 列表缩略图缓存（id → 图像）按窗口共享：速贴与搜索展示同一批条目，
//! 共用一份解码结果与失败哨兵，避免双份内存与重复解码。

use floatpaste_core::domain::clip_item::ClipItemSummary;
use floatpaste_core::state::CoreState;
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::picker::App;

thread_local! {
    /// 缩略图缓存：id -> Some(图像) | None(解码失败哨兵，避免反复重试)
    static CACHE: std::cell::RefCell<std::collections::HashMap<String, Option<slint::Image>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// 列表版本号：异步缩略图回来时校验列表是否已变
    static LIST_VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// 读取缓存的缩略图（未解码/失败返回 None）
pub fn cached(id: &str) -> Option<slint::Image> {
    CACHE
        .with(|cache| cache.borrow().get(id).cloned())
        .flatten()
}

/// 是否已在缓存中（含失败哨兵）
pub fn contains(id: &str) -> bool {
    CACHE.with(|cache| cache.borrow().contains_key(id))
}

/// 写入缓存（事件循环线程）
pub fn insert(id: String, image: Option<slint::Image>) {
    CACHE.with(|cache| {
        cache.borrow_mut().insert(id, image);
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
