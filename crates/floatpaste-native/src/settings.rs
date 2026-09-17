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
use floatpaste_core::domain::settings::{PickerPositionMode, ThemeMode, UserSetting};
use floatpaste_core::platform::windows::active_app::ActiveAppResolver;
use floatpaste_core::services::startup_service::StartupService;
use floatpaste_core::services::tag_service::TagService;
use floatpaste_core::theme::ResolvedTheme;
use floatpaste_core::theme;

use crate::picker::App;
use crate::theme_bridge::hex_color;
use crate::{app_icon, theme_bridge, win32_ext};
use crate::{AccentSwatch, PresetCard, SettingsWindow, TagRowData};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(800);
const NOTICE_TIMEOUT: Duration = Duration::from_millis(1800);
/// 分区滚动定位偏移（旧版 settingsScrollSpy SCROLL_OFFSET）
const SCROLL_OFFSET: f32 = 80.0;

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
    if let Some(hwnd) = win32_ext::window_hwnd(&win) {
        app_icon::apply_window_icon(hwnd);
        win32_ext::warm_surface(hwnd);
        // 设置窗口需要真实前台（输入框键盘输入），绕前台锁获取
        if !ActiveAppResolver::force_foreground_window(hwnd) {
            warn!("设置窗口获取前台失败");
        }
    }
    // 窗口级键盘（Esc 关窗）挂在 root-scope capture 上，开窗先聚焦
    win.invoke_focus_root_scope();
    info!("打开设置窗口");
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
    win.set_history_limit_text(settings.history_limit.to_string().into());
    win.set_picker_limit_text(settings.picker_record_limit.to_string().into());
    win.set_position_mode(match settings.picker_position_mode {
        PickerPositionMode::Mouse => 0,
        PickerPositionMode::LastPosition => 1,
        PickerPositionMode::Caret => 2,
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
    if shortcut_conflict(&win) {
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
    if shortcut_conflict(&win) {
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
    }
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

fn set_parts(win: &SettingsWindow, search: bool, value: &str) {
    let parts: Vec<slint::SharedString> = if value.is_empty() {
        Vec::new()
    } else {
        value.split('+').map(slint::SharedString::from).collect()
    };
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
