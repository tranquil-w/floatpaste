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
use std::sync::atomic::Ordering;
use std::time::Duration;

use slint::{ComponentHandle, Model, VecModel};
use tracing::{info, warn};

use floatpaste_core::domain::clip_item::ClipItemDetail;
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::picker_position::{ScreenPoint, work_area_from_point};
use floatpaste_core::services::clip_display::format_file_size;
use floatpaste_core::services::clip_service::ClipService;
use floatpaste_core::services::tag_service::TagService;
use floatpaste_core::services::time_format::format_relative_time_or_unused;

use crate::app_state::{EditorSession, TargetSession};
use crate::picker::{self, App};
use crate::search;
use crate::thumbnails;
use crate::tooltip::HoverHost;
use crate::win32_ext;
use crate::{app_icon, EditorTagSuggestion, EditorWindow};

const NOTICE_TIMEOUT_MS: u64 = 3000;
const DELETE_ARM_TIMEOUT_MS: u64 = 3000;
const MAX_TAG_NAME_LEN: usize = 32;
const MAX_TAGS_PER_ITEM: usize = 20;
const MAX_SUGGESTIONS: usize = 8;
/// 编辑窗口逻辑尺寸（对齐旧版 inner_size(800,600)）
const EDITOR_LOGICAL_WIDTH: f32 = 800.0;
const EDITOR_LOGICAL_HEIGHT: f32 = 600.0;

thread_local! {
    static NOTICE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static DELETE_TOKEN: Cell<u64> = const { Cell::new(0) };
    static DELETE_ARMED: Cell<bool> = const { Cell::new(false) };
    /// 全部标签缓存（name, item_count）：建议浮层的数据源
    static ALL_TAGS: RefCell<Vec<(String, u32)>> = const { RefCell::new(Vec::new()) };
    /// 编辑窗口本会话位置记忆（物理像素）。SLINT_DESTROY_WINDOW_ON_HIDE
    /// 在 hide 时销毁 winit 窗口、位置随之丢失，须在 hide 前读取暂存；
    /// None = 本会话尚未打开过，首开落在宿主窗口所在屏的中心
    static LAST_POSITION: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

/// 宿主锚点：宿主窗口在停屏/隐藏**前**捕获的物理中心与 DPI。
/// 速贴的「隐藏」是平移到 (-32000,-32000) 停屏，隐藏后取 rect 只会得到
/// 屏外坐标——首开居中必须用停屏前的锚点
struct HostAnchor {
    center: (i32, i32),
    dpi: f32,
}

fn host_anchor(hwnd: isize) -> Option<HostAnchor> {
    if hwnd <= 0 {
        return None;
    }
    let rect = win32_ext::physical_rect(hwnd)?;
    let dpi = win32_ext::window_dpi(hwnd).max(96) as f32 / 96.0;
    Some(HostAnchor {
        center: ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2),
        dpi,
    })
}

/// 位置记忆有效性：点必须落在某显示器工作区内（拦截停屏坐标等屏外值，
/// 避免一次异常位置让编辑器此后每次都开在屏外）
fn position_is_visible((x, y): (i32, i32)) -> bool {
    work_area_from_point(ScreenPoint { x, y })
        .map(|area| x >= area.left && x < area.right && y >= area.top && y < area.bottom)
        .unwrap_or(false)
}

// ── 打开入口 ──────────────────────────────────────────────

/// 速贴面板 Ctrl+Enter：收起面板（不还原前台，编辑器接管），打开编辑器
pub fn open_from_picker(app: &App, item_id: String) {
    let target = app.state.picker_session();
    // 锚点须在停屏前捕获：hide_for_editor 把速贴平移到屏外
    let anchor = host_anchor(app.state.picker_hwnd.load(Ordering::SeqCst));
    picker::hide_for_editor(app);
    show_editor(
        app,
        EditorSession {
            item_id,
            return_to: 0,
            target_window_hwnd: target.target_window_hwnd,
            target_focus_hwnd: target.target_focus_hwnd,
        },
        anchor,
    );
}

