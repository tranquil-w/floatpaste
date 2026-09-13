//! 编辑窗口：条目文本编辑、标签管理、删除与返回流程。
//!
//! 对齐旧版 EditorShell.tsx / tagEditor.tsx / keyboard.ts 与
//! WindowCoordinator 的 open_editor_from_* / hide_editor_and_restore_source：
//! - 速贴入口：收起面板（保留目标窗口会话），关闭后无激活恢复会话
//! - 搜索入口：隐藏窗口但不清理列表状态，关闭后原样恢复选中与滚动
//! - 文本保存走 ClipService::update_text（归一化 + 落库 + FTS 同步）
//! - 标签全量替换走 TagService::set_item_tags（NOCASE 去重与规范对齐）

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::time::Duration;

use slint::{ComponentHandle, Model, VecModel};
use tracing::{info, warn};

use floatpaste_core::domain::clip_item::ClipItemDetail;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::services::clip_service::ClipService;
use floatpaste_core::services::tag_service::TagService;
use floatpaste_core::services::time_format::format_relative_time_or_unused;

use crate::app_state::{EditorSession, TargetSession};
use crate::picker::{self, App};
use crate::search;
use crate::thumbnails;
use crate::win32_ext;
use crate::{EditorTagSuggestion, EditorWindow};

const NOTICE_TIMEOUT_MS: u64 = 3000;
const DELETE_ARM_TIMEOUT_MS: u64 = 3000;
const MAX_TAG_NAME_LEN: usize = 32;
const MAX_TAGS_PER_ITEM: usize = 20;
const MAX_SUGGESTIONS: usize = 8;

thread_local! {
    /// 已保存文本（dirty 判定的基准，与 slint 侧 saved-text 属性同源）
    static SAVED_TEXT: RefCell<String> = const { RefCell::new(String::new()) };
    static NOTICE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static DELETE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static DELETE_ARMED: Cell<bool> = const { Cell::new(false) };
    /// 全部标签缓存（name, item_count）：建议浮层的数据源
    static ALL_TAGS: RefCell<Vec<(String, u32)>> = const { RefCell::new(Vec::new()) };
}

// ── 打开入口 ──────────────────────────────────────────────

/// 速贴面板 Ctrl+Enter：收起面板（不还原前台，编辑器接管），打开编辑器
pub fn open_from_picker(app: &App, item_id: String) {
    let target = app.state.picker_session();
    picker::hide_for_editor(app);
    show_editor(
        app,
        EditorSession {
            item_id,
            return_to: 0,
            target_window_hwnd: target.target_window_hwnd,
            target_focus_hwnd: target.target_focus_hwnd,
        },
    );
}

/// 搜索窗口 Ctrl+Enter / 铅笔按钮：隐藏窗口（保留列表状态），打开编辑器
pub fn open_from_search(app: &App, item_id: String) {
    let target = app.state.search_session();
    search::hide_for_editor(app);
    show_editor(
        app,
        EditorSession {
            item_id,
            return_to: 1,
            target_window_hwnd: target.target_window_hwnd,
            target_focus_hwnd: None,
        },
    );
}

fn show_editor(app: &App, session: EditorSession) {
    app.state.set_editor_session(Some(session.clone()));
    let Some(win) = app.editor.upgrade() else {
        app.state.set_editor_session(None);
        warn!("编辑窗口尚未就绪，无法打开编辑器");
        return;
    };

    reset_delete_arm(&win);
    // 显式定尺寸：窗口根布局的首选高被 stretch 子元素拉成极小，会被钳到
    // min（400×300），不能依赖 preferred（对齐旧版 inner_size(800,600)）
    win.window().set_size(slint::LogicalSize::new(800.0, 600.0));
    let _ = win.window().show();
    // 首显可能走 SW_SHOWNOACTIVATE（winit 首窗语义），而从速贴打开时
    // 前台在回贴目标应用上——必须主动把前台请过来，否则 Esc/Ctrl+S
    // 落空（对齐旧版 window.set_focus()；不能复用带 TOPMOST 的
    // restore_window_and_focus——编辑器是普通窗口，不该常驻置顶）
    if let Some(hwnd) = win32_ext::window_hwnd(&win) {
        if !ActiveAppResolver::restore_foreground_window(hwnd) {
            warn!("编辑窗口获取前台失败");
        }
    }
    load_session(app, &win, &session.item_id);
    info!("打开 Editor，item={}", session.item_id);
}

