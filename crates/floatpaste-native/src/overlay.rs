//! 浮层窗口的 Win32 生命周期公共层。
//!
//! winit 在 `window().show()` 后会异步按自身窗口参数重排样式：
//! - 剥掉 TOOLWINDOW/NOACTIVATE（任务栏按钮判定发生在显示瞬间，
//!   一旦生成了按钮无法靠事后补样式撤销）；
//! - 带回 WS_SYSMENU（无边框窗的非客户区虽被 WM_NCCALCSIZE 吃掉，
//!   但显示瞬间 DWM 按含标题栏的窗口绘制一帧非客户区，右上角会
//!   闪过原生关闭按钮）；
//! - SW_SHOW 可能把浮层激活成前台、抢走目标窗口焦点。
//!
//! 因此所有浮层窗口显示后必须经 [`after_show`] 重挂样式与执行前台
//! 策略，禁止直接 show 后自行散落补样式；启动期取 HWND 用
//! [`silent_assemble`]。

use std::time::Duration;

use slint::ComponentHandle;

use floatpaste_core::platform::windows::{active_app::ActiveAppResolver, window_control};

use crate::win32_ext;

/// 显示后的前台归还策略
#[derive(Clone, Copy)]
pub enum ForegroundPolicy {
    /// 不动前台
    Keep,
    /// 无条件把前台还给指定窗口（面板显示路径：还给显示前捕获的目标）
    Restore(isize),
    /// 仅当前台被本浮层占据时归还，不覆盖用户主动的窗口切换
    /// （tooltip 路径 / 面板的延迟兜底复查）
    RestoreIfStolen(isize),
}

/// 启动期静默装配：show 出窗口取 HWND → 挂浮层样式 → 停屏到屏幕外。
/// winit 惰性建窗，必须在事件循环内调用；show 与停屏之间不泵帧，窗口不闪现。
/// 收尾**不走 Slint hide**：hide→show 周期中 winit 清空窗口表面且 Slint
/// 脏区跟踪失效，重现时会得到透明空壳；停屏保持 Slint「已显示」持续渲染，
/// 此后显隐一律在屏上/屏外之间平移，表面内容始终有效
pub fn silent_assemble<W: ComponentHandle>(win: &W, decorated: bool) -> Option<isize> {
    let previous_foreground = ActiveAppResolver::current_foreground_hwnd();
    let _ = win.window().show();
    let hwnd = win32_ext::window_hwnd(win)?;
    win32_ext::apply_overlay_style(hwnd, true);
    if decorated {
        win32_ext::apply_dwm_shadow(hwnd);
        win32_ext::apply_dwm_rounded_corners(hwnd);
    }
    let _ = window_control::remove_window_system_menu(hwnd);
    park_offscreen(win);
    surrender_startup_foreground(hwnd, previous_foreground);
    schedule_style_reassert(hwnd, true);
    Some(hwnd)
}

/// 搜索窗口的启动期静默装配：可激活（接收键盘输入）的 TOOLWINDOW 变体，
/// 只挂阴影不挂系统圆角（旧版搜索窗为方角）。停屏语义同 [`silent_assemble`]
pub fn silent_assemble_focusable<W: ComponentHandle>(win: &W) -> Option<isize> {
    let previous_foreground = ActiveAppResolver::current_foreground_hwnd();
    let _ = win.window().show();
    let hwnd = win32_ext::window_hwnd(win)?;
    win32_ext::apply_overlay_style(hwnd, false);
    win32_ext::apply_dwm_shadow(hwnd);
    let _ = window_control::remove_window_system_menu(hwnd);
    park_offscreen(win);
    surrender_startup_foreground(hwnd, previous_foreground);
    schedule_style_reassert(hwnd, false);
    Some(hwnd)
}

/// 停屏到屏幕外（Windows 坐标下限）：窗口对 Slint 保持「已显示」、
/// 表面内容有效，重现即移回原位
fn park_offscreen<W: ComponentHandle>(win: &W) {
    win.window()
        .set_position(slint::PhysicalPosition::new(-32000, -32000));
}

/// 停屏窗口保持 Win32 可见：若启动 show 激活了自己（winit 首窗语义之外
/// 的 show 走 SW_SHOW），立即把前台归还 show 前的持有者，避免键盘焦点
/// 落进屏幕外窗口
fn surrender_startup_foreground(hwnd: isize, previous: Option<isize>) {
    if ActiveAppResolver::current_foreground_hwnd() != Some(hwnd) {
        return;
    }
    if let Some(previous) = previous {
        let _ = ActiveAppResolver::restore_foreground_window(previous);
    }
}

