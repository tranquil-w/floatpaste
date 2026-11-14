//! 文本文件命名窗口；暂存来源会话，取消原样返回，确认沿共享上屏链执行。
use std::cell::RefCell;
use std::sync::atomic::Ordering;

use floatpaste_core::{
    domain::{clip_item::PasteOption, error::AppError},
    platform::windows::active_app::ActiveAppResolver,
    services::text_file_storage::normalize_file_name,
    theme::{self, ThemeTokens},
};
use slint::ComponentHandle;
use tracing::warn;

use crate::{app::App, app_state::TargetSession, paste_flow::PasteSource, TextFileWindow, Theme};

struct Dialog {
    window: TextFileWindow,
    app: App,
    id: String,
    option: PasteOption,
    source: PasteSource,
    target: TargetSession,
    generation: u64,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

pub(crate) fn apply_theme(tokens: &ThemeTokens) {
    DIALOG.with(|slot| {
        if let Some(dialog) = slot.borrow().as_ref() {
            crate::theme_bridge::write_full_tokens(&dialog.window.global::<Theme>(), tokens);
        }
    });
}

pub(crate) fn open(
    app: &App,
    id: &str,
    option: PasteOption,
    source: PasteSource,
) -> Result<(), AppError> {
    if app.core().session_locked() || app.core().is_quitting() {
        return Ok(());
    }
    let generation = app.core().privacy_generation();
    let detail = app.core().repository.get_item_for_paste(id)?;
    if detail.r#type != "text" {
        return Err(AppError::Message("只有文本条目可以生成文本文件".into()));
    }
    // 再次发起动作时释放旧请求；不恢复其已过期的来源。
    close(false);
    let window =
        TextFileWindow::new().map_err(|e| AppError::Message(format!("创建文件名窗口失败：{e}")))?;
    let settings = app.state.current_settings();
    let resolved = theme::resolve_theme(settings.theme_mode, theme::system_prefers_dark());
    let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
    crate::theme_bridge::write_full_tokens(&window.global::<Theme>(), &tokens);
    window.on_cancelled(|| close(true));
    window.on_confirmed(confirm);
    window.window().on_close_requested(|| {
        close(true);
        slint::CloseRequestResponse::KeepWindowShown
    });
    let (target, source_hwnd) = match source {
        PasteSource::Picker => (
            app.state.picker_session(),
            app.state.picker_hwnd.load(Ordering::SeqCst),
        ),
        PasteSource::Search => {
            let session = app.state.search_session();
            (
                TargetSession {
                    session_id: session.session_id,
                    target_window_hwnd: session.target_window_hwnd,
                    target_focus_hwnd: None,
                },
                app.state.search_hwnd.load(Ordering::SeqCst),
            )
        }
    };
    let target_position = match crate::win32_ext::physical_rect(source_hwnd) {
        Some(rect) => {
            let area = floatpaste_core::platform::windows::picker_position::work_area_from_point(
                floatpaste_core::platform::windows::picker_position::ScreenPoint {
                    x: rect.left,
                    y: rect.top,
                },
            )?;
            let scale = window.window().scale_factor();
            let point = floatpaste_core::services::picker_position_service::center_in_work_area(
                area,
                (460.0 * scale) as i32,
                (250.0 * scale) as i32,
            );
            Some((point.x, point.y))
        }
        None => None,
    };
    // 有落位点时先停屏首秀（同 picker/tooltip 的停屏语义）：图标须等窗口实体
    // 就绪才能写入，先上屏会露出一帧系统缩放的糊图标。
    if target_position.is_some() {
        window.window().set_position(slint::PhysicalPosition::new(
            OFFSCREEN_POSITION,
            OFFSCREEN_POSITION,
        ));
    }
    let previous_foreground = ActiveAppResolver::current_foreground_hwnd();
    // 先收口来源输入与鼠标监控，保留原目标；对话框接管焦点。
    match source {
        PasteSource::Picker => crate::picker::hide_for_editor(app),
        PasteSource::Search => crate::search::hide_for_editor(app),
    }
    if let Err(error) = window.show() {
        restore(app, source, target);
        return Err(AppError::Message(format!("显示文件名窗口失败：{error}")));
    }
    reveal_with_icon(window.as_weak(), target_position);
    // 速贴不抢焦点，前台仍在目标应用；不强制获取前台时
    // 对话框会被压在目标窗口之下，键盘输入（Esc/Enter/文件名）全部
    // 落入目标应用。沿用编辑器窗口绕过前台锁的获取方式：show 刚返回
    // 时首帧与窗口句柄都未必就绪，立即抢前台会失败，延后到窗口就绪
    // 再从本次对话框取句柄执行；旧请求及用户已切往其他窗口时不再抢焦点。
    let weak = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(120), move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_focus(&window) {
            return;
        }
        let Some(hwnd) = crate::win32_ext::window_hwnd(&window) else {
            warn!("文件名窗口句柄缺失，无法获取前台");
            return;
        };
        let foreground = ActiveAppResolver::current_foreground_hwnd();
        if foreground.is_some()
            && foreground != Some(hwnd)
            && foreground != previous_foreground
            && foreground != target.target_window_hwnd
        {
            return;
        }
        if !ActiveAppResolver::force_foreground_window(hwnd) {
            warn!("文件名窗口获取前台失败");
        }
    });
    window.invoke_focus_name();
    DIALOG.with(|slot| {
        *slot.borrow_mut() = Some(Dialog {
            window,
            app: app.clone(),
            id: id.into(),
            option,
            source,
            target,
            generation,
        })
    });
    Ok(())
}

