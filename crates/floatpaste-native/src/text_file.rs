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
    if let Some(rect) = crate::win32_ext::physical_rect(source_hwnd) {
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
            (290.0 * scale) as i32,
        );
        window
            .window()
            .set_position(slint::PhysicalPosition::new(point.x, point.y));
    }
    // 先收口来源输入与鼠标监控，保留原目标；对话框接管焦点。
    match source {
        PasteSource::Picker => crate::picker::hide_for_editor(app),
        PasteSource::Search => crate::search::hide_for_editor(app),
    }
    if let Err(error) = window.show() {
        restore(app, source, target);
        return Err(AppError::Message(format!("显示文件名窗口失败：{error}")));
    }
    // 速贴/搜索均为不抢焦点窗口，前台仍在目标应用；不强制获取前台时
    // 对话框会被压在目标窗口之下，键盘输入（Esc/Enter/文件名）全部
    // 落入目标应用。沿用编辑器窗口绕过前台锁的获取方式：show 刚返回
    // 时首帧与窗口句柄都未必就绪，立即抢前台会失败，延后到窗口就绪
    // 再从当前对话框取句柄执行。
    slint::Timer::single_shot(std::time::Duration::from_millis(120), || {
        let hwnd = DIALOG.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|dialog| crate::win32_ext::window_hwnd(&dialog.window))
        });
        let Some(hwnd) = hwnd else {
            // 对话框已被取消或替换，无需获取前台
            return;
        };
        let Some(hwnd) = hwnd else {
            warn!("文件名窗口句柄缺失，无法获取前台");
            return;
        };
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

fn restore(app: &App, source: PasteSource, target: TargetSession) {
    match source {
        PasteSource::Picker => crate::picker::restore_after_editor(app, target),
        PasteSource::Search => crate::search::restore_after_editor(app),
    }
}

#[cfg(test)]
pub(crate) fn contract_tests(app: &App) {
    use floatpaste_core::{
        domain::clip_item::PasteMode, services::history_service::HistoryService,
    };
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
        window.invoke_confirmed();
        assert!(
            DIALOG.with(|slot| slot.borrow().is_none()),
            "旧代际不得发布或恢复来源"
        );
    }
}
