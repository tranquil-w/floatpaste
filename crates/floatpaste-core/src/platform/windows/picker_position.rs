use std::mem::size_of;

use windows::core::PWSTR;
use windows::Win32::{
    Foundation::{CloseHandle, HWND, POINT, RECT},
    Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    },
    System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    },
    UI::WindowsAndMessaging::{
        GetCursorPos, GetGUIThreadInfo, GetSystemMetrics, GetWindowTextW, GetWindowThreadProcessId,
        GUITHREADINFO, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    },
};

use crate::domain::error::AppError;

use super::uia_caret::caret_point_via_uia;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// 定位锚点：速贴贴着 `point`（插入符/光标**下沿**）往下展开。
/// `line_height` 是这一行的高度，翻到上方时要用它让开**整行**——只让开空隙
/// 会把窗口压在插入符自己那行上（如输入框首行被切掉半行）。鼠标锚点没有行
/// 概念，取 0。`centered` 标记 provider 给不出插入符列位置的锚点（字段框
/// 兜底）：窗口在 `point` 上**水平居中**打开而非左收偏移——行带/字段框中间
/// 才是「贴着这一行」的预期位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub point: ScreenPoint,
    pub line_height: i32,
    pub centered: bool,
}

impl Anchor {
    /// 鼠标锚点：无行高
    pub fn at_point(point: ScreenPoint) -> Self {
        Self {
            point,
            line_height: 0,
            centered: false,
        }
    }
}

pub fn current_cursor_point() -> Result<ScreenPoint, AppError> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }?;
    Ok(ScreenPoint {
        x: point.x,
        y: point.y,
    })
}

/// 目标窗口的插入符锚点（下沿中点 + 插入符行高）。两条路依次尝试：
///
/// 1. Win32 线程插入符（`GetGUIThreadInfo`）：只对调用 `CreateCaret` 的应用
///    有效——原生编辑控件、部分 Win32 程序；
/// 2. UI Automation 的 TextPattern：浏览器、Electron、WinUI/UWP 等自绘插入符
///    的应用 `hwndCaret` 恒为空，只能走这条（见 [`caret_point_via_uia`]）。
///
/// 两条路的结果都要过「像不像真实插入符」的校验（见 [`usable_probe`]）
pub fn caret_point_for_window(hwnd: isize) -> Result<Anchor, AppError> {
    let (process, title) = window_identity(hwnd);
    tracing::debug!("定位目标 进程={process} 标题='{title}'");

    usable_probe("Win32 线程插入符", win32_caret_point(hwnd))
        .or_else(|| usable_probe("UI Automation", caret_point_via_uia(hwnd)))
        .ok_or_else(|| AppError::Message("目标窗口未暴露可用的插入符位置".to_string()))
}

/// 定位目标的身份：进程名 + 窗口标题（截断 40 字符）。定位日志只有坐标时
/// 对不上「当时的软件」，靠这一行把日志与测试场景关联起来；读不到的字段
/// 留空，不因此产生新的失败
fn window_identity(hwnd: isize) -> (String, String) {
    let hwnd = HWND(hwnd as *mut _);

    let mut process = String::new();
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != 0 {
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buffer = [0u16; 512];
                let mut length = buffer.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    PWSTR(buffer.as_mut_ptr()),
                    &mut length,
                )
                .is_ok()
                {
                    let image = String::from_utf16_lossy(&buffer[..length as usize]);
                    process = image
                        .rsplit(['\\', '/'])
                        .next()
                        .unwrap_or_default()
                        .to_string();
                }
                let _ = CloseHandle(handle);
            }
        }
    }

    let mut title = String::new();
    let mut buffer = [0u16; 128];
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if length > 0 {
        title = String::from_utf16_lossy(&buffer[..length as usize])
            .chars()
            .take(40)
            .collect();
    }

    (process, title)
}

/// 插入符行高的合理上限（物理像素）。真实文本行 ~10-60px，超过它说明
/// provider 把整个字段/页面大框当成了一行（实测 Chrome 新标签页搜索区
/// 1007px、夸克新标签页搜索框 143px），大框的边沿不是插入符位置，宁可回退
/// 鼠标。UIA 链内部也用它下沉校验锚点（见 [`uia_caret`] 的 `trusted`）
pub(crate) const MAX_CARET_LINE_HEIGHT: i32 = 100;

