//! 单次 OpenClipboard 会话内复制受支持表示，解析和磁盘访问在锁外进行。
use std::{cell::RefCell, ptr::copy_nonoverlapping, slice};

use windows::{
    core::w,
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
                GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
                RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{
                GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT,
            },
        },
        UI::{
            Shell::{DragQueryFileW, HDROP},
            WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
        },
    },
};

use super::{
    clipboard_error::map_clipboard_error,
    image_clipboard::{decode_dib_bytes, decode_png_bytes, ClipboardImageData},
};
use crate::services::input_limits::{
    MAX_FILE_COUNT, MAX_FORMAT_BYTES, MAX_PATH_UNITS, MAX_SNAPSHOT_BYTES, MAX_TEXT_BYTES,
};
use crate::{domain::error::AppError, services::html_format::MAX_HTML_BYTES};

const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;

#[derive(Debug, Clone)]
pub struct ClipboardSnapshot {
    pub sequence: u32,
    pub html_available: bool,
    pub excluded_from_history: bool,
    pub source_app: Option<String>,
    formats: Vec<(u32, Vec<u8>)>,
    pub files: Option<Vec<String>>,
    html_format: u32,
    png_format: u32,
}

struct OpenSession;
impl Drop for OpenSession {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn open_clipboard(owner: HWND) -> Result<OpenSession, AppError> {
    for attempt in 0..5 {
        match unsafe { OpenClipboard(Some(owner)) } {
            Ok(()) => return Ok(OpenSession),
            Err(error) if attempt < 4 && error.code().0 == 0x80070005u32 as i32 => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => return Err(map_clipboard_error(error)),
        }
    }
    unreachable!()
}

struct ClipboardOwnerWindow(HWND);
impl Drop for ClipboardOwnerWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

thread_local! {
    // 属主存活至调用线程退出，避免每次写入后销毁属主改变剪贴板版本。
    static FALLBACK_OWNER: RefCell<Option<ClipboardOwnerWindow>> = const { RefCell::new(None) };
}

fn fallback_owner() -> Result<HWND, AppError> {
    FALLBACK_OWNER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(owner) = slot.as_ref() {
            return Ok(owner.0);
        }
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("FloatPasteClipboard"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(map_clipboard_error)?;
        *slot = Some(ClipboardOwnerWindow(hwnd));
        Ok(hwnd)
    })
}

pub fn clipboard_sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub(super) fn registered_format(name: windows::core::PCWSTR) -> Result<u32, AppError> {
    let format = unsafe { RegisterClipboardFormatW(name) };
    if format == 0 {
        Err(AppError::Clipboard("注册剪贴板格式失败".into()))
    } else {
        Ok(format)
    }
}

impl ClipboardSnapshot {
    /// 恢复快照要求完整保留白名单内的 HTML；超限须在清空剪贴板前拒绝。
    pub fn capture(require_restorable: bool) -> Result<Self, AppError> {
        Self::capture_with_policy(require_restorable, &[])
    }

    pub fn capture_for_history(excluded_apps: &[String]) -> Result<Self, AppError> {
        Self::capture_with_policy(false, excluded_apps)
    }

