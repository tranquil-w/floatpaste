pub mod clip_display;
pub mod clip_service;
pub mod dedup_service;
pub mod history_service;
pub mod image_decode;
pub mod image_storage;
pub mod input_limits;
pub mod list_navigation;
pub mod normalize_service;
pub mod paste_support;
pub mod picker_position_service;
pub mod privacy_service;
pub mod retention_service;

pub mod tag_service;
pub mod text_file_storage;
pub mod text_identity;
pub mod text_kind;
pub mod time_format;

pub(crate) fn digest_to_hex(bytes: impl Iterator<Item = u8>) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

pub mod html_format;