/// 载入条目详情并填充界面（同步读库：单条查询延迟可忽略，省去加载态）
fn load_session(app: &App, win: &EditorWindow, item_id: &str) {
    win.set_error_text("".into());
    win.set_notice_text("".into());
    win.set_image_loading(false);
    win.set_image_failed(false);
    win.set_has_image(false);

    let detail = app.core().repository.get_item_detail(item_id);
    let detail = match detail {
        Ok(detail) => detail,
        Err(error) => {
            warn!("读取条目详情失败: {error}");
            win.set_has_session(true);
            win.set_loading(false);
            win.set_item_missing(true);
            return;
        }
    };

    win.set_has_session(true);
    win.set_loading(false);
    win.set_item_missing(false);
    win.set_is_text(detail.r#type == "text");
    win.set_meta_source(
        detail
            .source_app
            .clone()
            .unwrap_or_else(|| "未知来源".into())
            .into(),
    );
    win.set_meta_time(format_relative_time_or_unused(Some(&detail.created_at)).into());
    win.set_meta_extra(build_meta_extra(&detail).into());

    SAVED_TEXT.with(|slot| *slot.borrow_mut() = String::new());
    if detail.r#type == "text" {
        let full = detail.full_text.clone();
        win.set_char_count(full.chars().count() as i32);
        win.set_draft_text(full.clone().into());
        win.set_saved_text(full.clone().into());
        SAVED_TEXT.with(|slot| *slot.borrow_mut() = full);
    } else {
        // 非文本条目：预览区只读，无草稿
        win.set_char_count(0);
        win.set_draft_text("".into());
        win.set_saved_text("".into());
    }

    // 图片大图：后台解码原始像素，事件循环侧构造 slint::Image
    if detail.r#type == "image" {
        load_image_preview(app, win, &detail);
    }
    let paths: Vec<slint::SharedString> =
        detail.file_paths.iter().map(|p| p.clone().into()).collect();
    win.set_file_paths(slint::ModelRc::new(std::rc::Rc::new(VecModel::from(paths))));

    set_tags_model(win, &detail.tags);
    refresh_all_tags(app);
    rebuild_suggestions_for(app, "");

    if detail.r#type == "text" {
        win.invoke_focus_input();
    }
}

fn load_image_preview(app: &App, win: &EditorWindow, detail: &ClipItemDetail) {
    let Some(path) = detail.image_path.clone() else {
        win.set_image_failed(true);
        return;
    };
    win.set_image_loading(true);
    let core = app.core().clone();
    let app_cb = app.clone();
    std::thread::spawn(move || {
        let raw = thumbnails::load_full_image_raw(&core, &path);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(win) = app_cb.editor.upgrade() else {
                return;
            };
            win.set_image_loading(false);
            match raw.map(thumbnails::image_from_rgba) {
                Some(image) => {
                    win.set_image_preview(image);
                    win.set_has_image(true);
                }
                None => win.set_image_failed(true),
            }
        });
    });
}

/// 顶部元信息附加段：图片附尺寸，文件附个数与总大小
fn build_meta_extra(detail: &ClipItemDetail) -> String {
    let mut parts: Vec<String> = Vec::new();
    if detail.r#type == "image" {
        if let (Some(width), Some(height)) = (detail.image_width, detail.image_height) {
            parts.push(format!("{width} × {height}"));
        }
    }
    if detail.r#type == "file" && detail.file_count > 0 {
        parts.push(format!("{} 个文件", detail.file_count));
    }
    let size_bytes = if detail.r#type == "file" {
        detail.total_size.or(detail.file_size)
    } else {
        detail.file_size
    };
    if let Some(label) = crate::search::format_file_size(size_bytes) {
        parts.push(label);
    }
    parts.join(" · ")
}

// ── 保存 / 关闭 / 删除 ────────────────────────────────────

