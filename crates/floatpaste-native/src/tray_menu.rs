//! 托盘菜单：Slint 自绘浮层（跟随应用主题 token），替代 Win32
//! TrackPopupMenu——原生菜单绘制权在系统 uxtheme 手里，只跟系统明暗，
//! 跟不上 themeMode 与强调色预设。
//!
//! - 托盘线程右键仅转发（`tray.rs` WM_RBUTTONUP → dispatch 到事件循环），
//!   菜单窗在事件循环线程打开：光标处弹出、底边贴光标（TPM_BOTTOMALIGN
//!   语义）、按光标所在工作区 clamp（任务栏在上/左右时落到屏内）；
//! - 关闭时机：菜单项点击 / 菜单外点击（WH_MOUSE_LL 会话，速贴同款）/
//!   Esc（会话热键 Escape→Dismiss，纯鼠标菜单的最小键盘兜底）/
//!   其它浮层或窗口打开——`mouse_monitor`/`session_keyboard` 均为单例
//!   会话，速贴/搜索/设置打开路径入口处调 [`close`] 收口互斥；
//! - 无焦点纪律同 tooltip（docs/no-focus-picker.md）：NOACTIVATE、停屏
//!   显隐、裸 SetWindowPos、after_show 重挂样式。

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::WindowFromPoint;

use floatpaste_core::platform::windows::mouse_monitor;
use floatpaste_core::platform::windows::picker_position::{
    current_cursor_point, work_area_from_point,
};
use floatpaste_core::platform::windows::session_keyboard::{self, SessionAction, SessionKeyConfig};
use floatpaste_core::platform::windows::window_control;

use crate::app::App;
use crate::overlay::{self, ForegroundPolicy};
use crate::win32_ext;

/// 菜单项索引（顺序对齐旧壳 MenuBuilder：速贴、搜索、设置、监听、退出）
const MENU_OPEN_PICKER: usize = 0;
const MENU_OPEN_SEARCH: usize = 1;
const MENU_OPEN_SETTINGS: usize = 2;
const MENU_TOGGLE_MONITORING: usize = 3;
const MENU_QUIT: usize = 4;

/// 停屏坐标（Windows 坐标下限）：同 tooltip，保持 Win32 可见、隐藏期重绘照常
const PARKED_POS: (i32, i32) = (-32000, -32000);

/// 外击关闭后的再开抑制窗口：原生 TrackPopupMenu 的第二次右键被模态循环
/// 吞掉、净效果是收起；这里外击钩子的 close（RBUTTONDOWN 触发）与托盘线程
/// 转发的 toggle（RBUTTONUP 触发）先后落进事件循环队列，无守卫会表现为
/// 「收起又立刻重开」。左键外击收起后马上右键托盘（<300ms）同样被抑制，
/// 是为复刻原生语义付出的微小误伤
const REOPEN_SUPPRESS_MS: u64 = 300;

static LAST_OUTSIDE_CLOSE_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

/// 监听菜单文案（对齐旧壳 monitoring_menu_label）
fn monitoring_menu_label(paused: bool) -> &'static str {
    if paused {
        "恢复监听（当前已暂停）"
    } else {
        "暂停监听"
    }
}

fn menu_items(paused: bool) -> Vec<&'static str> {
    vec![
        "打开速贴",
        "打开搜索窗口",
        "打开设置",
        monitoring_menu_label(paused),
        "退出",
    ]
}

/// 装配菜单项命中回调：先收菜单再执行动作（打开类动作与菜单收起解耦，
/// 动作路径内部的 [`close`] 幂等无害）
pub fn wire(app: &App) {
    let Some(win) = app.tray_menu.upgrade() else {
        return;
    };
    let app_cb = app.clone();
    win.on_activate(move |index| {
        let app = app_cb.clone();
        close(&app);
        match index as usize {
            MENU_OPEN_PICKER => crate::picker::activate(&app),
            MENU_OPEN_SEARCH => crate::search::open_global(&app),
            MENU_OPEN_SETTINGS => crate::settings::open(&app),
            MENU_TOGGLE_MONITORING => crate::tray::toggle_monitoring(&app),
            MENU_QUIT => crate::tray::quit(&app),
            _ => {}
        }
    });
}