/// 搜索窗口 Ctrl+Enter / 铅笔按钮：隐藏窗口（保留列表状态），打开编辑器
pub fn open_from_search(app: &App, item_id: String) {
    let target = app.state.search_session();
    let anchor = host_anchor(app.state.search_hwnd.load(Ordering::SeqCst));
    search::hide_for_editor(app);
    show_editor(
        app,
        EditorSession {
            item_id,
            return_to: 1,
            target_window_hwnd: target.target_window_hwnd,
            target_focus_hwnd: None,
        },
        anchor,
    );
}

fn show_editor(app: &App, session: EditorSession, anchor: Option<HostAnchor>) {
    app.state.set_editor_session(Some(session.clone()));
    let Some(win) = app.editor.upgrade() else {
        app.state.set_editor_session(None);
        warn!("编辑窗口尚未就绪，无法打开编辑器");
        return;
    };

    reset_delete_arm(&win);
    // 显式定尺寸：窗口根布局的首选高被 stretch 子元素拉成极小，会被钳到
    // min（400×300），不能依赖 preferred（对齐旧版 inner_size(800,600)）
    // 整帧重绘：hide→show 后 Slint 只重绘「与上次渲染不同的区域」，静态
    // 的窗口背景/头部/底栏保持表面销毁时的白底（尺寸变化与同步翻转均
    // 实测无效——同步翻转在渲染前自我抵消）。在 show 之前翻转
    // force-repaint 并保留到下一次打开再翻回：覆盖层颜色变化固化进新帧，
    // 重开时全窗每个区域都与上次渲染不同 → 脏区覆盖全窗（0.4% 白视觉
    // 不可感知）
    win.set_force_repaint(!win.get_force_repaint());
    win.window()
        .set_size(slint::LogicalSize::new(EDITOR_LOGICAL_WIDTH, EDITOR_LOGICAL_HEIGHT));
    // 位置：本会话有用过且位置可见则原样还原（用户拖过的位置不丢）；
    // 否则以停屏前捕获的宿主中心弹出，编辑器不出现在别的屏
    match LAST_POSITION.get().filter(|pos| position_is_visible(*pos)) {
        Some((x, y)) => win.window().set_position(slint::PhysicalPosition::new(x, y)),
        None => {
            if let Some(anchor) = anchor {
                let width = (EDITOR_LOGICAL_WIDTH * anchor.dpi) as i32;
                let height = (EDITOR_LOGICAL_HEIGHT * anchor.dpi) as i32;
                win.window().set_position(slint::PhysicalPosition::new(
                    anchor.center.0 - width / 2,
                    anchor.center.1 - height / 2,
                ));
            }
        }
    }
    let _ = win.window().show();
    load_session(app, &win, &session.item_id);
    info!("打开 Editor，item={}", session.item_id);
    // SLINT_DESTROY_WINDOW_ON_HIDE 下每次隐藏都销毁 winit 窗口，再次
    // 打开是重建：建窗在下一拍事件循环落地，句柄相关收尾（边框/暖屏/
    // 前置/焦点）延后执行，否则编辑器不在前台
    let app_cb = app.clone();
    let item_id_cb = session.item_id.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        if let Some(win) = app_cb.editor.upgrade() {
            if !app_cb
                .state
                .editor_session()
                .is_some_and(|session| session.item_id == item_id_cb)
            {
                return;
            }
            if let Some(editor_hwnd) = win32_ext::window_hwnd(&win) {
                app_icon::apply_window_icon(editor_hwnd);
                win32_ext::remove_dwm_border(editor_hwnd);
                // 全应用统一 Acrylic（Mica 视觉过弱用户实测否决）；
                // hide 销毁重建型窗口每次打开重挂
                {
                    let settings = app_cb.state.current_settings();
                    let resolved = floatpaste_core::theme::resolve_theme(
                        settings.theme_mode.clone(),
                        floatpaste_core::theme::system_prefers_dark(),
                    );
                    let active = win32_ext::apply_window_backdrop(
                        editor_hwnd,
                        true,
                        resolved == floatpaste_core::theme::ResolvedTheme::Dark,
                    );
                    win.set_material_active(active);
                }
                win32_ext::warm_surface(editor_hwnd);
                // 前台获取放在全部尺寸/显隐操作之后：从速贴打开时本进程
                // 不是前台（前台在目标应用上），裸 SetForegroundWindow 会
                // 被前台锁拒绝、编辑器被目标窗口遮挡——force_foreground_window
                // 经 AttachThreadInput + BringWindowToTop 绕过（对齐旧版
                // window.set_focus() 语义），仍失败时以 TOPMOST 提升→回落
                // 保底可见，且不常驻置顶（不能复用带 TOPMOST 的
                // restore_window_and_focus）
                if !ActiveAppResolver::force_foreground_window(editor_hwnd) {
                    warn!("编辑窗口获取前台失败");
                }
                win.invoke_focus_root_scope();
            }
        }
    });
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
            win.set_item_missing(true);
            focus_loaded(win);
            return;
        }
    };

    win.set_has_session(true);
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

    if detail.r#type == "text" {
        let full = detail.full_text.clone();
        win.set_char_count(full.chars().count() as i32);
        // set-selection-offsets 要 UTF-8 字节偏移（Slint 内置语义），
        // Slint 侧拿不到字节长度（只有 character-count），由这里供给
        win.set_draft_byte_len(full.len() as i32);
        win.set_draft_text(full.clone().into());
        win.set_saved_text(full.into());
    } else {
        // 非文本条目：预览区只读，无草稿
        win.set_char_count(0);
        win.set_draft_text("".into());
        win.set_saved_text("".into());
    }

    // 图片大图：后台解码原始像素，事件循环侧构造 slint::Image
    if detail.r#type == "image" {
        win.set_image_path(detail.image_path.clone().unwrap_or_default().into());
        load_image_preview(app, win, &detail);
    }
    let paths: Vec<slint::SharedString> =
        detail.file_paths.iter().map(|p| p.clone().into()).collect();
    win.set_file_paths(slint::ModelRc::new(std::rc::Rc::new(VecModel::from(paths))));

    set_tags_model(win, &detail.tags);
    refresh_all_tags(app);
    rebuild_suggestions_for(app, "");

    focus_loaded(win);
}

