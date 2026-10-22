use std::{mem::size_of, slice};

use super::clipboard_formats::{write_formats, ClipboardSnapshot};
use crate::domain::error::AppError;
use windows::{
    core::BOOL,
    Win32::{Foundation::POINT, UI::Shell::DROPFILES},
};

pub fn write_file_paths_to_clipboard(
    file_paths: &[String],
    owner: Option<isize>,
    rollback: Option<&ClipboardSnapshot>,
) -> Result<u32, AppError> {
    let header = DROPFILES {
        pFiles: size_of::<DROPFILES>() as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: BOOL(0),
        fWide: BOOL(1),
    };
    // DROPFILES 由五个连续的 32 位字段组成，无未初始化填充。
    let mut bytes = unsafe {
        slice::from_raw_parts(
            (&header as *const DROPFILES).cast::<u8>(),
            size_of::<DROPFILES>(),
        )
        .to_vec()
    };
    for path in file_paths {
        bytes.extend(path.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
    }
    bytes.extend([0, 0]);
    write_formats(&[(15, bytes)], owner, None, rollback)?
        .ok_or_else(|| AppError::Clipboard("发布后剪贴板发生变化，已取消上屏".into()))
}