pub fn text_edited(app: &App, text: &str) {
    if let Some(win) = app.editor.upgrade() {
        win.set_char_count(text.chars().count() as i32);
    }
}

pub fn save(app: &App) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    if !win.get_is_text() {
        return;
    }
    let Some(session) = app.state.editor_session() else {
        return;
    };
    let draft = win.get_draft_text().to_string();
    match ClipService::update_text(&app.core(), &session.item_id, &draft) {
        Ok(_) => {
            // 对齐旧版 markSaved(draftText)：草稿即视为已保存基线
            SAVED_TEXT.with(|slot| *slot.borrow_mut() = draft.clone());
            show_notice(app, "已保存当前修改");
            win.set_error_text("".into());
            refresh_lists(app);
        }
        Err(error) => {
            warn!("保存条目失败: {error}");
            win.set_notice_text("".into());
            win.set_error_text(format!("保存失败：{error}").into());
        }
    }
}

/// 确认框"保存并关闭"：保存成功才关闭，失败留在编辑器（对齐旧版）
pub fn save_then_close(app: &App) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    if !win.get_is_text() {
        close_editor(app);
        return;
    }
    save(app);
    // 保存失败时 error 条已展示且 saved 仍是旧值（dirty 保持），不关闭
    if !win.get_is_dirty() {
        close_editor(app);
    }
}

pub fn request_close(app: &App) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    if win.get_is_dirty() {
        win.set_close_confirm_open(true);
        return;
    }
    close_editor(app);
}

pub fn cancel_confirm(app: &App) {
    if let Some(win) = app.editor.upgrade() {
        win.set_close_confirm_open(false);
    }
}

pub fn close_editor(app: &App) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    win.set_close_confirm_open(false);
    win.set_error_text("".into());
    let _ = win.window().hide();
    hide_and_restore_source(app);
}

/// 标题栏 X 关闭：脏 → 弹确认框拦截；干净 → 放行隐藏（返回流程同上）
pub fn window_close_requested(app: &App) -> bool {
    let Some(win) = app.editor.upgrade() else {
        return true;
    };
    if win.get_is_dirty() {
        win.set_close_confirm_open(true);
        return false;
    }
    close_editor(app);
    true
}

pub fn delete_requested(app: &App) {
    let Some(session) = app.state.editor_session() else {
        return;
    };
    let armed = DELETE_ARMED.with(|slot| slot.get());
    if !armed {
        // 首次点击进入待确认态，3 秒未确认自动解除（对齐 useArmedConfirm）
        DELETE_ARMED.with(|slot| slot.set(true));
        let token = DELETE_TOKEN.with(|slot| {
            slot.set(slot.get() + 1);
            slot.get()
        });
        if let Some(win) = app.editor.upgrade() {
            win.set_delete_armed(true);
        }
        let app_cb = app.clone();
        slint::Timer::single_shot(Duration::from_millis(DELETE_ARM_TIMEOUT_MS), move || {
            if DELETE_TOKEN.with(|slot| slot.get()) != token {
                return;
            }
            DELETE_ARMED.with(|slot| slot.set(false));
            if let Some(win) = app_cb.editor.upgrade() {
                win.set_delete_armed(false);
            }
        });
        return;
    }

    DELETE_ARMED.with(|slot| slot.set(false));
    DELETE_TOKEN.with(|slot| slot.set(slot.get() + 1));
    if let Some(win) = app.editor.upgrade() {
        win.set_delete_armed(false);
    }
    match ClipService::delete(&app.core(), &session.item_id) {
        Ok(()) => {
            refresh_lists(app);
            close_editor(app);
        }
        Err(error) => {
            warn!("删除条目失败: {error}");
            if let Some(win) = app.editor.upgrade() {
                win.set_notice_text("".into());
                win.set_error_text(format!("删除失败：{error}").into());
            }
        }
    }
}

fn reset_delete_arm(win: &EditorWindow) {
    DELETE_ARMED.with(|slot| slot.set(false));
    DELETE_TOKEN.with(|slot| slot.set(slot.get() + 1));
    win.set_delete_armed(false);
}

