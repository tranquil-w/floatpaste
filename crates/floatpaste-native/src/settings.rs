//! 设置窗口：打开/水合/800ms 防抖自动保存/运行时联动（热键重注册、自启动
//! 同步、主题应用）+ 标签管理 + 滚动跟随导航。
//!
//! 数据流对齐旧壳 SettingsShell.tsx：任何字段编辑 → 800ms 防抖 →
//! core.update_settings → 应用运行时副作用（SettingsService::
//! apply_runtime_side_effects 的原生等价：sync_registered_shortcuts +
//! StartupService::sync_from_settings + SETTINGS_CHANGED 主题联动；托盘
//! 菜单文案为右键现建，无需推送刷新）。快捷键冲突时拦截保存并在头部
//! 提示「修改暂未保存」；Esc 关窗前 flush 未落盘修改；X 关闭仅隐藏
//! （防抖定时器仍在，随后台落盘，对齐旧壳 prevent_close+hide）。

use std::cell::RefCell;
use std::time::Duration;

use slint::{ComponentHandle, Model, VecModel};
use tracing::{info, warn};

use floatpaste_core::domain::clip_item::TagInfo;
use floatpaste_core::domain::settings::{
    PasteTrigger, PickerPositionMode, SessionKeys, ThemeMode, UserSetting,
};
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::platform::windows::picker_position::{current_cursor_point, work_area_from_point};
use floatpaste_core::platform::windows::session_keyboard::parse_session_combo;
use floatpaste_core::services::startup_service::StartupService;
use floatpaste_core::services::tag_service::TagService;
use floatpaste_core::theme::ResolvedTheme;
use floatpaste_core::theme;

use crate::picker::App;
use crate::theme_bridge::hex_color;
use crate::{app_icon, theme_bridge, win32_ext, winv_takeover};
use crate::{AccentSwatch, PresetCard, SessionKeyRow, SettingsWindow, TagRowData};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(800);
const NOTICE_TIMEOUT: Duration = Duration::from_millis(1800);
/// 分区滚动定位偏移（旧版 settingsScrollSpy SCROLL_OFFSET）
const SCROLL_OFFSET: f32 = 80.0;

/// 会话键行定义（序号 = SessionKeys 字段序号，顺序即界面行序）
const SESSION_KEY_DEFS: [(&str, &str); 8] = [
    ("上屏", "把选中条目粘贴到目标应用。"),
    ("粘贴为文件路径", "把文件或图片的路径作为文本上屏。"),
    ("编辑内容", "打开编辑窗口修改条目文本与标签。"),
    ("收藏 / 取消收藏", "切换选中条目的收藏状态。"),
    ("关闭 / 取消", "收起速贴面板或搜索窗口。"),
    ("向上选择", "选中项上移一条，按住可连发。"),
    ("向下选择", "选中项下移一条，按住可连发。"),
    ("删除条目", "删除选中条目（仅搜索窗口，需二次确认）。"),
];

const STATUS_IDLE: i32 = 0;
const STATUS_SAVING: i32 = 1;
const STATUS_SAVED: i32 = 2;
const STATUS_ERROR: i32 = 3;
const STATUS_BLOCKED: i32 = 4;