/// 焦点收尾（数据全部就绪后调用）：序号自增驱动文本编辑区 changed 重新
/// 聚焦并把光标置尾——自增必须在 draft/char-count 写入之后，changed 里
/// 读到的才是本条目值；同窗口再次打开时编辑区 init 不再执行，重开路径
/// 全靠这条链路。非文本/缺失条目没有输入框，焦点先落在窗口级 FocusScope
/// 接收 Esc/Ctrl+S，文本条目的输入框随后由编辑区 init/changed 抢走
fn focus_loaded(win: &EditorWindow) {
    win.set_session_seq(win.get_session_seq() + 1);
    win.invoke_focus_root_scope();
}

fn load_image_preview(app: &App, win: &EditorWindow, detail: &ClipItemDetail) {
    let Some(path) = detail.image_path.clone() else {
        win.set_image_failed(true);
        return;
    };
    win.set_image_loading(true);
    let core = app.core().clone();
    let app_cb = app.clone();
    let item_id = detail.id.clone();
    std::thread::spawn(move || {
        let raw = thumbnails::load_full_image_raw(&core, &path);
        let _ = slint::invoke_from_event_loop(move || {
            // 会话可能已切换条目或关闭：过期解码结果直接丢弃，避免覆盖
            // 后开条目的预览态（对齐 search.rs 的 still_selected 守卫）
            if !app_cb
                .state
                .editor_session()
                .is_some_and(|session| session.item_id == item_id)
            {
                return;
            }
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
    if let Some(label) = format_file_size(size_bytes) {
        parts.push(label);
    }
    parts.join(" · ")
}

// ── 保存 / 关闭 / 删除 ────────────────────────────────────

pub fn text_edited(app: &App, text: &str) {
    if let Some(win) = app.editor.upgrade() {
        win.set_char_count(text.chars().count() as i32);
        win.set_draft_byte_len(text.len() as i32);
    }
}

/// 光标移动（含载入置尾触发）：按 UTF-8 byte offset 计算行列写回底栏。
/// offset 由 Slint 侧从 TextInput 内部属性读出，异常值按边界钳制
pub fn cursor_moved(app: &App, byte_offset: i32) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let text = win.get_draft_text().to_string();
    let (line, col) = cursor_line_col(&text, byte_offset.max(0) as usize);
    win.set_cursor_line(line as i32);
    win.set_cursor_col(col as i32);
}

/// 行列计算（1 起）：列按字符数（中英文同权），offset 越界或落在
/// char 中间时向前钳到合法边界
fn cursor_line_col(text: &str, byte_offset: usize) -> (usize, usize) {
    let mut offset = byte_offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = before[line_start..].chars().count() + 1;
    (line, col)
}

/// 用系统默认程序打开路径（图片预览「打开」按钮 / 文件路径行点击）。
/// 外部程序会抢走前台，属预期行为；失败以错误条提示
pub fn open_externally(app: &App, path: &str) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    if let Err(error) = floatpaste_core::platform::windows::shell_open::open_path(path) {
        warn!("打开路径失败: {error}");
        win.set_notice_text("".into());
        win.set_error_text(format!("打开失败：{error}").into());
    }
}

/// 文件条目：按行索引取路径打开
pub fn open_file_path_at(app: &App, index: usize) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let Some(path) = win.get_file_paths().row_data(index).map(|s| s.to_string()) else {
        return;
    };
    open_externally(app, &path);
}

// ── 按钮 tooltip（悬浮气泡 simple 模式，锚点=按钮下方）────

pub fn editor_is_active(app: &App) -> bool {
    // 编辑窗显示期间会话恒存在（close_editor 先清会话再还原来源）
    app.state.editor_session().is_some()
}

fn button_hint(app: &App, text: &str, x: f32, y: f32) {
    // 活跃校验由 schedule_static 的定时器回调统一兜（host_active），
    // 此处不再重复守卫
    let Some(win) = app.editor.upgrade() else {
        return;
    };
    let Some(hwnd) = win32_ext::window_hwnd(&win) else {
        return;
    };
    let host = HoverHost {
        hwnd,
        is_active: editor_is_active,
    };
    crate::tooltip::schedule_static(app, host, text.to_string(), x, y);
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
            // 对齐旧版 markSaved(draftText)：草稿即视为已保存基线。
            // UI 侧 saved-text 必须同步回写，否则 is-dirty（draft!=saved）
            // 恒为真，保存后 Esc 仍会弹"未保存"确认框
            win.set_saved_text(draft.into());
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
    // 位置记忆须在 hide 前读取：SLINT_DESTROY_WINDOW_ON_HIDE 会销毁 winit 窗口
    let position = win.window().position();
    LAST_POSITION.set(Some((position.x, position.y)));
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
            thumbnails::evict(&session.item_id);
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
    // Esc 分层退出（面板模式）：清空输入、收起面板、焦点回窗口级。
    // 输入非空时 Slint 侧 capture 已先按「仅清空」处理，不会到这里；
    // 此处兜底清空并关闭
    if let Some(win) = app.editor.upgrade() {
        win.set_tag_input_text("".into());
        win.set_tag_open(false);
        win.invoke_focus_root_scope();
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

/* ───────────────── 回调装配 ───────────────── */

/// 编辑窗口回调绑定（main 装配期调用一次）
pub fn wire(app: &App) {
    let Some(win) = app.editor.upgrade() else {
        return;
    };

    let app_cb = app.clone();
    win.on_text_edited(move |text| {
        text_edited(&app_cb, text.as_str());
    });
    let app_cb = app.clone();
    win.on_editor_cursor_moved(move |offset| {
        cursor_moved(&app_cb, offset);
    });
    let app_cb = app.clone();
    win.on_open_externally(move |path| {
        open_externally(&app_cb, path.as_str());
    });
    let app_cb = app.clone();
    win.on_open_file_path_at(move |index| {
        open_file_path_at(&app_cb, index.max(0) as usize);
    });
    let app_cb = app.clone();
    win.on_button_hint(move |text, x, y| {
        button_hint(&app_cb, text.as_str(), x, y);
    });
    let app_cb = app.clone();
    win.on_button_hint_left(move || {
        crate::tooltip::cancel(&app_cb);
    });
    let app_cb = app.clone();
    win.on_save_requested(move || {
        save(&app_cb);
    });
    let app_cb = app.clone();
    win.on_request_close(move || {
        request_close(&app_cb);
    });
    let app_cb = app.clone();
    win.on_close_discard(move || {
        close_editor(&app_cb);
    });
    let app_cb = app.clone();
    win.on_close_save(move || {
        save_then_close(&app_cb);
    });
    let app_cb = app.clone();
    win.on_confirm_cancel(move || {
        cancel_confirm(&app_cb);
    });
    let app_cb = app.clone();
    win.on_delete_requested(move || {
        delete_requested(&app_cb);
    });
    let app_cb = app.clone();
    win.on_tag_input_changed(move |input| {
        tag_input_changed(&app_cb, input.as_str());
    });
    let app_cb = app.clone();
    win.on_tag_commit_add(move || {
        tag_commit_add(&app_cb);
    });
    let app_cb = app.clone();
    win.on_tag_adopt(move |index| {
        tag_adopt(&app_cb, index.max(0) as usize);
    });
    let app_cb = app.clone();
    win.on_tag_complete(move || {
        tag_complete(&app_cb);
    });
    let app_cb = app.clone();
    win.on_tag_remove(move |index| {
        tag_remove(&app_cb, index.max(0) as usize);
    });
    let app_cb = app.clone();
    win.on_tag_remove_last(move || {
        tag_remove_last(&app_cb);
    });
    let app_cb = app.clone();
    win.on_tag_escape(move || {
        tag_escape(&app_cb);
    });
    // 标题栏 X：脏 → 确认框拦截；干净 → 隐藏并返回来源窗口
    let app_cb = app.clone();
    win.window().on_close_requested(move || {
        if window_close_requested(&app_cb) {
            slint::CloseRequestResponse::HideWindow
        } else {
            slint::CloseRequestResponse::KeepWindowShown
        }
    });
}

#[cfg(test)]
mod tests {
    use super::cursor_line_col;

    #[test]
    fn cursor_line_col_counts_chars_and_clamps_offsets() {
        assert_eq!(cursor_line_col("", 0), (1, 1));
        assert_eq!(cursor_line_col("abc", 0), (1, 1));
        assert_eq!(cursor_line_col("abc", 3), (1, 4));
        assert_eq!(cursor_line_col("a\nb", 3), (2, 2));
        // 换行符之后即新行行首
        assert_eq!(cursor_line_col("a\nb", 2), (2, 1));
        // 中文按字符计列（每字 3 字节）
        assert_eq!(cursor_line_col("中文", 3), (1, 2));
        // offset 落在 char 中间：钳到前一个边界
        assert_eq!(cursor_line_col("中文", 4), (1, 2));
        // offset 越界：钳到文本末尾
        assert_eq!(cursor_line_col("ab", 99), (1, 3));
    }
}