    fn capture_with_policy(
        require_restorable: bool,
        excluded_apps: &[String],
    ) -> Result<Self, AppError> {
        let html_format = registered_format(w!("HTML Format"))?;
        let png_format = registered_format(w!("PNG"))?;
        let _session = open_clipboard(fallback_owner()?)?;
        let sequence = clipboard_sequence();
        let source_app = super::active_app::ActiveAppResolver::process_name_for_window(
            unsafe { GetClipboardOwner() }.unwrap_or_default(),
        );
        let mut formats = read_privacy_formats()?;
        let exclude_format = registered_format(w!("ExcludeClipboardContentFromMonitorProcessing"))?;
        let history_format = registered_format(w!("CanIncludeInClipboardHistory"))?;
        let excluded_from_history = source_app.as_ref().is_some_and(|source| {
            excluded_apps
                .iter()
                .any(|app| app.eq_ignore_ascii_case(source))
        }) || formats.iter().any(|(format, bytes)| {
            *format == exclude_format
                || (*format == history_format && bytes.as_slice() == [0, 0, 0, 0])
        });
        if !require_restorable && excluded_from_history {
            return Ok(Self {
                sequence,
                html_available: false,
                excluded_from_history,
                source_app,
                formats: Vec::new(),
                files: None,
                html_format,
                png_format,
            });
        }
        let mut total_bytes = 0usize;
        let mut files = None;
        let mut html_available = false;
        for format in [
            CF_HDROP,
            html_format,
            CF_UNICODETEXT,
            png_format,
            CF_DIBV5,
            CF_DIB,
        ] {
            if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
                continue;
            }
            if format == html_format {
                html_available = true;
            }
            let handle = match unsafe { GetClipboardData(format) } {
                Ok(handle) => handle,
                Err(error)
                    if [png_format, CF_DIBV5, CF_DIB].contains(&format)
                        && ![0x80070005u32 as i32, 0x8007058Au32 as i32]
                            .contains(&error.code().0) =>
                {
                    // 系统可能宣告可合成 DIBV5，但实际转换没有可读句柄；
                    // 保留本轮可读的 PNG/DIB，不能因此丢弃整份多格式快照。
                    tracing::debug!(format, error = %error, "图片补充表示不可读，保留其他表示");
                    continue;
                }
                Err(error) if format == html_format && !require_restorable => {
                    tracing::debug!(error = %error, "HTML 载荷不可读，保留其他表示");
                    continue;
                }
                Err(error) => {
                    return Err(AppError::Clipboard(format!(
                        "读取格式 {format} 失败：{error}"
                    )))
                }
            };
            let global = HGLOBAL(handle.0);
            let size = unsafe { GlobalSize(global) };
            // 为 NUL 与分配器对齐预留少量空间；载荷本身仍严格限制 4 MiB。
            if format == html_format && size > MAX_HTML_BYTES + 16 {
                if require_restorable {
                    return Err(AppError::Clipboard(
                        "原剪贴板 HTML 超过 4 MiB，无法安全恢复，本次上屏已取消".into(),
                    ));
                }
                continue;
            }
            if size == 0 {
                continue;
            }
            if size > MAX_FORMAT_BYTES
                || (format == CF_UNICODETEXT && size > MAX_TEXT_BYTES * 2 + 2)
            {
                return Err(AppError::InputLimit("剪贴板单格式载荷超过限制"));
            }
            total_bytes = total_bytes
                .checked_add(size)
                .filter(|total| *total <= MAX_SNAPSHOT_BYTES)
                .ok_or(AppError::InputLimit("剪贴板快照超过 512 MiB"))?;
            let ptr = unsafe { GlobalLock(global) }.cast::<u8>();
            if ptr.is_null() {
                if format == html_format && !require_restorable {
                    continue;
                }
                return Err(AppError::Clipboard("读取剪贴板载荷失败".into()));
            }
            let mut bytes = unsafe { slice::from_raw_parts(ptr, size).to_vec() };
            unsafe {
                let _ = GlobalUnlock(global);
            }
            if format == html_format {
                // GlobalSize 可能包含分配器填充；NUL 后不是 HTML 载荷身份的一部分。
                if let Some(end) = bytes.iter().position(|b| *b == 0) {
                    bytes.truncate(end);
                }
                if bytes.len() > MAX_HTML_BYTES {
                    if require_restorable {
                        return Err(AppError::Clipboard(
                            "原剪贴板 HTML 超过 4 MiB，无法安全恢复，本次上屏已取消".into(),
                        ));
                    }
                    continue;
                }
            }
            if format == CF_HDROP {
                let hdrop = HDROP(handle.0);
                let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
                if count as usize > MAX_FILE_COUNT {
                    return Err(AppError::InputLimit("文件列表超过 1024 项"));
                }
                let mut paths = Vec::new();
                for index in 0..count {
                    let len = unsafe { DragQueryFileW(hdrop, index, None) };
                    if len as usize > MAX_PATH_UNITS {
                        return Err(AppError::InputLimit("文件路径超过长度限制"));
                    }
                    let mut buf = vec![0u16; len as usize + 1];
                    let copied = unsafe { DragQueryFileW(hdrop, index, Some(&mut buf)) };
                    if copied > 0 {
                        paths.push(String::from_utf16_lossy(&buf[..copied as usize]));
                    }
                }
                if !paths.is_empty() {
                    crate::services::input_limits::validate_files(&paths)?;
                    files = Some(paths);
                }
            }
            formats.push((format, bytes));
        }
        if clipboard_sequence() != sequence {
            return Err(AppError::Clipboard("读取期间剪贴板版本发生变化".into()));
        }
        Ok(Self {
            sequence,
            html_available,
            excluded_from_history,
            source_app,
            formats,
            files,
            html_format,
            png_format,
        })
    }

    pub fn html(&self) -> Option<&[u8]> {
        self.bytes(self.html_format)
    }
    fn bytes(&self, format: u32) -> Option<&[u8]> {
        self.formats
            .iter()
            .find(|(f, _)| *f == format)
            .map(|(_, b)| b.as_slice())
    }
    pub fn text(&self) -> Option<String> {
        let bytes = self.bytes(CF_UNICODETEXT)?;
        let text: Vec<_> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|c| *c != 0)
            .collect();
        Some(String::from_utf16_lossy(&text))
    }
    pub fn image(&self) -> Option<ClipboardImageData> {
        if let Some(bytes) = self.bytes(self.png_format) {
            if let Ok(image) = decode_png_bytes(bytes.to_vec()) {
                return Some(image);
            }
        }
        for format in [CF_DIBV5, CF_DIB] {
            if let Some(bytes) = self.bytes(format) {
                if let Ok(image) = decode_dib_bytes(bytes) {
                    return Some(image);
                }
            }
        }
        None
    }

    /// 序列号在持有系统剪贴板锁后比较，避免检查与清空之间插入新复制。
    pub fn restore(
        &self,
        owner: Option<isize>,
        expected: Option<u32>,
    ) -> Result<Option<u32>, AppError> {
        write_formats(&self.formats, owner, expected, None)
    }
}