fn can_focus(window: &TextFileWindow) -> bool {
    DIALOG.with(|slot| {
        slot.borrow().as_ref().is_some_and(|dialog| {
            std::ptr::eq(dialog.window.window(), window.window()) && is_current(dialog)
        })
    })
}

fn is_current(dialog: &Dialog) -> bool {
    let session_id = match dialog.source {
        PasteSource::Picker => dialog.app.state.picker_session().session_id,
        PasteSource::Search => dialog.app.state.search_session().session_id,
    };
    dialog.generation == dialog.app.core().privacy_generation()
        && !dialog.app.core().session_locked()
        && !dialog.app.core().is_quitting()
        && dialog.target.session_id == session_id
        && !dialog.app.state.is_picker_active()
        && !dialog.app.state.is_search_active()
}

fn confirm() {
    // 不持有 RefCell 借用进入上屏：窗口收口与主题回调可能再次访问对话框。
    let Some(dialog) = DIALOG.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    if !is_current(&dialog) {
        let _ = dialog.window.hide();
        return;
    }
    let result = normalize_file_name(&dialog.window.get_file_name()).and_then(|name| {
        crate::paste_flow::paste_named_text_file(
            &dialog.app,
            &dialog.id,
            dialog.option.clone(),
            dialog.source,
            &name,
            dialog.generation,
        )
    });
    match result {
        Ok(()) | Err(AppError::OperationCancelled) => {
            let _ = dialog.window.hide();
        }
        Err(error) => {
            dialog.window.set_error_text(error.to_string().into());
            DIALOG.with(|slot| *slot.borrow_mut() = Some(dialog));
        }
    }
}

pub(crate) fn close(return_to_source: bool) {
    let Some(dialog) = DIALOG.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    let current = is_current(&dialog);
    let _ = dialog.window.hide();
    if return_to_source && current {
        restore(&dialog.app, dialog.source, dialog.target);
    }
}

/// 停屏位置，与 picker/tooltip 的停屏口径一致（Windows 窗口坐标下限）。
const OFFSCREEN_POSITION: i32 = -32000;
/// 等窗口实体就绪的轮询间隔与次数上限。句柄迟迟不来时也必须落位，
/// 不能让对话框永久留在屏外。
const REVEAL_RETRY_MS: u64 = 8;
const REVEAL_RETRY_LIMIT: u32 = 16;