thread_local! {
    /// 800ms 防抖保存定时器（单实例，重编辑重置）
    static SAVE_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    /// 录制非法提示的 1800ms 自动清除定时器
    static NOTICE_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    /// 滚动动画驱动定时器（导航点击的程序性滚动）
    static ANIM_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    /// 动画结束后延迟解锁 scroll-spy 的单发定时器
    static UNLOCK_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    /// 滚动动画状态（Some=程序性滚动进行中，scroll-spy 锁定）
    static SCROLL_ANIM: RefCell<Option<ScrollAnim>> = const { RefCell::new(None) };
    /// 最近一次服务端设置：与本地草稿比对判定脏（对齐 lastServerSettingsRef）
    static SAVED: RefCell<Option<UserSetting>> = const { RefCell::new(None) };
    /// Win+V「重启资源管理器」两段式确认的自动复位定时器
    static RESTART_ARM_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

struct ScrollAnim {
    from: f32,
    to: f32,
    started_at: std::time::Instant,
}

impl Clone for ScrollAnim {
    fn clone(&self) -> Self {
        Self {
            from: self.from,
            to: self.to,
            started_at: self.started_at,
        }
    }
}

const ANIM_DURATION: Duration = Duration::from_millis(240);
const ANIM_STEP: Duration = Duration::from_millis(16);

pub fn wire(app: &App) {
    let Some(win) = app.settings.upgrade() else {
        return;
    };

    // ── 快捷键字段 ──
    {
        let app_cb = app.clone();
        win.on_shortcut_edited(move |value| {
            let Some(win) = app_cb.settings.upgrade() else {
                return;
            };
            match normalize_captured(&value) {
                Captured::Value(v) => {
                    win.set_shortcut(v.clone().into());
                    set_parts(&win, false, &v);
                    win.set_shortcut_notice_text("".into());
                    schedule_save(&app_cb);
                }
                Captured::Invalid => {
                    win.set_shortcut_notice_text(
                        "请至少配合 Ctrl、Alt 或 Win 修饰键，避免影响正常打字".into(),
                    );
                    start_notice_timer(&app_cb, false);
                }
                Captured::Cancel => {}
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_shortcut_cancelled(move || {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_shortcut_notice_text("".into());
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_search_shortcut_edited(move |value| {
            let Some(win) = app_cb.settings.upgrade() else {
                return;
            };
            match normalize_captured(&value) {
                Captured::Value(v) => {
                    win.set_search_shortcut(v.clone().into());
                    set_parts(&win, true, &v);
                    win.set_search_notice_text("".into());
                    schedule_save(&app_cb);
                }
                Captured::Invalid => {
                    win.set_search_notice_text(
                        "请至少配合 Ctrl、Alt 或 Win 修饰键，避免影响正常打字".into(),
                    );
                    start_notice_timer(&app_cb, true);
                }
                Captured::Cancel => {}
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_search_shortcut_cancelled(move || {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_search_notice_text("".into());
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_search_shortcut_enabled_toggled(move |enabled| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_search_shortcut_enabled(enabled);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_picker_digit_toggled(move |enabled| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_picker_digit_enabled(enabled);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_winv_toggled(move |checked| {
            let Some(win) = app_cb.settings.upgrade() else {
                return;
            };
            win.set_winv_enabled(checked);
            // 注册表立即写/清（ADR-0001：重启 Explorer 生效，可逆）
            let result = if checked {
                winv_takeover::enable_in_registry()
            } else {
                winv_takeover::disable_in_registry()
            };
            if let Err(error) = result {
                warn!("Win+V 接管注册表写入失败: {error}");
            }
            // 立即保存并重注册（走 apply_side_effects 的统一联动），随后
            // 刷新生效状态提示
            perform_save(&app_cb);
            let failures = app_cb.state.hotkey_failures();
            refresh_hotkey_status_hints(&win, &failures);
            refresh_winv_status(&win, &failures);
        });
    }
    {
        let app_cb = app.clone();
        win.on_winv_restart_explorer(move || {
            let Some(win) = app_cb.settings.upgrade() else {
                return;
            };
            if win.get_winv_restart_armed() {
                win.set_winv_restart_armed(false);
                info!("用户确认重启资源管理器（Win+V 接管生效）");
                winv_takeover::restart_explorer();
            } else {
                // 两段式确认：3 秒内再点执行，超时自动复位
                win.set_winv_restart_armed(true);
                let app_cb_arm = app_cb.clone();
                let timer = slint::Timer::default();
                timer.start(
                    slint::TimerMode::SingleShot,
                    Duration::from_millis(3000),
                    move || {
                        if let Some(win) = app_cb_arm.settings.upgrade() {
                            win.set_winv_restart_armed(false);
                        }
                    },
                );
                RESTART_ARM_TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_session_key_captured(move |action, value| {
            let Some(win) = app_cb.settings.upgrade() else {
                return;
            };
            let action = action.max(0) as usize;
            if action >= SESSION_KEY_DEFS.len() {
                return;
            }
            match normalize_session_captured(&value) {
                Some(combo) => {
                    // 数字 1-9 直达保留：数字开关开启时拒绝裸数字键位
                    if win.get_picker_digit_enabled() && is_bare_digit(&combo) {
                        update_session_row(
                            &win,
                            action,
                            None,
                            Some("数字 1-9 保留给直达上屏：可先关闭「数字键 1-9 直达」，或改用带修饰键的组合。"),
                        );
                        return;
                    }
                    update_session_row(&win, action, Some(&combo), Some(""));
                    schedule_save(&app_cb);
                }
                None => {
                    update_session_row(&win, action, None, Some("无法识别的键位，请重新录制。"));
                }
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_session_key_cancelled(move |action| {
            if let Some(win) = app_cb.settings.upgrade() {
                update_session_row(&win, action.max(0) as usize, None, Some(""));
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_paste_trigger_selected(move |mode| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_paste_trigger(mode);
                schedule_save(&app_cb);
            }
        });
    }

    // ── 数字字段（逐键钳制，对齐 toBoundedNumber）──
    {
        let app_cb = app.clone();
        win.on_history_limit_edited(move |text| {
            if let Some(win) = app_cb.settings.upgrade() {
                let clamped = clamp_number_text(&text, 100, 10_000, 1_000);
                win.set_history_limit_text(clamped.into());
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_picker_limit_edited(move |text| {
            if let Some(win) = app_cb.settings.upgrade() {
                let clamped = clamp_number_text(&text, 9, 1_000, 50);
                win.set_picker_limit_text(clamped.into());
                schedule_save(&app_cb);
            }
        });
    }

    // ── 外观 ──
    {
        let app_cb = app.clone();
        win.on_theme_mode_selected(move |mode| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_theme_mode(mode);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_preset_selected(move |index| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_theme_preset(index);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_accent_selected(move |accent| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_theme_accent_id(accent);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_position_mode_selected(move |mode| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_position_mode(mode);
                schedule_save(&app_cb);
            }
        });
    }

    // ── 行为 ──
    {
        let app_cb = app.clone();
        win.on_launch_on_startup_toggled(move |checked| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_launch_on_startup(checked);
                // 关闭自启动时强制关闭静默启动（对齐旧版 onChange）
                if !checked {
                    win.set_silent_on_startup(false);
                }
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_silent_on_startup_toggled(move |checked| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_silent_on_startup(checked);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_restore_clipboard_toggled(move |checked| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_restore_clipboard(checked);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_pause_monitoring_toggled(move |checked| {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_pause_monitoring(checked);
                schedule_save(&app_cb);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_excluded_apps_edited(move |_text| {
            schedule_save(&app_cb);
        });
    }

    // ── 保存状态 ──
    {
        let app_cb = app.clone();
        win.on_save_retry(move || {
            perform_save(&app_cb);
        });
    }
    {
        let app_cb = app.clone();
        win.on_save_error_dismiss(move || {
            if let Some(win) = app_cb.settings.upgrade() {
                win.set_save_banner_visible(false);
                win.set_status_kind(STATUS_IDLE);
            }
        });
    }

    // ── 关闭：Esc flush 后隐藏；X 仅隐藏 ──
    {
        let app_cb = app.clone();
        win.on_request_close(move || {
            flush_pending_save(&app_cb);
            if let Some(win) = app_cb.settings.upgrade() {
                let _ = win.window().hide();
            }
        });
    }
    {
        let app_cb = app.clone();
        win.window().on_close_requested(move || {
            // 对齐旧壳 configure_settings_window：拦截关闭仅隐藏；
            // 防抖中的修改由仍存活的定时器随后落盘
            let _ = app_cb;
            slint::CloseRequestResponse::HideWindow
        });
    }
    // ── 导航与滚动 ──
    {
        let app_cb = app.clone();
        win.on_nav_selected(move |index| {
            let Some(win) = app_cb.settings.upgrade() else {
                return;
            };
            scroll_to_section(&win, index.max(0) as usize);
        });
    }
    // ── 标签管理 ──
    {
        let app_cb = app.clone();
        win.on_tag_rename_start(move |index| {
            if let Some(win) = app_cb.settings.upgrade() {
                set_tag_row_state(&win, index as usize, 1);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_rename_draft(move |index, text| {
            if let Some(win) = app_cb.settings.upgrade() {
                update_tag_conflict(&win, index as usize, &text);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_rename_commit(move |index, text| {
            commit_tag_rename(&app_cb, index as usize, &text);
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_rename_cancel(move || {
            if let Some(win) = app_cb.settings.upgrade() {
                reset_tag_rows(&win);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_delete_request(move |index| {
            if let Some(win) = app_cb.settings.upgrade() {
                set_tag_row_state(&win, index as usize, 2);
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_delete_confirm(move |index| {
            let Some(win) = app_cb.settings.upgrade() else {
                return;
            };
            let Some(name) = tag_names(&win).get(index.max(0) as usize).cloned() else {
                return;
            };
            match TagService::delete_tag(&app_cb.state.core, &name) {
                Ok(()) => reload_tags(&app_cb, &win),
                Err(error) => {
                    win.set_tag_error_text(format!("删除失败：{error}").into());
                }
            }
        });
    }
    {
        let app_cb = app.clone();
        win.on_tag_delete_cancel(move || {
            if let Some(win) = app_cb.settings.upgrade() {
                reset_tag_rows(&win);
            }
        });
    }
}

/* ───────────────── 打开 / 关闭 / 水合 ───────────────── */

/// 打开设置窗口（托盘「打开设置」与窗口 X 的唯一入口语义）。
/// 速贴活跃时先收起（会话快捷键是全局的，不先解除会劫持设置窗口键盘，
/// 对齐旧壳 WindowCoordinator::open_settings）。
pub fn open(app: &App) {
    if app.state.is_picker_active() {
        crate::picker::hide(app, true);
    }
    let Some(win) = app.settings.upgrade() else {
        warn!("设置窗口尚未就绪，无法打开");
        return;
    };

    hydrate(app, &win);

    // 整帧重绘：hide→show 周期后 Slint 只重绘变化区域（与编辑窗口同理）
    win.set_force_repaint(!win.get_force_repaint());
    let _ = win.window().show();
    // 尺寸在 show 之后显式设置：Slint 隐藏窗口不带真实尺寸，show 首帧会
    // 按根布局收缩到最小（搜索/编辑窗口同款问题），后置 set_size 才生效。
    // 每次打开重置为 920×760（旧版 inner_size 在窗口重建时同样复位；
    // 用户逐次调整的尺寸记忆属旧版 window 级持久化，本壳暂不保留）
    win.window()
        .set_size(slint::LogicalSize::new(920.0 as f32, 760.0 as f32));
    // 窗口级键盘（Esc 关窗）挂在 root-scope capture 上，开窗先聚焦
    win.invoke_focus_root_scope();
    info!("打开设置窗口");
    // SLINT_DESTROY_WINDOW_ON_HIDE 下每次隐藏都销毁 winit 窗口，再次
    // 打开是重建：建窗在下一拍事件循环落地，句柄相关收尾（尺寸补齐/
    // 图标/暖屏/前置）延后执行，否则窗口不在前台
    let app_cb = app.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        if let Some(win) = app_cb.settings.upgrade() {
            win.window()
                .set_size(slint::LogicalSize::new(920.0 as f32, 760.0 as f32));
            // 默认摆位：光标所在显示器工作区中心（设置窗不保留位置记忆；
            // 尺寸此时刚落到真实物理值，直接取窗口尺寸求中心）
            if let Ok(area) = current_cursor_point().and_then(work_area_from_point) {
                let size = win.window().size();
                win.window().set_position(slint::PhysicalPosition::new(
                    (area.left + area.right) / 2 - size.width as i32 / 2,
                    (area.top + area.bottom) / 2 - size.height as i32 / 2,
                ));
            }
            win.invoke_focus_root_scope();
            if let Some(hwnd) = win32_ext::window_hwnd(&win) {
                app_icon::apply_window_icon(hwnd);
                win32_ext::warm_surface(hwnd);
                // 设置窗口需要真实前台（输入框键盘输入），绕前台锁获取
                if !ActiveAppResolver::force_foreground_window(hwnd) {
                    warn!("设置窗口获取前台失败");
                }
            }
        }
    });
}

/// 把当前持久化设置水合进界面（对齐旧版 applyServerSettings：
/// 外部变更——托盘切换监听等——总是覆盖本地草稿）
fn hydrate(app: &App, win: &SettingsWindow) {
    let settings = app.state.current_settings();
    SAVED.with(|slot| *slot.borrow_mut() = Some(settings.clone()));

    win.set_shortcut(settings.shortcut.clone().into());
    set_parts(win, false, &settings.shortcut);
    win.set_search_shortcut(settings.search_shortcut.clone().into());
    set_parts(win, true, &settings.search_shortcut);
    win.set_search_shortcut_enabled(settings.search_shortcut_enabled);
    win.set_search_notice_text("".into());
    win.set_shortcut_notice_text("".into());
    win.set_search_shortcut_hint("".into());
    win.set_picker_digit_enabled(settings.picker_digit_shortcuts_enabled);
    win.set_session_key_rows(slint::ModelRc::new(VecModel::from(session_key_rows(
        &settings,
    ))));
    win.set_session_recorder_id(0);
    win.set_winv_enabled(settings.takeover_winv);
    win.set_winv_restart_armed(false);
    // 全局快捷键注册状态（启动或保存路径的失败记录在此呈现给用户）
    let failures = app.state.hotkey_failures();
    refresh_hotkey_status_hints(win, &failures);
    refresh_winv_status(win, &failures);
    win.set_history_limit_text(settings.history_limit.to_string().into());
    win.set_picker_limit_text(settings.picker_record_limit.to_string().into());
    win.set_position_mode(match settings.picker_position_mode {
        PickerPositionMode::Mouse => 0,
        PickerPositionMode::LastPosition => 1,
        PickerPositionMode::Caret => 2,
    });
    win.set_paste_trigger(match settings.paste_trigger {
        PasteTrigger::Click => 0,
        PasteTrigger::DoubleClick => 1,
    });
    win.set_theme_mode(match settings.theme_mode {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    });
    win.set_theme_preset(theme::THEME_PRESET_IDS
        .iter()
        .position(|id| *id == settings.theme_preset)
        .unwrap_or(0) as i32);
    win.set_theme_accent_id(settings.theme_accent.clone().into());
    win.set_launch_on_startup(settings.launch_on_startup);
    win.set_silent_on_startup(settings.silent_on_startup);
    win.set_restore_clipboard(settings.restore_clipboard_after_paste);
    win.set_pause_monitoring(settings.pause_monitoring);
    win.set_excluded_apps_text(settings.excluded_apps.join("\n").into());

    win.set_status_kind(STATUS_IDLE);
    win.set_status_reasons("".into());
    win.set_save_banner_visible(false);
    win.set_nav_override(-1);
    win.set_scroll_y(0.0);
    SCROLL_ANIM.with(|slot| *slot.borrow_mut() = None);

    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    rebuild_preview_models(win, &settings, resolved);
    reload_tags(app, win);
}

/* ───────────────── 草稿与保存 ───────────────── */

/// 从界面属性收集当前草稿（字段语义对齐旧版 toSettingsPayload）
fn current_draft(win: &SettingsWindow) -> UserSetting {
    let launch_on_startup = win.get_launch_on_startup();
    let history = clamp_number_text(&win.get_history_limit_text(), 100, 10_000, 1_000)
        .parse::<u32>()
        .unwrap_or(1_000);
    let picker_limit = clamp_number_text(&win.get_picker_limit_text(), 9, 1_000, 50)
        .parse::<u32>()
        .unwrap_or(50);
    UserSetting {
        shortcut: win.get_shortcut().to_string(),
        launch_on_startup,
        // 关闭自启动时静默启动强制为 false（对齐 toSettingsPayload）
        silent_on_startup: if launch_on_startup {
            win.get_silent_on_startup()
        } else {
            false
        },
        history_limit: history,
        picker_record_limit: picker_limit,
        picker_position_mode: match win.get_position_mode() {
            1 => PickerPositionMode::LastPosition,
            2 => PickerPositionMode::Caret,
            _ => PickerPositionMode::Mouse,
        },
        paste_trigger: match win.get_paste_trigger() {
            1 => PasteTrigger::DoubleClick,
            _ => PasteTrigger::Click,
        },
        excluded_apps: win
            .get_excluded_apps_text()
            .to_string()
            .split('\n')
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect(),
        restore_clipboard_after_paste: win.get_restore_clipboard(),
        pause_monitoring: win.get_pause_monitoring(),
        theme_mode: match win.get_theme_mode() {
            1 => ThemeMode::Light,
            2 => ThemeMode::Dark,
            _ => ThemeMode::System,
        },
        search_shortcut: win.get_search_shortcut().to_string(),
        search_shortcut_enabled: win.get_search_shortcut_enabled(),
        picker_digit_shortcuts_enabled: win.get_picker_digit_enabled(),
        takeover_winv: win.get_winv_enabled(),
        session_keys: session_keys_from_rows(win),
        theme_preset: theme::THEME_PRESET_IDS
            .get(win.get_theme_preset().max(0) as usize)
            .map(|id| id.to_string())
            .unwrap_or_else(|| "default".to_string()),
        theme_accent: win.get_theme_accent_id().to_string(),
        ..UserSetting::default()
    }
}

/// 与后端比较口径对齐的快捷键归一化：忽略大小写与修饰键顺序
/// （对齐旧版 normalizeShortcutValue）
fn normalize_shortcut_value(value: &str) -> String {
    let mut parts: Vec<String> = value
        .split('+')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    parts.sort();
    parts.join("+")
}

/// 快捷键冲突（对齐旧版 shortcutConflict：搜索键启用且与主键相同）
fn shortcut_conflict(win: &SettingsWindow) -> bool {
    win.get_search_shortcut_enabled()
        && !win.get_shortcut().trim().is_empty()
        && normalize_shortcut_value(&win.get_shortcut())
            == normalize_shortcut_value(&win.get_search_shortcut())
}

/// 会话键冲突：行间键位重复，或与全局快捷键相同。返回首个冲突的提示
/// 文案（无冲突为 None）。归一口径与全局冲突一致（忽略大小写与修饰键
/// 顺序）
fn session_conflict_reason(win: &SettingsWindow) -> Option<String> {
    let keys = session_keys_from_rows(win);
    let entries: Vec<(&str, String)> = SESSION_KEY_DEFS
        .iter()
        .enumerate()
        .map(|(action, (title, _))| (*title, normalize_shortcut_value(keys.field(action))))
        .collect();

    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            if !entries[i].1.is_empty() && entries[i].1 == entries[j].1 {
                return Some(format!("「{}」与「{}」键位相同。", entries[i].0, entries[j].0));
            }
        }
    }
    let main = normalize_shortcut_value(&win.get_shortcut());
    let search_enabled = win.get_search_shortcut_enabled();
    let search = normalize_shortcut_value(&win.get_search_shortcut());
    for (title, normalized) in &entries {
        if !main.is_empty() && *normalized == main {
            return Some(format!("「{title}」与速贴唤起快捷键相同。"));
        }
        if search_enabled && !search.is_empty() && *normalized == search {
            return Some(format!("「{title}」与搜索窗口快捷键相同。"));
        }
    }
    None
}

/// 会话键行模型（按当前设置构建，打开设置窗口时整体水合）
fn session_key_rows(settings: &UserSetting) -> Vec<SessionKeyRow> {
    SESSION_KEY_DEFS
        .iter()
        .enumerate()
        .map(|(action, (title, description))| {
            let value = settings.session_keys.field(action);
            SessionKeyRow {
                action: action as i32,
                title: (*title).into(),
                description: (*description).into(),
                value: value.into(),
                parts: slint::ModelRc::new(VecModel::from(parts_vec(value))),
                notice: "".into(),
            }
        })
        .collect()
}

/// 界面行 → SessionKeys（保存草稿回读）
fn session_keys_from_rows(win: &SettingsWindow) -> SessionKeys {
    let mut keys = SessionKeys::default();
    let model = win.get_session_key_rows();
    for index in 0..model.row_count() {
        if let Some(row) = model.row_data(index) {
            keys.set_field(row.action.max(0) as usize, row.value.to_string());
        }
    }
    keys
}

/// 更新会话键行：value 传 None 表示只改提示（录制取消/拒绝时保留原值）
fn update_session_row(win: &SettingsWindow, action: usize, value: Option<&str>, notice: Option<&str>) {
    let model = win.get_session_key_rows();
    if action >= model.row_count() {
        return;
    }
    if let Some(mut row) = model.row_data(action) {
        if let Some(value) = value {
            row.value = value.into();
            row.parts = slint::ModelRc::new(VecModel::from(parts_vec(value)));
        }
        if let Some(notice) = notice {
            row.notice = notice.into();
        }
        model.set_row_data(action, row);
    }
}

/// 编辑后重置防抖定时器；冲突时拦截保存并置头部提示
fn schedule_save(app: &App) {
    let Some(win) = app.settings.upgrade() else {
        return;
    };
    stop_save_timer();
    if shortcut_conflict(&win) {
        win.set_status_kind(STATUS_BLOCKED);
        win.set_status_reasons("快捷键冲突".into());
        win.set_search_shortcut_hint("与主快捷键相同，请换一组组合；冲突时不会保存。".into());
        return;
    }
    if let Some(reason) = session_conflict_reason(&win) {
        win.set_status_kind(STATUS_BLOCKED);
        win.set_status_reasons(reason.into());
        return;
    }
    win.set_search_shortcut_hint("".into());
    let app_cb = app.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::SingleShot,
        SAVE_DEBOUNCE,
        move || perform_save(&app_cb),
    );
    SAVE_TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
}

fn stop_save_timer() {
    SAVE_TIMER.with(|slot| *slot.borrow_mut() = None);
}

/// 立即保存当前草稿并应用运行时副作用（防抖回调 / 重试 / Esc flush 共用）
fn perform_save(app: &App) {
    let Some(win) = app.settings.upgrade() else {
        return;
    };
    stop_save_timer();
    if shortcut_conflict(&win) || session_conflict_reason(&win).is_some() {
        return;
    }
    let payload = current_draft(&win);
    win.set_status_kind(STATUS_SAVING);
    match app.state.core.update_settings(payload) {
        Ok(saved) => {
            SAVED.with(|slot| *slot.borrow_mut() = Some(saved));
            apply_side_effects(app);
            if let Some(win) = app.settings.upgrade() {
                win.set_status_kind(STATUS_SAVED);
                win.set_save_banner_visible(false);
            }
        }
        Err(error) => {
            warn!("保存设置失败: {error}");
            if let Some(win) = app.settings.upgrade() {
                win.set_status_kind(STATUS_ERROR);
                win.set_save_banner_visible(true);
            }
        }
    }
}

/// Esc 关窗前把防抖中未落盘的修改立即保存（对齐 flushPendingSave）
fn flush_pending_save(app: &App) {
    let Some(win) = app.settings.upgrade() else {
        return;
    };
    if shortcut_conflict(&win) || session_conflict_reason(&win).is_some() {
        return;
    }
    let payload = current_draft(&win);
    // JSON 序列化比较（UserSetting 未实现 PartialEq；与旧版
    // isSameSettings 的 JSON.stringify 口径一致）
    let saved_json = SAVED
        .with(|slot| slot.borrow().clone())
        .and_then(|saved| serde_json::to_string(&saved).ok());
    let payload_json = serde_json::to_string(&payload).ok();
    let unchanged = saved_json.is_some() && saved_json == payload_json;
    if unchanged {
        return;
    }
    perform_save(app);
}

/// 保存后的运行时联动（对齐旧壳 SettingsService::apply_runtime_side_effects；
/// 托盘「切换监听」等外部路径也复用）：
/// 热键重注册 + 自启动同步 + 主题 token 全窗重应用 + 预览模型刷新。
/// 托盘菜单文案右键现建，无需推送刷新。
pub fn apply_side_effects(app: &App) {
    let settings = app.state.current_settings();
    crate::sync_global_hotkeys(app);
    if let Err(error) = StartupService::sync_from_settings(&settings) {
        warn!("同步开机自启失败: {error}");
    }
    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    let tokens = theme::derive_tokens(&settings.theme_preset, &settings.theme_accent, resolved);
    theme_bridge::reapply_theme(app, &tokens);
    if let Some(win) = app.settings.upgrade() {
        rebuild_preview_models(&win, &settings, resolved);
        // 保存路径可能改变快捷键（含 Win+V 接管），注册结果即时呈现
        let failures = app.state.hotkey_failures();
        refresh_hotkey_status_hints(&win, &failures);
        refresh_winv_status(&win, &failures);
    }
}

/// 全局快捷键注册失败的用户可见反馈（1409=组合被其他程序占用）。
/// 空串 = 该键位注册正常
fn refresh_hotkey_status_hints(win: &SettingsWindow, failures: &[(u32, u32)]) {
    let text_for = |id: u32| {
        failures
            .iter()
            .find(|(failed_id, _)| *failed_id == id)
            .map(|(_, code)| match *code {
                crate::ERROR_HOTKEY_OCCUPIED => "注册失败：组合键已被其他程序占用，请换一组组合。".to_string(),
                code => format!("注册失败（错误码 {code}）。"),
            })
            .unwrap_or_default()
    };
    win.set_main_hotkey_status(text_for(1).into());
    win.set_search_hotkey_status(text_for(2).into());
}

/// Win+V 接管生效状态：开关关闭不显示；开启时区分「已注册」与
/// 「未注册」（未重启 Explorer 前注册失败是预期态，给出指引而非报错）
fn refresh_winv_status(win: &SettingsWindow, failures: &[(u32, u32)]) {
    if !win.get_winv_enabled() {
        win.set_winv_status_text("".into());
        return;
    }
    let registered = !failures.iter().any(|(id, _)| *id == 3);
    win.set_winv_status_text(if registered {
        "已接管：Win+V 现在打开速贴面板；重启资源管理器前的提示可忽略。".into()
    } else {
        "尚未生效：重启资源管理器后 Win+V 即由 FloatPaste 接管；若重启后仍显示被占用，说明组合被其他程序持有。".into()
    });
}

/* ───────────────── 预览模型（主题预设卡 / 强调色） ───────────────── */

/// 重建主题预设卡与强调色色点模型：预览颜色用 core derive_tokens 按
/// 「该预设 + 当前强调色 + 当前明暗模式」派生，所见即所得（对齐
/// ThemePresetPicker 的 resolveSemanticTokens 用法）。
fn rebuild_preview_models(win: &SettingsWindow, settings: &UserSetting, resolved: ResolvedTheme) {
    let preset_index = theme::THEME_PRESET_IDS
        .iter()
        .position(|id| *id == settings.theme_preset)
        .unwrap_or(0);
    let preset_names = [("默认", "低色度中性灰基调，跨设备观感最稳定。"),
        ("Catppuccin", "柔和低饱和的社区配色，亮暗为 Latte / Mocha。"),
        ("Tokyo Night", "蓝墨夜色风格，亮暗为 day / night。")];
    let cards: Vec<PresetCard> = theme::THEME_PRESET_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let tokens = theme::derive_tokens(id, &settings.theme_accent, resolved);
            let (name, description) = preset_names
                .get(index)
                .copied()
                .unwrap_or((*id, ""));
            PresetCard {
                name: name.into(),
                description: description.into(),
                canvas_default: hex_color(&tokens.canvas_default),
                canvas_subtle: hex_color(&tokens.canvas_subtle),
                border_muted: hex_color(tokens.border_muted),
                fg_default: hex_color(&tokens.fg_default),
                fg_muted: hex_color(&tokens.fg_muted),
                accent_emphasis: hex_color(&tokens.accent_emphasis),
                fg_on_emphasis: hex_color(&tokens.fg_on_emphasis),
                selected: index == preset_index,
            }
        })
        .collect();
    win.set_preset_cards(slint::ModelRc::new(VecModel::from(cards)));

    // 强调色：跟随预设 + 8 档安全列表（旧版迁移保留的自定义 hex 显示为
    // 无选中态，点任一档即覆盖）
    let dark = matches!(resolved, ResolvedTheme::Dark);
    let mut swatches = vec![AccentSwatch {
        id: "default".into(),
        hex: hex_color(&theme::resolve_accent_hex("default", &settings.theme_preset, resolved)),
        selected: settings.theme_accent == "default",
    }];
    swatches.extend(theme::ACCENT_CHOICES.iter().map(|choice| AccentSwatch {
        id: choice.id.into(),
        hex: hex_color(if dark { choice.dark } else { choice.light }),
        selected: settings.theme_accent == choice.id,
    }));
    win.set_accent_swatches(slint::ModelRc::new(VecModel::from(swatches)));
}

/* ───────────────── 滚动跟随导航 ───────────────── */

/// 导航点击：立即高亮 + ease-out 滚动到分区（对齐旧版 scrollToSection 的
/// smooth 滚动；目标 = 分区顶 80px 下方），动画期间锁定 scroll-spy
fn sec_y(win: &SettingsWindow, index: usize) -> Option<f32> {
    match index {
        0 => Some(win.get_sec_y0()),
        1 => Some(win.get_sec_y1()),
        2 => Some(win.get_sec_y2()),
        3 => Some(win.get_sec_y3()),
        4 => Some(win.get_sec_y4()),
        5 => Some(win.get_sec_y5()),
        _ => None,
    }
}

fn scroll_to_section(win: &SettingsWindow, index: usize) {
    let Some(y) = sec_y(win, index) else {
        return;
    };
    let (view_h, content_h) = (win.get_view_height(), win.get_content_height());
    let target = (SCROLL_OFFSET - y).clamp((view_h - content_h).min(0.0), 0.0);
    // 程序性滚动期间锁定表达式 scroll-spy（对齐旧版 programmaticScrollTarget）
    win.set_nav_override(index as i32);

    let anim = ScrollAnim {
        from: win.get_scroll_y() as f32,
        to: target,
        started_at: std::time::Instant::now(),
    };
    SCROLL_ANIM.with(|slot| *slot.borrow_mut() = Some(anim));
    let win_cb = win.as_weak();
    ANIM_TIMER.with(|slot| {
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            ANIM_STEP,
            move || {
                let Some(win) = win_cb.upgrade() else {
                    SCROLL_ANIM.with(|slot| *slot.borrow_mut() = None);
                    return;
                };
                let Some(current) = SCROLL_ANIM.with(|slot| slot.borrow().clone()) else {
                    return;
                };
                let elapsed = std::time::Instant::now()
                    .duration_since(current.started_at)
                    .min(ANIM_DURATION);
                let progress = elapsed.as_secs_f32() / ANIM_DURATION.as_secs_f32();
                // ease-out（二次缓出，近似浏览器 smooth 滚动手感）
                let eased = 1.0 - (1.0 - progress) * (1.0 - progress);
                win.set_scroll_y(current.from + (current.to - current.from) * eased);
                if progress >= 1.0 {
                    win.set_scroll_y(current.to);
                    // 停止动画；120ms 后解除锁定，高亮交还表达式 scroll-spy
                    // （对齐旧版 PROGRAMMATIC_SCROLL_UNLOCK_DELAY）
                    SCROLL_ANIM.with(|slot| *slot.borrow_mut() = None);
                    let win_for_unlock = win.as_weak();
                    UNLOCK_TIMER.with(|slot| {
                        let unlock = slint::Timer::default();
                        unlock.start(
                            slint::TimerMode::SingleShot,
                            Duration::from_millis(120),
                            move || {
                                if let Some(win) = win_for_unlock.upgrade() {
                                    win.set_nav_override(-1);
                                }
                            },
                        );
                        *slot.borrow_mut() = Some(unlock);
                    });
                }
            },
        );
        *slot.borrow_mut() = Some(timer);
    });
}

/* ───────────────── 标签管理 ───────────────── */

fn tag_names(win: &SettingsWindow) -> Vec<String> {
    let model = win.get_tag_rows();
    (0..model.row_count())
        .filter_map(|index| model.row_data(index).map(|row| row.name.to_string()))
        .collect()
}

fn reload_tags(app: &App, win: &SettingsWindow) {
    win.set_tag_error_text("".into());
    match app.state.core.repository.list_tags() {
        Ok(tags) => {
            let rows: Vec<TagRowData> = tags
                .into_iter()
                .map(|tag: TagInfo| TagRowData {
                    name: tag.name.clone().into(),
                    count_text: format!("{} 条记录", tag.item_count).into(),
                    state: 0,
                    conflict: false,
                })
                .collect();
            win.set_tag_rows(slint::ModelRc::new(VecModel::from(rows)));
        }
        Err(error) => {
            warn!("加载标签失败: {error}");
            win.set_tag_error_text(format!("标签加载失败：{error}").into());
        }
    }
}

fn set_tag_row_state(win: &SettingsWindow, index: usize, state: i32) {
    let model = win.get_tag_rows();
    if index >= model.row_count() {
        return;
    }
    if let Some(mut row) = model.row_data(index) {
        row.state = state;
        model.set_row_data(index, row);
    }
}

fn reset_tag_rows(win: &SettingsWindow) {
    let model = win.get_tag_rows();
    for index in 0..model.row_count() {
        if let Some(mut row) = model.row_data(index) {
            if row.state != 0 {
                row.state = 0;
                model.set_row_data(index, row);
            }
        }
    }
}

/// 重命名草稿变化：与其他标签同名（忽略大小写）时提示将合并
/// （对齐旧版 renameConflict）
fn update_tag_conflict(win: &SettingsWindow, index: usize, text: &str) {
    let name = tag_names(win).get(index).cloned();
    let Some(name) = name else {
        return;
    };
    let trimmed = text.trim();
    let conflict = !trimmed.is_empty()
        && trimmed != name
        && tag_names(win)
            .iter()
            .any(|other| other.to_lowercase() == trimmed.to_lowercase());
    let model = win.get_tag_rows();
    if index < model.row_count() {
        if let Some(mut row) = model.row_data(index) {
            row.conflict = conflict;
            model.set_row_data(index, row);
        }
    }
}

fn commit_tag_rename(app: &App, index: usize, text: &str) {
    let Some(win) = app.settings.upgrade() else {
        return;
    };
    let Some(name) = tag_names(&win).get(index).cloned() else {
        return;
    };
    let new_name = text.trim();
    // 空名或未变化直接退出编辑态（对齐旧版 commitRename 前置检查）
    if new_name.is_empty() || new_name == name {
        reset_tag_rows(&win);
        return;
    }
    match TagService::rename_tag(&app.state.core, &name, new_name) {
        Ok(()) => reload_tags(app, &win),
        Err(error) => {
            win.set_tag_error_text(format!("重命名失败：{error}").into());
        }
    }
}

/* ───────────────── 工具 ───────────────── */

/// 录制结果归一化：单字母主键转大写（Slint 侧按下档字母原样上报）
enum Captured {
    Value(String),
    Invalid,
    Cancel,
}

fn normalize_captured(value: &str) -> Captured {
    if let Some(main) = value.strip_prefix('!') {
        // 非法主键（缺修饰键）：仅产生提示，不改值
        let _ = main;
        return Captured::Invalid;
    }
    if value.is_empty() {
        return Captured::Cancel;
    }
    let mut parts: Vec<String> = value.split('+').map(str::to_string).collect();
    if let Some(last) = parts.last_mut() {
        let chars: Vec<char> = last.chars().collect();
        if chars.len() == 1 && chars[0].is_ascii_alphabetic() {
            *last = chars[0].to_ascii_uppercase().to_string();
        }
    }
    Captured::Value(parts.join("+"))
}

/// 数字输入钳制：越界或非法值收敛到边界/回退值（对齐 toBoundedNumber）
fn clamp_number_text(text: &str, min: u32, max: u32, fallback: u32) -> String {
    let parsed: Option<f64> = text.trim().parse().ok();
    let value = match parsed {
        Some(value) if value.is_finite() => (value.round() as f64)
            .clamp(min as f64, max as f64) as u32,
        _ => fallback,
    };
    value.to_string()
}

/// 会话键录制结果归一化：单字母主键转大写（与全局快捷键录制一致）；
/// 组合本身不可解析时返回 None（录制端正常不会产生）
fn normalize_session_captured(value: &str) -> Option<String> {
    parse_session_combo(value)?;
    let mut parts: Vec<String> = value.split('+').map(str::to_string).collect();
    if let Some(last) = parts.last_mut() {
        let chars: Vec<char> = last.chars().collect();
        if chars.len() == 1 && chars[0].is_ascii_alphabetic() {
            *last = chars[0].to_ascii_uppercase().to_string();
        }
    }
    Some(parts.join("+"))
}

/// 裸数字键位（无修饰键的 0-9）：数字直达开关开启时保留给 1-9 直达
fn is_bare_digit(combo: &str) -> bool {
    parse_session_combo(combo).is_some_and(|parsed| {
        !parsed.ctrl
            && !parsed.alt
            && !parsed.shift
            && !parsed.win
            && parsed.key.chars().all(|char| char.is_ascii_digit())
    })
}

/// 键位串 → 键帽展示拆分（"Ctrl+Enter" → ["Ctrl","Enter"]）
fn parts_vec(value: &str) -> Vec<slint::SharedString> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split('+').map(slint::SharedString::from).collect()
    }
}

fn set_parts(win: &SettingsWindow, search: bool, value: &str) {
    let parts = parts_vec(value);
    if search {
        win.set_search_shortcut_parts(slint::ModelRc::new(VecModel::from(parts)));
    } else {
        win.set_shortcut_parts(slint::ModelRc::new(VecModel::from(parts)));
    }
}

fn start_notice_timer(app: &App, search: bool) {
    let app_cb = app.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::SingleShot, NOTICE_TIMEOUT, move || {
        if let Some(win) = app_cb.settings.upgrade() {
            if search {
                win.set_search_notice_text("".into());
            } else {
                win.set_shortcut_notice_text("".into());
            }
        }
    });
    NOTICE_TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
}