fn hide_and_restore_source(app: &App) {
    let Some(session) = app.state.editor_session() else {
        return;
    };
    app.state.set_editor_session(None);
    match session.return_to {
        0 => picker::restore_after_editor(
            app,
            TargetSession {
                target_window_hwnd: session.target_window_hwnd,
                target_focus_hwnd: session.target_focus_hwnd,
            },
        ),
        _ => search::restore_after_editor(app),
    }
}

fn refresh_lists(app: &App) {
    // 两窗此时都可能处于失活态：走无条件的编辑后刷新
    picker::refresh_after_editor(app);
    search::refresh_after_editor(app);
}

fn show_notice(app: &App, text: &str) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    win.set_notice_text(text.into());
    let token = NOTICE_TOKEN.with(|slot| {
        slot.set(slot.get() + 1);
        slot.get()
    });
    let app_cb = app.clone();
    slint::Timer::single_shot(Duration::from_millis(NOTICE_TIMEOUT_MS), move || {
        if NOTICE_TOKEN.with(|slot| slot.get()) != token {
            return;
        }
        if let Some(win) = app_cb.editor.upgrade() {
            win.set_notice_text("".into());
        }
    });
}

// ── 标签编辑 ──────────────────────────────────────────────

pub fn tag_input_changed(app: &App, input: &str) {
    rebuild_suggestions_for(app, input);
}

/// 回车：浮层开着采纳高亮建议，否则按输入文本添加
pub fn tag_commit_add(app: &App) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    if win.get_tag_open() {
        let index = win.get_tag_active().max(0) as usize;
        tag_adopt(app, index);
        return;
    }
    let input = win.get_tag_input_text().to_string();
    add_tag(app, &input);
}

pub fn tag_adopt(app: &App, index: usize) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let suggestions = win.get_tag_suggestions();
    let Some(suggestion) = suggestions.row_data(index) else {
        return;
    };
    if suggestion.kind == 2 {
        // 已添加：仅清空输入，不重复提交（对齐 adoptSuggestion exists）
        win.set_tag_input_text("".into());
        return;
    }
    add_tag(app, &suggestion.name);
}

pub fn tag_complete(app: &App) {
    // Tab 仅补全文本（match/create），让用户在此基础上继续编辑
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let suggestions = win.get_tag_suggestions();
    let index = win.get_tag_active().max(0) as usize;
    let Some(suggestion) = suggestions.row_data(index) else {
        return;
    };
    if suggestion.kind != 2 {
        win.set_tag_input_text(suggestion.name.clone());
        win.set_tag_highlight(0);
    }
}

pub fn tag_remove(app: &App, index: usize) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let current: Vec<String> = win
        .get_tags()
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, name)| name.to_string())
        .collect();
    commit_tags(app, current);
}

pub fn tag_remove_last(app: &App) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let current: Vec<String> = win.get_tags().iter().map(|s| s.to_string()).collect();
    let Some(last) = current.last().cloned() else {
        return;
    };
    commit_tags(
        app,
        current
            .iter()
            .filter(|name| **name != last)
            .cloned()
            .collect(),
    );
}

pub fn tag_escape(app: &App) {
    // 本地消费：清空输入并保持焦点（对齐 tagEditor 的 Esc 分支）
    if let Some(win) = app.editor.upgrade() {
        win.set_tag_input_text("".into());
        win.invoke_focus_input();
    }
}

fn add_tag(app: &App, raw_name: &str) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let name = normalize_tag_input(raw_name);
    if name.is_empty() {
        win.set_tag_input_text("".into());
        return;
    }
    if name.chars().count() > MAX_TAG_NAME_LEN {
        win.set_notice_text("".into());
        win.set_error_text(format!("标签名不能超过 {MAX_TAG_NAME_LEN} 个字符").into());
        return;
    }
    let current: Vec<String> = win.get_tags().iter().map(|s| s.to_string()).collect();
    if current
        .iter()
        .any(|existing| existing.to_lowercase() == name.to_lowercase())
    {
        win.set_tag_input_text("".into());
        return;
    }
    if current.len() >= MAX_TAGS_PER_ITEM {
        win.set_notice_text("".into());
        win.set_error_text(format!("单条目标签数不能超过 {MAX_TAGS_PER_ITEM} 个").into());
        return;
    }
    let mut next = current;
    next.push(name);
    commit_tags(app, next);
}