/// 右键托盘（托盘线程转发）：开着就收（外击抑制窗口内），没开就开
pub fn toggle(app: &App) {
    let last = LAST_OUTSIDE_CLOSE_MS.load(Ordering::SeqCst);
    if last != 0 && now_ms().saturating_sub(last) < REOPEN_SUPPRESS_MS {
        LAST_OUTSIDE_CLOSE_MS.store(0, Ordering::SeqCst);
        return;
    }
    open(app);
}

/// 收起菜单（幂等）：结束键鼠会话 + 停屏（同 tooltip：平移而非 Slint hide，
/// 保持表面有效、下次打开免重建）
pub fn close(app: &App) {
    let hwnd = app.state.tray_menu_hwnd.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    mouse_monitor::end_session();
    session_keyboard::end_session();
    window_control::set_window_position_no_activate(hwnd, PARKED_POS.0, PARKED_POS.1);
}

fn open(app: &App) {
    let Some(win) = app.tray_menu.upgrade() else {
        return;
    };
    let hwnd = app.state.tray_menu_hwnd.load(Ordering::SeqCst);
    if hwnd == 0 {
        // 装配失败：无菜单可用（左键托盘开设置不受影响）
        return;
    }

    let paused = app.state.current_settings().pause_monitoring;
    let items = menu_items(paused);
    win.set_items(ModelRc::new(Rc::new(VecModel::from(
        items
            .iter()
            .map(|text| SharedString::from(*text))
            .collect::<Vec<_>>(),
    ))));

    // 窗口尺寸：宽 = 最长菜单项自然宽（与渲染同排版引擎标尺）+ 横向占用，
    // 高 = 行数 × 行高 + 纵向占用；均再加四周柔影留边。物理像素按光标所在
    // 屏的 DPI 折算（菜单窗停屏在屏外，自身 DPI 不可用）
    let margin = win.get_shadow_margin();
    let dpi = dpi_at_cursor();
    let max_item_width = items
        .iter()
        .map(|text| win.invoke_measure_item_width((*text).into()))
        .fold(0.0f32, f32::max);
    let physical_w = ((max_item_width + win.get_chrome_h() + 2.0 * margin) * dpi)
        .round()
        .max(1.0) as i32;
    let physical_h =
        ((win.get_row_height() * items.len() as f32 + win.get_chrome_v() + 2.0 * margin) * dpi)
            .round()
            .max(1.0) as i32;

    // 出现动画从透明起步；停屏态定尺寸，重绘在屏外完成
    win.set_appear(false);
    window_control::set_window_bounds(hwnd, PARKED_POS.0, PARKED_POS.1, physical_w, physical_h);
    win32_ext::apply_overlay_style(hwnd, true);
    let _ = window_control::remove_window_system_menu(hwnd);

    // 重绘落盘后暖表面、定位上屏（debug 构建整帧渲染慢，同 tooltip 时序）
    let reveal_delay_ms = if cfg!(debug_assertions) { 200 } else { 16 };
    let win_cb = win.as_weak();
    let app_cb = app.clone();
    slint::Timer::single_shot(Duration::from_millis(reveal_delay_ms), move || {
        if win_cb.upgrade().is_none() {
            return;
        }
        win32_ext::warm_surface(hwnd);
        if let Some((x, y)) = resolve_position(physical_w, physical_h) {
            window_control::set_window_bounds(hwnd, x, y, physical_w, physical_h);
        }
        overlay::after_show(
            hwnd,
            false,
            ForegroundPolicy::Keep,
            ForegroundPolicy::Keep,
            || true,
        );
        slint::Timer::single_shot(Duration::from_millis(16), move || {
            if let Some(win) = win_cb.upgrade() {
                win.set_appear(true);
            }
            win32_ext::warm_surface(hwnd);
        });
        // 键鼠会话最后装配（钩子回调由事件循环泵送，重活在前会阻塞回调、
        // 造成全系统光标短暂冻结，同 picker 纪律）
        begin_dismiss_sessions(&app_cb, hwnd);
    });
}