/// winit 会对 show 过的窗口异步按自身参数重排样式（剥 TOOLWINDOW、带回
/// APPWINDOW/WS_SYSMENU）。停屏窗口保持 Win32 可见，样式被剥会直接长出
/// 任务栏按钮，落地后必须补挂；落地时间不定，两拍都补
fn schedule_style_reassert(hwnd: isize, no_activate: bool) {
    for delay_ms in [0u64, 50] {
        slint::Timer::single_shot(Duration::from_millis(delay_ms), move || {
            win32_ext::apply_overlay_style(hwnd, no_activate);
            let _ = window_control::remove_window_system_menu(hwnd);
        });
    }
}

/// tooltip 启动装配：同 silent_assemble，但收起走 Win32 SW_HIDE 而非
/// Slint hide。winit 对二次 show 固定 SW_SHOW（无视 WS_EX_NOACTIVATE，
/// 激活窗口会打断宿主输入焦点与 IME 组合）；保持其可见标志恒为真、
/// 显隐全部走 Win32（show_window_no_activate / hide_window），才能彻底
/// 绕开 apply_diff 的 ShowWindow
pub fn silent_assemble_win32_hidden<W: ComponentHandle>(win: &W, decorated: bool) -> Option<isize> {
    let _ = win.window().show();
    let hwnd = win32_ext::window_hwnd(win)?;
    win32_ext::apply_overlay_style(hwnd, true);
    if decorated {
        win32_ext::apply_dwm_shadow(hwnd);
        win32_ext::apply_dwm_rounded_corners(hwnd);
    }
    let _ = window_control::remove_window_system_menu(hwnd);
    let _ = window_control::hide_window(hwnd);
    Some(hwnd)
}

/// 搜索窗口显示后的样式兜底：重挂可激活 TOOLWINDOW 样式并保持置顶
/// （不抢焦点、不还原前台——搜索窗本身就该是前台）。winit 显示后的
/// 异步样式重置同样适用，50ms 后兜底重挂一次
pub fn after_show_focusable(hwnd: isize) {
    win32_ext::apply_overlay_style(hwnd, false);
    let _ = window_control::remove_window_system_menu(hwnd);
    window_control::set_window_topmost_no_activate(hwnd);

    slint::Timer::single_shot(Duration::from_millis(50), move || {
        win32_ext::apply_overlay_style(hwnd, false);
        let _ = window_control::remove_window_system_menu(hwnd);
        window_control::set_window_topmost_no_activate(hwnd);
    });
}

/// 显示后立即重挂浮层样式并执行前台策略，50ms 后兜底重挂一次
/// （winit 样式重置的落地时间不定，兜底等不起但也不可省）。
/// click_through 供点击穿透的 tooltip 使用，面板传 false。
///
/// 顺序约束：immediate 前台策略必须先于置顶执行——归还前台是
/// SetForegroundWindow，会把目标窗口提到其 Z 序带顶部；宿主也是置顶
/// 窗口时（搜索窗口），若先置顶 tooltip 再归还，宿主会盖住 tooltip
pub fn after_show(
    hwnd: isize,
    click_through: bool,
    immediate: ForegroundPolicy,
    deferred: ForegroundPolicy,
) {
    win32_ext::apply_overlay_style(hwnd, true);
    let _ = window_control::remove_window_system_menu(hwnd);
    if click_through {
        let _ = window_control::set_window_click_through(hwnd);
    }
    apply_policy(hwnd, &immediate);
    window_control::set_window_topmost_no_activate(hwnd);

    slint::Timer::single_shot(Duration::from_millis(50), move || {
        win32_ext::apply_overlay_style(hwnd, true);
        let _ = window_control::remove_window_system_menu(hwnd);
        if click_through {
            let _ = window_control::set_window_click_through(hwnd);
        }
        window_control::set_window_topmost_no_activate(hwnd);
        apply_policy(hwnd, &deferred);
    });
}

fn apply_policy(hwnd: isize, policy: &ForegroundPolicy) {
    match *policy {
        ForegroundPolicy::Keep => {}
        ForegroundPolicy::Restore(target) => {
            let _ = ActiveAppResolver::restore_foreground_window(target);
        }
        ForegroundPolicy::RestoreIfStolen(target) => {
            if ActiveAppResolver::current_foreground_hwnd() == Some(hwnd) {
                let _ = ActiveAppResolver::restore_foreground_window(target);
            }
        }
    }
}