/// 隐私标记先于正文读取；恢复快照同时保留系统历史和云同步策略。
fn read_privacy_formats() -> Result<Vec<(u32, Vec<u8>)>, AppError> {
    let mut formats = Vec::new();
    for (name, presence_only) in [
        (w!("ExcludeClipboardContentFromMonitorProcessing"), true),
        (w!("CanIncludeInClipboardHistory"), false),
        (w!("CanUploadToCloudClipboard"), false),
    ] {
        let format = registered_format(name)?;
        if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
            continue;
        }
        let bytes = if presence_only {
            vec![1]
        } else {
            let handle = unsafe { GetClipboardData(format) }.map_err(map_clipboard_error)?;
            let global = HGLOBAL(handle.0);
            if unsafe { GlobalSize(global) } < 4 {
                return Err(AppError::Clipboard("剪贴板隐私标记无效，已取消读取".into()));
            }
            let ptr = unsafe { GlobalLock(global) }.cast::<u8>();
            if ptr.is_null() {
                return Err(AppError::Clipboard("无法读取剪贴板隐私标记".into()));
            }
            let bytes = unsafe { slice::from_raw_parts(ptr, 4) }.to_vec();
            unsafe {
                let _ = GlobalUnlock(global);
            }
            bytes
        };
        formats.push((format, bytes));
    }
    Ok(formats)
}

pub fn write_text_formats(
    text: &str,
    html: Option<&[u8]>,
    owner: Option<isize>,
) -> Result<u32, AppError> {
    write_text_formats_with_rollback(text, html, owner, None)
}

