//! 上屏支持：把剪辑项安全写入系统剪贴板，以及粘贴后的剪贴板快照恢复。
//!
//! 这里只承载与窗口框架无关的平台流程；窗口会话与目标焦点由 GUI 壳编排。
//! 壳层须先趁属主窗口存活发布剪贴板，成功后再隐藏来源窗口、恢复目标焦点。

use std::{
    sync::{Mutex, MutexGuard},
    thread,
    time::Duration,
};

use tracing::warn;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

use crate::{
    domain::{
        clip_item::{ClipItemDetail, PasteMode, PasteOption},
        error::AppError,
    },
    platform::windows::{
        clipboard_formats::{clipboard_sequence, write_text_formats_with_rollback},
        file_clipboard::write_file_paths_to_clipboard,
        image_clipboard::{write_image_to_clipboard, ClipboardImageData},
    },
    services::normalize_service::{analyze_file_paths, NormalizeService},
    state::CoreState,
};

pub use crate::platform::windows::clipboard_formats::ClipboardSnapshot;

/// 发布、延迟恢复和注入前的判定共用串行门；不在等待期间占锁。
static PASTE_GATE: Mutex<()> = Mutex::new(());
pub fn lock_paste_operation() -> Result<MutexGuard<'static, ()>, AppError> {
    Ok(PASTE_GATE.lock()?)
}

pub fn capture_snapshot_if_needed(
    option: &PasteOption,
) -> Result<Option<ClipboardSnapshot>, AppError> {
    if option.restore_clipboard_after_paste && option.paste_to_target {
        Ok(Some(capture_clipboard_snapshot()?))
    } else {
        Ok(None)
    }
}

pub fn capture_clipboard_snapshot() -> Result<ClipboardSnapshot, AppError> {
    ClipboardSnapshot::capture(true)
}

pub fn schedule_clipboard_restore(
    core: &CoreState,
    snapshot: ClipboardSnapshot,
    owner_hwnd: Option<isize>,
    session_id: u64,
    operation_id: u64,
    written_sequence: u32,
    privacy_generation: u64,
) -> Result<(), AppError> {
    let core = core.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(550));
        let result = (|| {
            let _gate = lock_paste_operation()?;
            let privacy = core.privacy_read()?;
            if privacy.session_locked
                || privacy.generation != privacy_generation
                || core.is_quitting()
            {
                return Ok(());
            }
            if clipboard_sequence() != written_sequence {
                return Ok(());
            }
            suppress_clipboard_snapshot(&core, &snapshot)?;
            snapshot.restore(owner_hwnd, Some(written_sequence))?;
            Ok::<_, AppError>(())
        })();
        if let Err(error) = result {
            warn!(session_id, operation_id, error = %error, "恢复剪贴板失败");
        }
    });
    Ok(())
}

fn suppress_clipboard_snapshot(
    core: &CoreState,
    snapshot: &ClipboardSnapshot,
) -> Result<(), AppError> {
    let extracted = snapshot
        .html()
        .and_then(super::html_format::extract_html_text);
    let plain = snapshot.text();
    if let Some(text) = plain
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .or(extracted.as_deref())
    {
        suppress_text(core, text, None)?;
        if extracted.is_some() {
            suppress_text(core, text, snapshot.html())?;
        }
    }
    if let Some(image) = snapshot.image() {
        let prepared = core.image_storage.prepare_image(
            &image.rgba,
            image.width,
            image.height,
            image.png_bytes.as_deref(),
        )?;
        if let Some(normalized) = NormalizeService::normalize_image(
            None,
            Some(prepared.width),
            Some(prepared.height),
            Some(prepared.image_format),
            Some(prepared.file_size),
            Some(prepared.content_hash),
            None,
        ) {
            core.self_write_guard()
                .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
        }
    }
    if let Some(paths) = snapshot.files.as_ref() {
        let stats = analyze_file_paths(paths);
        if let Some(normalized) = NormalizeService::normalize_files(
            paths.clone(),
            stats.directory_count,
            stats.total_size,
            None,
        ) {
            core.self_write_guard()
                .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
        }
    }
    Ok(())
}

fn suppress_text(core: &CoreState, text: &str, html: Option<&[u8]>) -> Result<(), AppError> {
    core.self_write_guard().suppress_hash(
        super::text_identity::text_formats_hash(text, html),
        Duration::from_secs(3),
    )
}

/// 发布失败时尝试恢复原快照；调用方不得继续注入 Ctrl+V。
pub fn publish_item(
    core: &CoreState,
    detail: &ClipItemDetail,
    mode: PasteMode,
    owner: Option<isize>,
    previous: Option<&ClipboardSnapshot>,
) -> Result<u32, AppError> {
    publish_item_if_current(core, detail, mode, owner, previous, None)
}