/// 写入窗口图标后落位。图标按 DPI 从内嵌 ICO 精确取档：Slint `Window.icon`
/// 交给 winit 的是单一位图（实测 `WM_GETICON` 拿到 512×512），系统再缩到
/// 标题栏/任务栏档位会把细线条糊掉——编辑/设置窗走 `app_icon` 的同一套取档。
/// 窗口实体由 winit 在下一轮事件循环创建（`show` 返回时句柄仍不可得），
/// 因此停屏首秀、就绪即写图标并落位：用户看到的首帧已带清晰图标。
fn reveal_with_icon(window: slint::Weak<TextFileWindow>, target: Option<(i32, i32)>) {
    attempt_reveal(window, target, REVEAL_RETRY_LIMIT);
}

fn attempt_reveal(window: slint::Weak<TextFileWindow>, target: Option<(i32, i32)>, retries: u32) {
    let Some(window) = window.upgrade() else {
        return;
    };
    let hwnd = crate::win32_ext::window_hwnd(&window);
    if let Some(hwnd) = hwnd {
        crate::app_icon::apply_window_icon(hwnd);
    }
    if hwnd.is_some() || retries == 0 {
        if let Some((x, y)) = target {
            window
                .window()
                .set_position(slint::PhysicalPosition::new(x, y));
        }
        return;
    }
    let weak = window.as_weak();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(REVEAL_RETRY_MS),
        move || {
            attempt_reveal(weak, target, retries - 1);
        },
    );
}

