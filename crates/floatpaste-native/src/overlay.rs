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

/// 启动期静默装配：show 出窗口取 HWND → 挂浮层样式（decorated 时另挂
/// DWM 阴影与系统圆角）→ 立即 hide。winit 惰性建窗，必须在事件循环内
/// 调用；show 与 hide 之间不泵帧，窗口不闪现
pub fn silent_assemble<W: ComponentHandle>(win: &W, decorated: bool) -> Option<isize> {
    let _ = win.window().show();
    let hwnd = win32_ext::window_hwnd(win)?;
    win32_ext::apply_overlay_style(hwnd);
    if decorated {
        win32_ext::apply_dwm_shadow(hwnd);
        win32_ext::apply_dwm_rounded_corners(hwnd);
    }
    let _ = window_control::remove_window_system_menu(hwnd);
    let _ = win.window().hide();
    Some(hwnd)
}

/// 显示后立即重挂浮层样式并执行前台策略，50ms 后兜底重挂一次
/// （winit 样式重置的落地时间不定，兜底等不起但也不可省）。
/// click_through 供点击穿透的 tooltip 使用，面板传 false
pub fn after_show(
    hwnd: isize,
    click_through: bool,
    immediate: ForegroundPolicy,
    deferred: ForegroundPolicy,
) {
    win32_ext::apply_overlay_style(hwnd);
    let _ = window_control::remove_window_system_menu(hwnd);
    window_control::set_window_topmost_no_activate(hwnd);
    if click_through {
        let _ = window_control::set_window_click_through(hwnd);
    }
    apply_policy(hwnd, &immediate);

    slint::Timer::single_shot(Duration::from_millis(50), move || {
        win32_ext::apply_overlay_style(hwnd);
        let _ = window_control::remove_window_system_menu(hwnd);
        window_control::set_window_topmost_no_activate(hwnd);
        if click_through {
            let _ = window_control::set_window_click_through(hwnd);
        }
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