/// 探测结果的统一收尾：失败、坐标不在虚拟屏幕内、或行高超出真实文本行的
/// 合理范围都不要——屏外是窗口最小化时 provider 报的图标化坐标（当锚点会把
/// 速贴甩到「最近的显示器」左上角），行高过大是 provider 拿字段/页面大框
/// 充当了一行几何（见 [`MAX_CARET_LINE_HEIGHT`]），各留一条 debug 日志
fn usable_probe(label: &str, result: Result<Anchor, AppError>) -> Option<Anchor> {
    match result {
        Ok(anchor)
            if is_real_screen_point(anchor.point)
                && anchor.line_height <= MAX_CARET_LINE_HEIGHT =>
        {
            tracing::debug!(
                "{label}定位成功 点=({},{}) 行高={}",
                anchor.point.x,
                anchor.point.y,
                anchor.line_height
            );
            Some(anchor)
        }
        Ok(anchor) => {
            tracing::debug!(
                "{label}锚点不可信: 点=({},{}) 行高={}",
                anchor.point.x,
                anchor.point.y,
                anchor.line_height
            );
            None
        }
        Err(error) => {
            tracing::debug!("{label}不可用: {error}");
            None
        }
    }
}

/// 坐标是否落在虚拟屏幕内（多屏含负原点）
fn is_real_screen_point(point: ScreenPoint) -> bool {
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };

    point.x >= left && point.x < left + width && point.y >= top && point.y < top + height
}

fn win32_caret_point(hwnd: isize) -> Result<Anchor, AppError> {
    let hwnd = HWND(hwnd as *mut _);
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, None) };
    if thread_id == 0 {
        return Err(AppError::Message("未找到目标窗口线程".to_string()));
    }

    let mut gui_info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread_id, &mut gui_info) }?;

    if gui_info.hwndCaret.0.is_null() {
        return Err(AppError::Message("目标线程没有可用插入符".to_string()));
    }

    let mut point = POINT {
        x: gui_info.rcCaret.left + rect_width(gui_info.rcCaret) / 2,
        y: gui_info.rcCaret.bottom,
    };
    unsafe { ClientToScreen(gui_info.hwndCaret, &mut point) }.ok()?;

    Ok(Anchor {
        point: ScreenPoint {
            x: point.x,
            y: point.y,
        },
        line_height: rect_height(gui_info.rcCaret).max(0),
        centered: false,
    })
}

pub fn work_area_from_point(point: ScreenPoint) -> Result<ScreenRect, AppError> {
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: point.x,
                y: point.y,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    if monitor.0.is_null() {
        return Err(AppError::Message("未找到目标显示器".to_string()));
    }

    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut monitor_info as *mut _ as *mut MONITORINFO) }.ok()?;

    Ok(ScreenRect {
        left: monitor_info.rcWork.left,
        top: monitor_info.rcWork.top,
        right: monitor_info.rcWork.right,
        bottom: monitor_info.rcWork.bottom,
    })
}

fn rect_width(rect: RECT) -> i32 {
    rect.right - rect.left
}

fn rect_height(rect: RECT) -> i32 {
    rect.bottom - rect.top
}

#[cfg(test)]
mod tests {
    use super::{current_cursor_point, is_real_screen_point, usable_probe, Anchor, ScreenPoint};
    use crate::domain::error::AppError;

    #[test]
    fn real_screen_check_rejects_offscreen_coordinates() {
        // 停屏位与 Windows 的图标化坐标都不是桌上位置，不能当插入符锚点
        assert!(!is_real_screen_point(ScreenPoint {
            x: -32000,
            y: -32000
        }));
        assert!(!is_real_screen_point(ScreenPoint {
            x: i32::MIN,
            y: i32::MIN
        }));
    }

    /// 真机不变量：真实光标位置必然落在虚拟屏幕内（多屏/负原点/任意缩放
    /// 下都成立，不假设固定分辨率）
    #[test]
    fn real_screen_check_accepts_current_cursor() {
        let Ok(cursor) = current_cursor_point() else {
            return;
        };

        assert!(
            is_real_screen_point(cursor),
            "光标位置被判为屏外: {cursor:?}"
        );
    }

    /// 行高超过真实文本行上限的锚点不可信：provider 把字段/页面大框当成
    /// 了一行（实测 Chrome 新标签页搜索区 1007px、夸克搜索框 143px），
    /// 必须拒绝让调用方回退鼠标
    #[test]
    fn usable_probe_rejects_page_sized_line_height() {
        let caret = |line_height| {
            Ok(Anchor {
                point: current_cursor_point().unwrap(),
                line_height,
                centered: false,
            })
        };

        assert!(usable_probe("测试", caret(21)).is_some());
        assert!(usable_probe("测试", caret(56)).is_some());
        assert!(usable_probe("测试", caret(143)).is_none());
        assert!(usable_probe("测试", caret(1007)).is_none());
        assert!(usable_probe("测试", Err(AppError::Message("x".into()))).is_none());
    }
}