/// 定位：菜单底边贴光标（TPM_BOTTOMALIGN 语义），按光标所在工作区 clamp
/// （任务栏在上/左右时菜单翻到屏内）
fn resolve_position(width: i32, height: i32) -> Option<(i32, i32)> {
    let cursor = current_cursor_point().ok()?;
    let area = work_area_from_point(cursor).ok()?;
    let x = cursor
        .x
        .clamp(area.left, (area.right - width).max(area.left));
    let y = (cursor.y - height).clamp(area.top, (area.bottom - height).max(area.top));
    Some((x, y))
}

/// 光标所在屏的有效 DPI（取命中窗口的窗口 DPI）：托盘菜单锚在光标处，
/// 而自身停屏在屏外，窗口 DPI 是停屏位的
fn dpi_at_cursor() -> f32 {
    let Ok(point) = current_cursor_point() else {
        return 1.0;
    };
    // SAFETY: 原生 API，POINT 为纯数据
    let hwnd = unsafe {
        WindowFromPoint(POINT {
            x: point.x,
            y: point.y,
        })
    };
    if hwnd.is_invalid() || hwnd.0.is_null() {
        return 1.0;
    }
    win32_ext::window_dpi(hwnd.0 as isize) as f32 / 96.0
}

/// 外击收起（WH_MOUSE_LL 会话）+ Esc 收起（会话热键）。回调都转投事件循环。
/// 命中矩形内缩柔影留边（菜单窗同样四周留边，留边内是透明区）
fn begin_dismiss_sessions(app: &App, hwnd: isize) {
    let app_cb = app.clone();
    // 留边按光标所在屏的 DPI 折算（菜单锚在光标处，自身窗口 DPI 是停屏位的）
    let inset = app
        .tray_menu
        .upgrade()
        .map(|win| (win.get_shadow_margin() * dpi_at_cursor()).round() as i32)
        .unwrap_or(0);
    mouse_monitor::begin_session(
        hwnd,
        inset,
        Box::new(move || {
            let app = app_cb.clone();
            let _ = slint::invoke_from_event_loop(move || {
                LAST_OUTSIDE_CLOSE_MS.store(now_ms(), Ordering::SeqCst);
                close(&app);
            });
        }),
    );

    let Some(combo) = session_keyboard::parse_session_combo("Escape") else {
        return;
    };
    let app_cb = app.clone();
    session_keyboard::begin_session(
        SessionKeyConfig {
            digit_shortcuts_enabled: false,
            bindings: vec![(SessionAction::Dismiss, combo)],
        },
        Box::new(move |action| {
            if action == SessionAction::Dismiss {
                let app = app_cb.clone();
                let _ = slint::invoke_from_event_loop(move || close(&app));
            }
        }),
    );
}

#[cfg(test)]
pub(crate) mod headless_contract_tests {
    //! 无头 UI 契约：items 模型渲染、宽度标尺与行命中回调。由
    //! main::l2_headless_ui_contracts 串行调用（winit 事件循环全局单例）

    use std::rc::Rc;

    use slint::ComponentHandle;

    use crate::TrayMenuWindow;

    pub(crate) fn run() {
        let win = TrayMenuWindow::new().expect("实例化托盘菜单窗失败");
        win.window().set_size(slint::LogicalSize::new(200.0, 220.0));

        // 宽度标尺与渲染同引擎：CJK 菜单项必有实质自然宽
        let width = win.invoke_measure_item_width("打开搜索窗口".into());
        assert!(width > 60.0, "菜单项自然宽度度量应非零（实测 {width}）");

        // 行命中：第 3 行竖直中心（±行内 padding 4px 不出 36px 行带）
        let hit: Rc<std::cell::Cell<Option<i32>>> = Rc::new(std::cell::Cell::new(None));
        let hit_cb = hit.clone();
        win.on_activate(move |index| hit_cb.set(Some(index)));
        win.set_items(slint::ModelRc::new(slint::VecModel::from(vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into(),
        ])));
        let margin = win.get_shadow_margin();
        let row = win.get_row_height();
        let y = margin + 2.0 * row + row / 2.0;
        let position = slint::LogicalPosition::new(30.0, y);
        let window = win.window();
        window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        assert_eq!(hit.get(), Some(2), "点击第 3 行应命中索引 2");
    }
}