fn commit_tags(app: &App, next: Vec<String>) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let Some(session) = app.state.editor_session() else {
        return;
    };
    match TagService::set_item_tags(&app.core(), &session.item_id, &next) {
        Ok(detail) => {
            // 失败回滚等效于"成功才动模型"：这里只提交服务端规范名
            set_tags_model(&win, &detail.tags);
            win.set_tag_input_text("".into());
            refresh_all_tags(app);
            let input = win.get_tag_input_text().to_string();
            rebuild_suggestions_for(app, &input);
            refresh_lists(app);
        }
        Err(error) => {
            warn!("标签保存失败: {error}");
            win.set_notice_text("".into());
            win.set_error_text(format!("标签保存失败：{error}").into());
        }
    }
}

fn set_tags_model(win: &EditorWindow, tags: &[String]) {
    let model = VecModel::from(
        tags.iter()
            .map(|name| slint::SharedString::from(name.as_str()))
            .collect::<Vec<_>>(),
    );
    win.set_tags(slint::ModelRc::new(std::rc::Rc::new(model)));
}

fn refresh_all_tags(app: &App) {
    match app.core().repository.list_tags() {
        Ok(tags) => {
            ALL_TAGS.with(|slot| {
                *slot.borrow_mut() = tags
                    .into_iter()
                    .map(|tag| (tag.name, tag.item_count))
                    .collect();
            });
        }
        Err(error) => warn!("读取标签列表失败: {error}"),
    }
}

fn rebuild_suggestions_for(app: &App, input: &str) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let current: Vec<String> = win.get_tags().iter().map(|s| s.to_string()).collect();
    let suggestions = ALL_TAGS.with(|slot| build_suggestions(&slot.borrow(), &current, input));
    win.set_tag_highlight(0);
    win.set_tag_suggestions(slint::ModelRc::new(std::rc::Rc::new(VecModel::from(
        suggestions,
    ))));
}

fn normalize_tag_input(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 建议算法（对齐 buildTagSuggestions）：空输入给常用标签；
/// 非空输入大小写不敏感包含匹配，末尾按需追加"已添加/创建"状态项
fn build_suggestions(
    all: &[(String, u32)],
    current: &[String],
    keyword: &str,
) -> Vec<EditorTagSuggestion> {
    let keyword = normalize_tag_input(keyword);
    let keyword_lower = keyword.to_lowercase();
    let added: HashSet<String> = current.iter().map(|name| name.to_lowercase()).collect();
    let pool: Vec<&(String, u32)> = all
        .iter()
        .filter(|(name, _)| !added.contains(&name.to_lowercase()))
        .collect();
    let matched: Vec<&(String, u32)> = if keyword.is_empty() {
        pool.clone()
    } else {
        pool.iter()
            .filter(|(name, _)| name.to_lowercase().contains(&keyword_lower))
            .copied()
            .collect()
    };

    let mut suggestions: Vec<EditorTagSuggestion> = matched
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(name, count)| EditorTagSuggestion {
            kind: 0,
            name: name.clone().into(),
            count_text: format!("{count} 条").into(),
        })
        .collect();

    if keyword.is_empty() {
        return suggestions;
    }
    if added.contains(&keyword_lower) {
        // 展示已添加标签的规范写法，让用户看到与输入的差异
        let existing = current
            .iter()
            .find(|name| name.to_lowercase() == keyword_lower)
            .cloned()
            .unwrap_or(keyword);
        suggestions.push(EditorTagSuggestion {
            kind: 2,
            name: existing.into(),
            count_text: "".into(),
        });
        return suggestions;
    }
    // 全库已有仅大小写不同的标签时不提示"创建"：提交会被对齐到既有写法
    let canonical_exists = pool
        .iter()
        .any(|(name, _)| name.to_lowercase() == keyword_lower);
    if !canonical_exists {
        suggestions.push(EditorTagSuggestion {
            kind: 1,
            name: keyword.into(),
            count_text: "".into(),
        });
    }
    suggestions
}