pub fn publish_item_if_current(
    core: &CoreState,
    detail: &ClipItemDetail,
    mode: PasteMode,
    owner: Option<isize>,
    previous: Option<&ClipboardSnapshot>,
    generation: Option<u64>,
) -> Result<u32, AppError> {
    let privacy = core.privacy_read()?;
    if privacy.session_locked
        || generation.is_some_and(|value| value != privacy.generation)
        || core.is_quitting()
    {
        return Err(AppError::OperationCancelled);
    }
    if mode == PasteMode::TextFile {
        return Err(AppError::Message("粘贴为文件需要提供文件名".into()));
    }
    if matches!(mode, PasteMode::RevealInFolder | PasteMode::OpenInBrowser) {
        // 非剪贴板形态：发布链路拒绝，由上屏流程分流执行，防止漏分流
        // 时静默按本体上屏
        return Err(AppError::Message("外部打开动作不经剪贴板发布".into()));
    }
    let needs_rollback =
        mode == PasteMode::Original && (detail.has_html || detail.r#type == "image");
    let rollback = if needs_rollback && previous.is_none() {
        Some(ClipboardSnapshot::capture(true)?)
    } else {
        None
    };
    let previous = previous.or(rollback.as_ref());
    if let Some(snapshot) = previous {
        suppress_clipboard_snapshot(core, snapshot)?;
    }
    if detail.r#type == "text" {
        if mode == PasteMode::Original && detail.has_html && detail.html_bytes.is_none() {
            return Err(AppError::Clipboard("无法读取条目原格式，请重试".into()));
        }
        let html = if mode == PasteMode::Original {
            detail.html_bytes.as_deref()
        } else {
            None
        };
        suppress_text(core, &detail.full_text, html)?;
        suppress_text(core, &detail.full_text, None)?;
        return write_text_formats_with_rollback(&detail.full_text, html, owner, previous);
    }
    write_item_to_clipboard(core, detail, mode, owner, previous)
}

/// 文本附件生成与发布同受隐私读守卫保护；发布失败撤销本次生成文件。
pub fn publish_text_file_if_current(
    core: &CoreState,
    detail: &ClipItemDetail,
    name: &str,
    owner: Option<isize>,
    previous: Option<&ClipboardSnapshot>,
    generation: u64,
) -> Result<u32, AppError> {
    let privacy = core.privacy_read()?;
    if privacy.session_locked || privacy.generation != generation || core.is_quitting() {
        return Err(AppError::OperationCancelled);
    }
    if detail.r#type != "text" {
        return Err(AppError::Message("只有文本条目可以生成文本文件".into()));
    }
    let path = core.text_file_storage.create(name, &detail.full_text)?;
    let result = (|| {
        if let Some(snapshot) = previous {
            suppress_clipboard_snapshot(core, snapshot)?;
        }
        let mut file = detail.clone();
        file.r#type = "file".into();
        file.file_paths = vec![path.to_string_lossy().into_owned()];
        file.directory_count = 0;
        file.total_size = Some(detail.full_text.len() as i64);
        write_item_to_clipboard(core, &file, PasteMode::Original, owner, previous)
    })();
    if result.is_err() {
        if let Err(error) = core.text_file_storage.discard(&path) {
            tracing::warn!(error = %error, "未发布的文本附件清理失败，保留至缓存清理");
        }
    }
    result
}