/// 交互桌面前台落地契约（docs/testing.md L2 前台段约束）：命名窗口确认
/// 时持有前台，确认路径必须在收起命名窗之前把前台归还目标并等激活
/// 落地，注入才能到达目标——回归「粘贴为文件」偶现焦点恢复失败且未
/// 注入（速贴来源确认后曾只靠后台线程延迟盲还）。无可控前台（无人
/// 值守会话）时整段跳过并报告覆盖缺失，不伪造通过。
#[cfg(test)]
pub(crate) fn foreground_settlement_contract_tests(app: &App) {
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use std::thread;
    use std::time::Duration;

    use floatpaste_core::{
        domain::clip_item::PasteMode, platform::windows::clipboard_formats::ClipboardSnapshot,
        platform::windows::single_instance, services::history_service::HistoryService,
    };
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN;

    // 注入观测：记录目标窗口收到的 Ctrl/V keydown（wparam 为虚拟键码）
    static INJECTED_KEYDOWN: AtomicU32 = AtomicU32::new(0);
    const VK_CONTROL: usize = 0x11;
    const VK_V: usize = 0x56;
    const INJECTION_SUBCLASS_ID: usize = 0x4650_0201;

    unsafe extern "system" fn injection_counter_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        _ref_data: usize,
    ) -> LRESULT {
        if msg == WM_KEYDOWN {
            INJECTED_KEYDOWN.fetch_or(
                match wparam.0 {
                    VK_CONTROL => 0b01,
                    VK_V => 0b10,
                    _ => 0b00,
                },
                AtomicOrdering::SeqCst,
            );
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    fn pump(ms: u64) {
        slint::Timer::single_shot(Duration::from_millis(ms), || {
            slint::quit_event_loop().unwrap();
        });
        slint::run_event_loop_until_quit().unwrap();
    }

    // 本段以真实发布 CF_HDROP 与真实注入为前提，剪贴板属 L3 串行资源：
    // 先占住日常实例互斥量（已有日常实例时跳过，防止测试内容被采进
    // 个人库），再捕获快照供段尾恢复，避免覆盖用户剪贴板
    let guard = match single_instance::reserve_clipboard_test_session() {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            eprintln!("日常实例运行中，跳过命名上屏前台落地契约");
            return;
        }
        Err(error) => panic!("获取剪贴板测试会话失败：{error}"),
    };
    let mut snapshot = None;
    for _ in 0..3 {
        match ClipboardSnapshot::capture(true) {
            Ok(captured) => {
                snapshot = Some(captured);
                break;
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
    let Some(snapshot) = snapshot else {
        eprintln!("剪贴板快照不可得（被占用或不可恢复），跳过命名上屏前台落地契约");
        return;
    };
    INJECTED_KEYDOWN.store(0, AtomicOrdering::SeqCst);

    let Some(_) = ActiveAppResolver::current_foreground_hwnd() else {
        eprintln!("无可控交互前台，跳过命名上屏前台落地契约");
        return;
    };
    // 目标窗口只提供真实 HWND 与前台；首次 show 前停屏（前台段辅助窗约束）
    let target = crate::EditorWindow::new().unwrap();
    target
        .window()
        .set_position(slint::PhysicalPosition::new(-32000, -32000));
    target.show().unwrap();
    pump(100);
    let target_hwnd = crate::win32_ext::window_hwnd(&target).unwrap();
    if !ActiveAppResolver::force_foreground_window(target_hwnd) {
        eprintln!("辅助目标窗取得前台失败，跳过命名上屏前台落地契约");
        return;
    }
    assert_eq!(
        crate::win32_ext::physical_rect(target_hwnd).map(|r| (r.left, r.top)),
        Some((-32000, -32000)),
        "辅助目标窗取得前台后位置不得漂移"
    );
    assert!(unsafe {
        SetWindowSubclass(
            HWND(target_hwnd as *mut _),
            Some(injection_counter_proc),
            INJECTION_SUBCLASS_ID,
            0,
        )
        .as_bool()
    });

    let item = HistoryService::ingest_text(app.core(), "命名上屏前台落地回归 😀", None)
        .unwrap()
        .unwrap();
    let session_id = 41_001;
    app.state.set_picker_session(TargetSession {
        session_id,
        target_window_hwnd: Some(target_hwnd),
        target_focus_hwnd: None,
    });
    let option = PasteOption {
        mode: PasteMode::TextFile,
        paste_to_target: true,
        restore_clipboard_after_paste: false,
    };
    crate::paste_flow::paste_item(app, &item.id, option, PasteSource::Picker).unwrap();
    let window = DIALOG.with(|slot| slot.borrow().as_ref().unwrap().window.clone_strong());
    // 命名窗口延迟聚焦应已从目标窗口抢走前台（与真实使用一致的抢占）
    pump(300);
    let naming_hwnd = crate::win32_ext::window_hwnd(&window).unwrap();
    assert_eq!(
        ActiveAppResolver::current_foreground_hwnd(),
        Some(naming_hwnd),
        "命名窗口应持有前台"
    );

    window.set_file_name("前台落地.txt".into());
    window.invoke_confirmed();
    // 确认返回即已归还：同步归还 + 激活落地先于命名窗收起，是本回归的核心
    assert!(
        DIALOG.with(|slot| slot.borrow().is_none()),
        "确认后释放请求"
    );
    assert_eq!(
        ActiveAppResolver::current_foreground_hwnd(),
        Some(target_hwnd),
        "确认必须先归还目标前台再收起命名窗"
    );

    // 后台链在此后注入：目标窗口应收到完整 Ctrl+V（前台未落地时注入
    // 会落进别的窗口，计数保持 0）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline
        && INJECTED_KEYDOWN.load(AtomicOrdering::SeqCst) != 0b11
    {
        pump(50);
    }
    assert_eq!(
        INJECTED_KEYDOWN.load(AtomicOrdering::SeqCst),
        0b11,
        "目标窗口应收到 Ctrl 与 V 的 keydown"
    );
    app.state.set_picker_session(Default::default());

    // 剪贴板恢复是本段的收尾义务（对齐 L3 卫生纪律）：短重试后仍失败
    // 不得算作验证通过
    let mut restored = false;
    for _ in 0..3 {
        if snapshot.restore(None, None).is_ok() {
            restored = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(restored, "测试剪贴板快照恢复失败，剪贴板被本段覆盖");
    drop(guard);
}

fn restore(app: &App, source: PasteSource, target: TargetSession) {
    match source {
        PasteSource::Picker => crate::picker::restore_after_editor(app, target),
        PasteSource::Search => crate::search::restore_after_editor(app),
    }
}

/// 窗口图标的位图尺寸（WM_GETICON 的 HICON 经 GetIconInfo 取 color 位图），
/// `kind` 为 `ICON_SMALL`（标题栏）或 `ICON_BIG`（任务栏）。仅测试用：核对落窗
/// 图标是 ICO 里与 DPI 匹配的档位，而非被系统缩放的位图——后者是 Slint
/// `Window.icon` 的原图（实测 512×512），缩到 16/24 必糊。
#[cfg(test)]
fn icon_bitmap_size(hwnd: isize, kind: u32) -> Option<(i32, i32)> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::Graphics::Gdi::{DeleteObject, GetObjectW, BITMAP, HGDIOBJ};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetIconInfo, SendMessageW, HICON, ICONINFO, WM_GETICON,
    };

    unsafe {
        let icon = SendMessageW(
            HWND(hwnd as *mut _),
            WM_GETICON,
            Some(WPARAM(kind as usize)),
            Some(LPARAM(0)),
        );
        if icon.0 == 0 {
            return None;
        }
        let mut info = ICONINFO::default();
        GetIconInfo(HICON(icon.0 as *mut _), &mut info).ok()?;
        let mut bitmap = BITMAP::default();
        let copied = GetObjectW(
            HGDIOBJ(info.hbmColor.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut _ as *mut _),
        );
        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        (copied != 0).then_some((bitmap.bmWidth, bitmap.bmHeight))
    }
}

#[cfg(test)]
pub(crate) fn contract_tests(app: &App) {
    use floatpaste_core::{
        domain::clip_item::PasteMode, services::history_service::HistoryService,
    };
    use windows::Win32::UI::WindowsAndMessaging::{ICON_BIG, ICON_SMALL};
    let item = HistoryService::ingest_text(app.core(), "文件名窗口验收 😀\n    原文", None)
        .unwrap()
        .unwrap();
    let option = PasteOption {
        mode: PasteMode::TextFile,
        paste_to_target: true,
        restore_clipboard_after_paste: true,
    };
    for source in [PasteSource::Search, PasteSource::Picker] {
        crate::paste_flow::paste_item(app, &item.id, option.clone(), source).unwrap();
        let window = DIALOG.with(|slot| slot.borrow().as_ref().unwrap().window.clone_strong());
        assert!(can_focus(&window));
        // 图标由窗口实体就绪后的定时器写入，先让事件循环转一圈（winit 在这一轮
        // 建窗，之后才谈得上 WM_SETICON 与落位）。
        slint::Timer::single_shot(std::time::Duration::from_millis(80), || {
            slint::quit_event_loop().unwrap()
        });
        slint::run_event_loop_until_quit().unwrap();
        // 标题栏取小图标、任务栏取大图标绘制，两者的位图都必须是 ICO 里与当前
        // DPI 匹配的档位：Slint 位图只有一个尺寸，系统缩到 16/24 会把细线条糊掉
        // （回归「粘贴为文件」窗口图标模糊）。
        let hwnd = crate::win32_ext::window_hwnd(&window).expect("命名窗口句柄缺失");
        let (small, big) = unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetSystemMetrics, SM_CXICON, SM_CXSMICON,
            };
            (
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CXICON) * 3 / 4,
            )
        };
        assert_eq!(
            icon_bitmap_size(hwnd, ICON_SMALL),
            Some((small, small)),
            "命名窗口标题栏图标应按 DPI 从内嵌 ICO 精确取档"
        );
        assert_eq!(
            icon_bitmap_size(hwnd, ICON_BIG),
            Some((big, big)),
            "命名窗口任务栏图标应按 DPI 从内嵌 ICO 精确取档"
        );
        // 停屏首秀是为了不在首帧露出糊图标，落位不能因此丢失
        assert_ne!(
            crate::win32_ext::physical_rect(hwnd).map(|rect| rect.left),
            Some(OFFSCREEN_POSITION),
            "命名窗口必须落位，不能留在停屏位置"
        );
        close(false);
        // 配置次级动作为文本文件后，两个真实窗口入口仍先进入命名流程。
        let mut settings = app.state.current_settings();
        let old_defaults = settings.secondary_paste;
        settings.secondary_paste.plain_text = PasteMode::TextFile;
        app.core().update_settings(settings).unwrap();
        match source {
            PasteSource::Search => {
                app.state.set_search_items(vec![item.to_summary()]);
                crate::search::paste_index(app, 0, true);
            }
            PasteSource::Picker => {
                app.state.set_items(vec![item.to_summary()]);
                crate::picker::confirm(app, 0, true);
            }
        }
        assert!(
            DIALOG.with(|slot| slot.borrow().is_some()),
            "次级键必须打开命名窗口"
        );
        let mut settings = app.state.current_settings();
        settings.secondary_paste = old_defaults;
        app.core().update_settings(settings).unwrap();
        let window = DIALOG.with(|slot| slot.borrow().as_ref().unwrap().window.clone_strong());
        crate::paste_flow::paste_item(app, &item.id, option.clone(), source).unwrap();
        assert!(!can_focus(&window), "旧聚焦回调不得接管替换后的窗口");
        let window = DIALOG.with(|slot| slot.borrow().as_ref().unwrap().window.clone_strong());
        assert_eq!(window.get_file_name(), "文本.txt");
        window.set_file_name("CON.txt".into());
        window.invoke_confirmed();
        assert!(
            !window.get_error_text().is_empty(),
            "无效文件名应保留窗口并显示错误"
        );
        assert!(DIALOG.with(|slot| slot.borrow().is_some()));
        if std::env::var_os("FLOATPASTE_UI_SNAPSHOTS").is_some() {
            for (name, resolved) in [
                ("light", theme::ResolvedTheme::Light),
                ("dark", theme::ResolvedTheme::Dark),
            ] {
                apply_theme(&theme::derive_tokens("default", "", resolved));
                let _ = window.window().take_snapshot().unwrap();
                slint::Timer::single_shot(std::time::Duration::from_millis(180), || {
                    slint::quit_event_loop().unwrap()
                });
                slint::run_event_loop_until_quit().unwrap();
                let pixels = window.window().take_snapshot().unwrap();
                let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.artifacts/snapshot");
                std::fs::create_dir_all(&dir).unwrap();
                image::save_buffer(
                    dir.join(format!(
                        "text-file-{name}-{}.png",
                        (window.window().scale_factor() * 100.0).round() as u32
                    )),
                    pixels.as_bytes(),
                    pixels.width(),
                    pixels.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Escape.into(),
            });
        assert!(
            DIALOG.with(|slot| slot.borrow().is_none()),
            "Esc 必须释放请求及窗口"
        );
        crate::paste_flow::paste_item(app, &item.id, option.clone(), source).unwrap();
        let window = DIALOG.with(|slot| slot.borrow().as_ref().unwrap().window.clone_strong());
        app.core().set_session_locked(true).unwrap();
        app.core().set_session_locked(false).unwrap();
        assert!(!can_focus(&window), "隐私代际失效后不得抢前台");
        window.invoke_confirmed();
        assert!(
            DIALOG.with(|slot| slot.borrow().is_none()),
            "旧代际不得发布或恢复来源"
        );
    }
}
