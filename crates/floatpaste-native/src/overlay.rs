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
use floatpaste_core::theme;

use crate::picker::App;
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
    win32_ext::install_caption_strip_subclass(hwnd);
    park_offscreen(win);
    surrender_startup_foreground(hwnd, previous_foreground);
    schedule_style_reassert(hwnd, false);
    Some(hwnd)
}

/// 内容窗（编辑/设置）的启动期静默装配：no-frame 自绘标题栏窗，停屏
/// 期挂 TOOLWINDOW 屏蔽任务栏按钮（上屏前由显示流程摘除）；阴影、
/// 系统圆角与系统窗控样式剥除须显式挂载。停屏到屏幕外保持 Slint
/// 「已显示」与表面内容有效，显隐一律屏外/屏上平移——彻底绕开
/// SLINT_DESTROY_WINDOW_ON_HIDE 的销毁重建时序（重建是首开无材质、
/// 透明首帧、显示态异常等问题的共同根源）
pub fn silent_assemble_content<W: ComponentHandle>(win: &W) -> Option<isize> {
    let previous_foreground = ActiveAppResolver::current_foreground_hwnd();
    let _ = win.window().show();
    let hwnd = win32_ext::window_hwnd(win)?;
    win32_ext::set_toolwindow_style(hwnd, true);
    // no-frame 窗（WS_POPUP）：阴影与系统圆角都须显式挂载
    win32_ext::apply_dwm_shadow(hwnd);
    win32_ext::apply_dwm_rounded_corners(hwnd);
    let _ = window_control::remove_window_system_menu(hwnd);
    win32_ext::install_caption_strip_subclass(hwnd);
    win.window()
        .set_position(slint::PhysicalPosition::new(-32000, -32000));
    surrender_startup_foreground(hwnd, previous_foreground);
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

/// 内容窗上屏后两拍补剥系统窗控样式：WS_SYSMENU 硬编码在 winit 的期望
/// 样式里（window_state.rs to_window_styles），显示序列中随时可能被重刷
/// 回来且时机不定；上屏后补剥收口，此后的异步重刷由 caption_strip
/// 子类兜底（见 [`win32_ext::install_caption_strip_subclass`]）
pub fn schedule_caption_strip(hwnd: isize) {
    for delay_ms in [0u64, 50] {
        slint::Timer::single_shot(Duration::from_millis(delay_ms), move || {
            let _ = window_control::remove_window_system_menu(hwnd);
        });
    }
}

/// 材质管线按窗口生命周期分流（对齐 PowerToys 选型：常驻内容窗
/// Mica、可激活瞬态浮层 Acrylic、无焦点窗专项管线）
pub enum MaterialSurface {
    /// 编辑/设置：常驻内容窗 → SystemBackdrop Mica
    Content,
    /// 搜索：有焦点瞬态浮层 → SystemBackdrop Acrylic
    Transient,
    /// 速贴：无焦点窗，SystemBackdrop 非前台自动降级纯色 →
    /// SWCA HOSTBACKDROP + Acrylic 绕过降级
    HostAcrylic,
}

/// 全应用统一材质挂载（明暗/回退门控在此收口）。
///
/// 不支持/系统透明关闭时返回 false，调用方把 material-active 置 false
/// 让面板回不透明底。挂载幂等，每次显示/恢复重挂即可
pub fn apply_material(app: &App, hwnd: isize, surface: MaterialSurface) -> bool {
    let settings = app.state.current_settings();
    let resolved = theme::resolve_theme(settings.theme_mode.clone(), theme::system_prefers_dark());
    let dark = resolved == theme::ResolvedTheme::Dark;
    match surface {
        MaterialSurface::Content => win32_ext::apply_window_backdrop(hwnd, false, dark),
        MaterialSurface::Transient => win32_ext::apply_window_backdrop(hwnd, true, dark),
        MaterialSurface::HostAcrylic => win32_ext::apply_window_host_backdrop_acrylic(hwnd, dark),
    }
}

/// tooltip 启动装配：样式同 silent_assemble，收起走**停屏**（-32000 平移）
/// 而非 SW_HIDE——SW_HIDE 隐藏期间 winit 抑制重绘（速贴/搜索停屏方案的
/// 同源问题），气泡每次显示尺寸不同，隐藏态 resize 的首帧会残缺闪烁；
/// 停屏保持 Win32 可见、隐藏期正常重绘，移上屏即完整内容。winit 对二次
/// show 固定 SW_SHOW（无视 WS_EX_NOACTIVATE，激活窗口会打断宿主输入焦点
/// 与 IME 组合），不用 Slint/Win32 的 show 显隐，收起只平移
pub fn silent_assemble_parked<W: ComponentHandle>(win: &W, decorated: bool) -> Option<isize> {
    let _ = win.window().show();
    let hwnd = win32_ext::window_hwnd(win)?;
    win32_ext::apply_overlay_style(hwnd, true);
    if decorated {
        win32_ext::apply_dwm_shadow(hwnd);
        win32_ext::apply_dwm_rounded_corners(hwnd);
    }
    let _ = window_control::remove_window_system_menu(hwnd);
    win.window()
        .set_position(slint::PhysicalPosition::new(-32000, -32000));
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
