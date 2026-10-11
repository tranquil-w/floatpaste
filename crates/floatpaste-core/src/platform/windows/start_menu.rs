//! Shell 表面（开始菜单/搜索面板）的识别与几何。
//!
//! 开始菜单位于桌面 z-order 带之上，速贴的 WS_EX_TOPMOST 压不过它：
//! 开始菜单开着时落位进它的矩形必被整块盖住。定位服务据此把速贴改放
//! 开始菜单旁边（同屏共存，勿关用户正开着的菜单）；唤起装配亦跳过把
//! shell 窗口记为回贴目标——shell 窗口当插入符锚点与回贴目标都无意义。
//!
//! 开始菜单开着的宿主形态不止一种：Win11 24H2 上前台是 SearchHost 的
//! 「搜索」CoreWindow，其窗口矩形就是可见面板；个别形态的宿主窗是
//! **全屏**不可见窗（承载触控命中与外点关闭），`GetWindowRect` 是整屏
//! 矩形——此时改从 UI Automation 树取可见面板，见
//! [`foreground_start_menu_rect`]。

use super::picker_position::ScreenRect;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Accessibility::IUIAutomationElement;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

/// 开始菜单/搜索面板宿主进程（小写比对）：Win11 24H2 开始菜单开着时
/// 前台是 SearchHost 的「搜索」CoreWindow（真机日志实证），另一形态是
/// StartMenuExperienceHost，两者都认
const START_MENU_HOST_PROCESSES: [&str; 2] = ["startmenuexperiencehost.exe", "searchhost.exe"];

/// 宿主矩形与显示器两维重叠都达到该百分比即判全屏宿主（Win10 的开始
/// 菜单宿主矩形就是菜单本体，不走 UIA）
const FULL_SCREEN_HOST_PERCENT: i32 = 95;

/// UIA 找菜单面板的下钻深度上限：XAML 根面板在宿主元素下 1-2 层内
const UIA_FLYOUT_MAX_DEPTH: usize = 3;

/// 菜单面板的最小边长（物理像素）：真子矩形里排除装饰小件
const MIN_FLYOUT_EDGE_PX: i32 = 100;

fn is_start_menu_host_process(name: &str) -> bool {
    START_MENU_HOST_PROCESSES
        .iter()
        .any(|host| name.eq_ignore_ascii_case(host))
}

/// 前台是否为开始菜单（宿主进程的窗口只在开始菜单打开时到前台）
pub fn foreground_is_start_menu() -> bool {
    super::active_app::ActiveAppResolver::current_foreground_process_name()
        .is_some_and(|name| is_start_menu_host_process(&name))
}

/// 开始菜单可见区域的屏幕矩形（物理像素）。开始菜单开着时其宿主窗口
/// 即前台窗口，矩形通常就是可见面板；未打开或几何不可得返回 None。
///
/// 个别宿主形态是全屏不可见窗（矩形两维覆盖显示器 ≥95%），此时改从
/// UIA 树取可见菜单面板（宿主元素下的最大真子矩形）；取不到时返回
/// None，调用方退回配置定位模式，宁可不给也不给整屏矩形
pub fn foreground_start_menu_rect() -> Option<ScreenRect> {
    if !foreground_is_start_menu() {
        return None;
    }
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let host = to_screen_rect(&rect);

    let full_screen = monitor_rect(hwnd)
        .is_some_and(|monitor| covers_percent(&host, &monitor, FULL_SCREEN_HOST_PERCENT));
    if !full_screen {
        return Some(host);
    }

    let flyout = flyout_rect_via_uia(hwnd, &host);
    if flyout.is_none() {
        tracing::warn!("开始菜单宿主为全屏矩形，UIA 未取到可见菜单面板，退回配置定位模式");
    }
    flyout
}

/// 全屏宿主下取可见菜单面板：广度优先走 UIA 控件视图，每层先收集子元素，
/// 再在其中找「真子矩形」（见 [`proper_subrect`]）里面积最大的一块——
/// 首个出现真子矩形的层里最大的一块即 XAML 根面板（菜单本体），同层的
/// 全屏中间容器被真子矩形判定天然跳过
fn flyout_rect_via_uia(host_hwnd: HWND, host: &ScreenRect) -> Option<ScreenRect> {
    let client = super::uia_caret::uia_client().ok()?;
    let host_element = unsafe { client.ElementFromHandle(host_hwnd) }.ok()?;
    let walker = unsafe { client.ControlViewWalker() }.ok()?;

    let mut frontier = vec![host_element];
    for _ in 0..UIA_FLYOUT_MAX_DEPTH {
        let mut children: Vec<IUIAutomationElement> = Vec::new();
        for element in &frontier {
            let mut child = unsafe { walker.GetFirstChildElement(element) };
            while let Ok(current) = child {
                child = unsafe { walker.GetNextSiblingElement(&current) };
                children.push(current);
            }
        }

        let mut best: Option<(i64, ScreenRect)> = None;
        for element in &children {
            let Some(rect) = (unsafe { element.CurrentBoundingRectangle() })
                .ok()
                .and_then(|rect| proper_subrect(&to_screen_rect(&rect), host))
            else {
                continue;
            };
            let area = rect.width() as i64 * rect.height() as i64;
            if best
                .as_ref()
                .map_or(true, |(best_area, _)| area > *best_area)
            {
                best = Some((area, rect));
            }
        }
        if let Some((_, rect)) = best {
            tracing::debug!(
                "UIA 取到开始菜单面板 ({},{})-({},{})",
                rect.left,
                rect.top,
                rect.right,
                rect.bottom
            );
            return Some(rect);
        }
        if children.is_empty() {
            break;
        }
        frontier = children;
    }
    None
}