/// 把剪辑项内容写入系统剪贴板，并登记自写抑制。
///
/// `owner_hwnd`：承载 OpenClipboard 的窗口句柄（无窗口时使用线程内消息窗）。
/// 壳层应在调用前传自己还可见的窗口句柄，隐藏窗口后再回贴目标。
/// `mode` 由共用能力解析器决定，图片/文件的 PathText 发布路径文本。
fn write_item_to_clipboard(
    core: &CoreState,
    detail: &ClipItemDetail,
    mode: PasteMode,
    owner_hwnd: Option<isize>,
    previous: Option<&ClipboardSnapshot>,
) -> Result<u32, AppError> {
    match detail.r#type.as_str() {
        "image" => {
            let Some(image_path) = detail.image_path.as_deref() else {
                return Err(AppError::Message(
                    "图片记录缺少可恢复的文件引用".to_string(),
                ));
            };

            if mode == PasteMode::ImageFile {
                // 图片文件形态：保存的 PNG 以文件列表（CF_HDROP）发布，
                // 目标应用按文件附件接收；文件本体必须存在
                let absolute_path = core.image_storage.resolve_existing_image_path(image_path)?;
                let path_str = absolute_path.to_string_lossy().to_string();
                let stats = analyze_file_paths(std::slice::from_ref(&path_str));
                if let Some(normalized) = NormalizeService::normalize_files(
                    vec![path_str.clone()],
                    stats.directory_count,
                    stats.total_size,
                    None,
                ) {
                    core.self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }
                return write_file_paths_to_clipboard(
                    std::slice::from_ref(&path_str),
                    owner_hwnd,
                    previous,
                );
            }

            if mode == PasteMode::PathText {
                let absolute_path = core.image_storage.resolve_existing_image_path(image_path)?;
                let path_str = absolute_path.to_string_lossy().to_string();

                if let Some(normalized) = NormalizeService::normalize_text(&path_str, None) {
                    core.self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                write_text_formats_with_rollback(&path_str, None, owner_hwnd, previous)
            } else {
                core.self_write_guard()
                    .suppress_hash(detail.hash.clone(), Duration::from_secs(3))?;
                let image = core.image_storage.load_image(image_path)?;
                write_image_to_clipboard(
                    owner_hwnd,
                    &ClipboardImageData {
                        rgba: image.rgba,
                        width: image.width,
                        height: image.height,
                        png_bytes: Some(image.png_bytes),
                    },
                    previous,
                )
            }
        }
        "file" => {
            if detail.file_paths.is_empty() {
                return Err(AppError::Message("文件记录缺少文件路径".to_string()));
            }

            // 变体「粘贴为完整路径」：路径列表作为文本上屏，不动文件本体
            if mode == PasteMode::PathText {
                let listing = file_paths_text(&detail.file_paths);
                if let Some(normalized) = NormalizeService::normalize_text(&listing, None) {
                    core.self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                return write_text_formats_with_rollback(&listing, None, owner_hwnd, previous);
            }

            // 变体「粘贴为文件名」：仅文件名逐行文本，同样不动文件本体
            if mode == PasteMode::FileNameText {
                let listing = file_names_text(&detail.file_paths);
                if let Some(normalized) = NormalizeService::normalize_text(&listing, None) {
                    core.self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                return write_text_formats_with_rollback(&listing, None, owner_hwnd, previous);
            }

            if let Some(normalized) = NormalizeService::normalize_files(
                detail.file_paths.clone(),
                detail.directory_count,
                detail.total_size,
                None,
            ) {
                core.self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }

            write_file_paths_to_clipboard(&detail.file_paths, owner_hwnd, previous)
        }
        other => Err(AppError::Message(format!("暂不支持 {other} 类型的写回"))),
    }
}

/// 文件条目次级上屏的文本形态：逐行路径（CRLF 分隔对齐 Windows 文本
/// 惯例，记事本等按行消费的目标应用才能正确换行）
fn file_paths_text(paths: &[String]) -> String {
    paths.join("\r\n")
}

/// 文件条目「仅文件名」形态的文本：逐行文件名，与路径文本同为 CRLF
/// 分隔。路径没有文件名分量（如根目录）时回退整条路径，条目不凭空少行
fn file_names_text(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| {
            std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone())
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// 恢复原剪贴板时不引导手动粘贴，避免用户粘贴到此前的内容。
pub fn paste_failure_hint(restore_clipboard: bool) -> &'static str {
    if restore_clipboard {
        "请重新打开速贴或搜索窗口重试。"
    } else {
        "内容已复制，请切换到需要粘贴的窗口后按 Ctrl+V。"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionOutcome {
    Injected,
    ClipboardChanged,
    PrivacyChanged,
}

/// 新复制或另一次发布到来后取消旧注入；锁失败和注入失败保留为错误。
pub fn trigger_ctrl_v_if_current(
    core: &CoreState,
    privacy_generation: u64,
    sequence: u32,
    session_id: u64,
    operation_id: u64,
) -> Result<InjectionOutcome, AppError> {
    let _gate = lock_paste_operation()?;
    let privacy = core.privacy_read()?;
    if privacy.session_locked || privacy.generation != privacy_generation || core.is_quitting() {
        return Ok(InjectionOutcome::PrivacyChanged);
    }
    inject_if_current(sequence, clipboard_sequence(), || {
        trigger_ctrl_v(session_id, operation_id)
    })
}

fn inject_if_current(
    expected: u32,
    actual: u32,
    inject: impl FnOnce() -> bool,
) -> Result<InjectionOutcome, AppError> {
    if actual != expected {
        return Ok(InjectionOutcome::ClipboardChanged);
    }
    if inject() {
        Ok(InjectionOutcome::Injected)
    } else {
        Err(AppError::Message("未能自动粘贴。".to_string()))
    }
}

/// 向前台窗口注入一次 Ctrl+V；返回注入是否成功。
pub fn trigger_ctrl_v(session_id: u64, operation_id: u64) -> bool {
    unsafe {
        let mut inputs: [INPUT; 4] = std::mem::zeroed();
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki.wVk = VK_CONTROL;
        inputs[0].Anonymous.ki.dwFlags = KEYBD_EVENT_FLAGS(0);

        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki.wVk = VK_V;
        inputs[1].Anonymous.ki.dwFlags = KEYBD_EVENT_FLAGS(0);

        inputs[2].r#type = INPUT_KEYBOARD;
        inputs[2].Anonymous.ki.wVk = VK_V;
        inputs[2].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        inputs[3].r#type = INPUT_KEYBOARD;
        inputs[3].Anonymous.ki.wVk = VK_CONTROL;
        inputs[3].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent != 4 {
            warn!(
                session_id,
                operation_id, sent, "SendInput 未发送完整的 Ctrl+V 键盘事件"
            );
        }
        sent == 4
    }
}

#[cfg(test)]
mod tests {
    use super::{
        file_names_text, file_paths_text, inject_if_current, paste_failure_hint, publish_item,
        InjectionOutcome,
    };

    #[test]
    fn changed_clipboard_cancels_without_injecting_and_failure_is_not_cancellation() {
        assert_eq!(
            inject_if_current(7, 8, || panic!("旧操作不得注入新内容")).unwrap(),
            InjectionOutcome::ClipboardChanged
        );
        let mut injections = 0;
        assert_eq!(
            inject_if_current(7, 7, || {
                injections += 1;
                true
            })
            .unwrap(),
            InjectionOutcome::Injected
        );
        assert_eq!(injections, 1);
        assert!(inject_if_current(7, 7, || false).is_err());
    }

    #[test]
    fn manual_paste_hint_requires_preserving_the_selected_content() {
        assert!(paste_failure_hint(false).contains("Ctrl+V"));
        assert!(!paste_failure_hint(true).contains("Ctrl+V"));
        assert!(!paste_failure_hint(true).contains("内容已复制"));
    }

    #[test]
    fn publish_rejects_external_actions_without_opening_clipboard() {
        use crate::domain::clip_item::{ClipItemDetail, PasteMode};
        use crate::repository::sqlite_repository::SqliteRepository;
        use crate::services::image_storage::ImageStorage;
        use crate::state::CoreState;

        let stamp = std::process::id();
        let dir = std::env::temp_dir().join(format!("floatpaste-reveal-pub-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let core = CoreState::new(
            SqliteRepository::new(&dir.join("db.sqlite")).unwrap(),
            ImageStorage::new(dir.clone()).unwrap(),
            Default::default(),
        );
        let detail = ClipItemDetail {
            has_html: false,
            html_bytes: None,
            id: "reveal".into(),
            r#type: "file".into(),
            content_preview: String::new(),
            full_text: String::new(),
            search_text: String::new(),
            source_app: None,
            is_favorited: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            hash: String::new(),
            image_path: None,
            image_width: None,
            image_height: None,
            image_format: None,
            file_size: None,
            file_paths: vec![r"C:\不存在\b.txt".into()],
            file_count: 1,
            directory_count: 0,
            total_size: None,
            tags: Vec::new(),
            text_kind: Default::default(),
        };
        for mode in [PasteMode::RevealInFolder, PasteMode::OpenInBrowser] {
            let error = publish_item(&core, &detail, mode, None, None)
                .expect_err("发布链路必须拒绝非剪贴板形态");
            assert!(error.to_string().contains("不经剪贴板发布"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_paths_text_joins_with_crlf_per_line() {
        assert_eq!(
            file_paths_text(&["C:\\a.txt".to_string(), "D:\\dir\\b.txt".to_string()]),
            "C:\\a.txt\r\nD:\\dir\\b.txt"
        );
        assert_eq!(
            file_paths_text(&["C:\\only.txt".to_string()]),
            "C:\\only.txt"
        );
        assert_eq!(file_paths_text(&[]), "");
    }

    #[test]
    fn file_names_text_strips_directories_and_keeps_line_count() {
        assert_eq!(
            file_names_text(&["C:\\a.txt".to_string(), "D:\\dir\\b.txt".to_string()]),
            "a.txt\r\nb.txt"
        );
        // 文件名与路径相同（裸文件名复制）时结果不变
        assert_eq!(file_names_text(&["readme.md".to_string()]), "readme.md");
        // 无文件名分量的路径回退整条路径，行数不丢
        assert_eq!(file_names_text(&["C:\\".to_string()]), "C:\\");
        assert_eq!(
            file_names_text(&["C:\\dir\\a.txt".to_string(), "\\\\srv\\share\\".to_string()]),
            "a.txt\r\n\\\\srv\\share\\"
        );
        assert_eq!(file_names_text(&[]), "");
    }
}
