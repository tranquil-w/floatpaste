use std::{mem::size_of, ptr::copy_nonoverlapping};

use tracing::debug;
use windows::{
    core::{Error as WindowsError, BOOL},
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, POINT},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetPriorityClipboardFormat,
                OpenClipboard, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT},
        },
        UI::Shell::{DragQueryFileW, DROPFILES, HDROP},
    },
};

use crate::domain::error::AppError;

const CF_HDROP_FORMAT: u32 = 15;

pub fn read_file_paths_from_clipboard() -> Result<Option<Vec<String>>, AppError> {
    unsafe {
        if !clipboard_format_available(CF_HDROP_FORMAT)? {
            debug!("CF_HDROP 格式当前不可用");
            return Ok(None);
        }

        OpenClipboard(None).map_err(map_clipboard_error)?;
        let result = (|| {
            let handle = GetClipboardData(CF_HDROP_FORMAT).map_err(map_clipboard_error)?;
            let hdrop = HDROP(handle.0);
            let file_count = DragQueryFileW(hdrop, u32::MAX, None);
            if file_count == 0 {
                return Ok(None);
            }

            let mut file_paths = Vec::with_capacity(file_count as usize);
            for index in 0..file_count {
                let required_len = DragQueryFileW(hdrop, index, None);
                if required_len == 0 {
                    continue;
                }

                let mut buffer = vec![0u16; required_len as usize + 1];
                let copied_len = DragQueryFileW(hdrop, index, Some(buffer.as_mut_slice()));
                if copied_len == 0 {
                    continue;
                }

                let path = String::from_utf16_lossy(&buffer[..copied_len as usize]);
                if !path.is_empty() {
                    file_paths.push(path);
                }
            }

            if file_paths.is_empty() {
                Ok(None)
            } else {
                Ok(Some(file_paths))
            }
        })();
        let _ = CloseClipboard();
        result
    }
}

fn clipboard_format_available(format: u32) -> Result<bool, AppError> {
    unsafe {
        match GetPriorityClipboardFormat(&[format]) {
            value if value == format as i32 => Ok(true),
            -1 => Ok(false),
            0 => Err(AppError::Clipboard(format!(
                "检查 CF_HDROP 剪贴板格式失败: {}",
                WindowsError::from_win32()
            ))),
            other => Err(AppError::Clipboard(format!(
                "检查 CF_HDROP 剪贴板格式返回了未知结果: {other}"
            ))),
        }
    }
}

pub fn write_file_paths_to_clipboard(file_paths: &[String]) -> Result<(), AppError> {
    let mut encoded_paths = Vec::new();
    for path in file_paths {
        encoded_paths.extend(path.encode_utf16());
        encoded_paths.push(0);
    }
    encoded_paths.push(0);

    let header_size = size_of::<DROPFILES>();
    let path_bytes_len = encoded_paths.len() * size_of::<u16>();
    let total_size = header_size + path_bytes_len;

    unsafe {
        let handle =
            GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total_size).map_err(map_clipboard_error)?;
        if handle.is_invalid() {
            return Err(AppError::Clipboard("分配文件剪贴板内存失败".to_string()));
        }

        let memory = GlobalLock(handle);
        if memory.is_null() {
            let _ = free_global_memory(handle);
            return Err(AppError::Clipboard("锁定文件剪贴板内存失败".to_string()));
        }

        let header = memory.cast::<DROPFILES>();
        (*header).pFiles = header_size as u32;
        (*header).pt = POINT { x: 0, y: 0 };
        (*header).fNC = BOOL(0);
        (*header).fWide = BOOL(1);

        let path_memory = memory.cast::<u8>().add(header_size).cast::<u16>();
        copy_nonoverlapping(encoded_paths.as_ptr(), path_memory, encoded_paths.len());
        let _ = GlobalUnlock(handle);

        if let Err(error) = OpenClipboard(None) {
            let _ = free_global_memory(handle);
            return Err(map_clipboard_error(error));
        }
        let result = (|| {
            EmptyClipboard().map_err(map_clipboard_error)?;
            SetClipboardData(CF_HDROP_FORMAT, Some(HANDLE(handle.0)))
                .map_err(map_clipboard_error)?;
            Ok(())
        })();
        let _ = CloseClipboard();
        if result.is_err() {
            let _ = free_global_memory(handle);
        }
        result
    }
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