/// 菜单面板候选判定：非退化（边长 ≥ [`MIN_FLYOUT_EDGE_PX`]）且两维都比
/// 宿主小 [`FULL_SCREEN_HOST_PERCENT`]% 以上——全屏中间容器与装饰小件
/// 都不算。不满足时返回 None，满足时原样返回该矩形
fn proper_subrect(rect: &ScreenRect, host: &ScreenRect) -> Option<ScreenRect> {
    (rect.width() >= MIN_FLYOUT_EDGE_PX
        && rect.height() >= MIN_FLYOUT_EDGE_PX
        && rect.width() * 100 < host.width() * FULL_SCREEN_HOST_PERCENT
        && rect.height() * 100 < host.height() * FULL_SCREEN_HOST_PERCENT)
        .then_some(*rect)
}

fn to_screen_rect(rect: &RECT) -> ScreenRect {
    ScreenRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

/// 窗口所在显示器的全屏矩形（非工作区——全屏宿主窗覆盖的是整块显示器）
fn monitor_rect(hwnd: HWND) -> Option<ScreenRect> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info as *mut MONITORINFO) }.as_bool() {
        return None;
    }
    Some(ScreenRect {
        left: info.rcMonitor.left,
        top: info.rcMonitor.top,
        right: info.rcMonitor.right,
        bottom: info.rcMonitor.bottom,
    })
}

/// rect 覆盖 reference 的比例是否两维都达到 percent%
fn covers_percent(rect: &ScreenRect, reference: &ScreenRect, percent: i32) -> bool {
    let covered = |a1: i32, a2: i32, b1: i32, b2: i32| {
        (a2.min(b2) - a1.max(b1)).max(0) * 100 >= percent * (b2 - b1).max(1)
    };
    covered(rect.left, rect.right, reference.left, reference.right)
        && covered(rect.top, rect.bottom, reference.top, reference.bottom)
}

#[cfg(test)]
mod tests {
    use super::{covers_percent, is_start_menu_host_process, proper_subrect, MIN_FLYOUT_EDGE_PX};
    use crate::platform::windows::picker_position::ScreenRect;

    const MONITOR: ScreenRect = ScreenRect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn start_menu_host_process_matches_case_insensitively() {
        assert!(is_start_menu_host_process("StartMenuExperienceHost.exe"));
        assert!(is_start_menu_host_process("startmenuexperiencehost.exe"));
        assert!(is_start_menu_host_process("SearchHost.exe"));
        assert!(!is_start_menu_host_process("explorer.exe"));
        assert!(!is_start_menu_host_process("ApplicationFrameHost.exe"));
    }

    /// 全屏宿主判定：整屏矩形（Win11 CoreWindow）判真；Win10 式的半屏
    /// 菜单本体矩形判假
    #[test]
    fn covers_percent_detects_full_screen_host() {
        let full_screen = MONITOR;
        let win10_menu = ScreenRect {
            left: 0,
            top: 0,
            right: 960,
            bottom: 1040,
        };

        assert!(covers_percent(&full_screen, &MONITOR, 95));
        assert!(!covers_percent(&win10_menu, &MONITOR, 95));
    }

    /// 菜单面板候选：Win11 居中菜单矩形算数；全屏中间容器（两维都 ≥95%）
    /// 与装饰小件（边长 <100px）都不算
    #[test]
    fn proper_subrect_accepts_pane_rejects_container_and_trinkets() {
        let pane = ScreenRect {
            left: 640,
            top: 300,
            right: 1280,
            bottom: 1030,
        };
        let trinket = ScreenRect {
            left: 0,
            top: 0,
            right: MIN_FLYOUT_EDGE_PX - 1,
            bottom: MIN_FLYOUT_EDGE_PX - 1,
        };

        assert_eq!(proper_subrect(&pane, &MONITOR), Some(pane));
        assert_eq!(proper_subrect(&MONITOR, &MONITOR), None);
        assert_eq!(proper_subrect(&trinket, &MONITOR), None);
    }
}