pub fn write_text_formats_with_rollback(
    text: &str,
    html: Option<&[u8]>,
    owner: Option<isize>,
    rollback: Option<&ClipboardSnapshot>,
) -> Result<u32, AppError> {
    let mut formats = Vec::new();
    if let Some(html) = html {
        if html.len() > MAX_HTML_BYTES {
            return Err(AppError::Clipboard("HTML 超过 4 MiB".into()));
        }
        formats.push((registered_format(w!("HTML Format"))?, html.to_vec()));
    }
    let text_bytes: Vec<_> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    formats.push((CF_UNICODETEXT, text_bytes));
    write_formats(&formats, owner, None, rollback)?
        .ok_or_else(|| AppError::Clipboard("剪贴板发布被取消".into()))
}

struct PreparedHandle(HGLOBAL);
impl Drop for PreparedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = GlobalFree(Some(self.0));
            }
        }
    }
}

fn prepare_formats(formats: &[(u32, Vec<u8>)]) -> Result<Vec<(u32, PreparedHandle)>, AppError> {
    // 全部内存准备成功后才清空；成功转移的句柄不再由本进程释放。
    let mut prepared = Vec::new();
    for (format, bytes) in formats {
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len() + 1) }
            .map_err(map_clipboard_error)?;
        let handle = PreparedHandle(memory);
        let ptr = unsafe { GlobalLock(memory) }.cast::<u8>();
        if ptr.is_null() {
            return Err(AppError::Clipboard("分配剪贴板载荷失败".into()));
        }
        unsafe {
            copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            let _ = GlobalUnlock(memory);
        }
        prepared.push((*format, handle));
    }
    Ok(prepared)
}

pub(super) fn write_formats(
    formats: &[(u32, Vec<u8>)],
    owner: Option<isize>,
    expected: Option<u32>,
    rollback: Option<&ClipboardSnapshot>,
) -> Result<Option<u32>, AppError> {
    let mut prepared = prepare_formats(formats)?;
    let mut recovery = rollback
        .map(|snapshot| prepare_formats(&snapshot.formats))
        .transpose()?;
    // EmptyClipboard 后 SetClipboardData 要求非空属主；无界面调用使用消息窗。
    let owner = if let Some(owner) = owner.filter(|h| *h != 0) {
        HWND(owner as *mut _)
    } else {
        fallback_owner()?
    };
    let session = open_clipboard(owner)?;
    if expected.is_some_and(|s| clipboard_sequence() != s) {
        return Ok(None);
    }
    unsafe {
        EmptyClipboard().map_err(map_clipboard_error)?;
    }
    if let Err(error) = publish_prepared(&mut prepared) {
        if let Some(recovery) = &mut recovery {
            // 仍持有同一系统剪贴板锁，失败回滚不会覆盖用户的新复制。
            let recovery_result = unsafe { EmptyClipboard().map_err(map_clipboard_error) }
                .and_then(|_| publish_prepared(recovery));
            if let Err(recovery_error) = recovery_result {
                tracing::warn!(error = %recovery_error, "多格式发布失败后的快照恢复失败");
            }
        }
        return Err(error);
    }
    drop(session);
    // CloseClipboard 可生成兼容格式并改变序列号。重新持锁核验属主，
    // 仅在仍为本次发布时取最终版本，避免误把期间用户复制的版本当作本次写入。
    let _verification = open_clipboard(fallback_owner()?)?;
    if unsafe { GetClipboardOwner() }.ok() != Some(owner) {
        return Ok(None);
    }
    Ok(Some(clipboard_sequence()))
}

fn publish_prepared(prepared: &mut [(u32, PreparedHandle)]) -> Result<(), AppError> {
    for (format, handle) in prepared {
        unsafe {
            SetClipboardData(*format, Some(HANDLE(handle.0 .0))).map_err(map_clipboard_error)?;
        }
        handle.0 = HGLOBAL::default();
    }
    Ok(())
}
