//! Slint 窗口的 Win32 侧装配：HWND 提取、无边框置顶工具窗样式、
//! DWM 悬浮阴影与系统圆角。

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_SYSTEMBACKDROP_TYPE,
    DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{ClientToScreen, InvalidateRect, UpdateWindow};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// 取 Slint 窗口的原始 HWND（窗口创建后可用）
pub fn window_hwnd<W: ComponentHandle>(window: &W) -> Option<isize> {
    let handle = window.window().window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as isize),
        _ => None,
    }
}

/// 强制窗口同步完成一次呈现（InvalidateRect + UpdateWindow 泵出
/// WM_PAINT）。停屏期间 Slint 只把帧画进后备缓冲，窗口表面要等
/// WM_PAINT 才落盘，而屏外窗口收不到自发绘制——上屏前先暖一次
/// 表面，移回屏上的第一帧即有内容，不会闪透明
pub fn warm_surface(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
        let _ = UpdateWindow(hwnd);
    }
}

/// 系统是否开启「透明效果」（设置 > 个性化 > 颜色）。注册表值缺失
/// 或读取失败（异常环境）按开启处理，不因检测问题阻断材质
pub fn system_transparency_enabled() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegGetValueW, RRF_RT_REG_DWORD};

    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    if result != WIN32_ERROR(0) {
        return true;
    }
    value != 0
}

/// 给窗口挂 Win11 系统材质背景（Mica/Acrylic）：暗色联动 + SystemBackdrop
/// + extend frame 铺满客户区。系统关闭「透明效果」或系统不支持该属性
/// （Win10 / Win11 初版）时返回 false 静默跳过——窗口保持自身背景色，
/// 回退即默认态。
///
/// DWM 属性挂在 HWND 上、不受 winit 样式重排影响，停屏方案窗口
/// （速贴/搜索/tooltip）每次显示前挂载即可；走 hide 销毁重建的窗口
/// （编辑/设置）须在重新 show 后重挂。`transient=true` 用 Acrylic
/// （瞬态浮层），false 用 Mica（常驻内容窗）。extend frame 会覆盖
/// [`apply_dwm_shadow`] 的 1px 底边——backdrop 窗口由 DWM 按窗口轮廓
/// 投影与圆角，无需保留
pub fn apply_window_backdrop(hwnd: isize, transient: bool, prefers_dark: bool) -> bool {
    if !system_transparency_enabled() {
        return false;
    }
    let hwnd = HWND(hwnd as *mut _);
    let dark: u32 = u32::from(prefers_dark);
    let backdrop = if transient { DWMSBT_TRANSIENTWINDOW } else { DWMSBT_MAINWINDOW };
    unsafe {
        if DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        )
        .is_err()
        {
            return false;
        }
        if DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const DWM_SYSTEMBACKDROP_TYPE as *const _,
            std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )
        .is_err()
        {
            return false;
        }
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        DwmExtendFrameIntoClientArea(hwnd, &margins).is_ok()
    }
}

/// 窗口物理 DPI（100%=96）
pub fn window_dpi(hwnd: isize) -> u32 {
    unsafe { GetDpiForWindow(HWND(hwnd as *mut _)) }.max(96)
}

pub fn physical_rect(hwnd: isize) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut rect).ok()? };
    Some(rect)
}

/// 客户区左上角的屏幕坐标（ClientToScreen 0,0）。带框窗口（编辑/设置）
/// 的窗口 rect 含标题栏与边框，而 Slint 的 absolute-position 相对客户区——
/// 以窗口 rect 为基准换算会偏移一个非客户区高度（tooltip 锚点错位的根因）
pub fn client_origin(hwnd: isize) -> Option<(i32, i32)> {
    let mut point = POINT::default();
    // ClientToScreen 返回 BOOL（成功非零）
    if unsafe { ClientToScreen(HWND(hwnd as *mut _), &mut point) }.as_bool() {
        Some((point.x, point.y))
    } else {
        None
    }
}

/// 无边框浮层样式：跳过任务栏（TOOLWINDOW）+ 可选不抢焦点（NOACTIVATE）。
/// winit 默认还挂 WS_EX_APPWINDOW——「窗口可见即强制给任务栏按钮」，
/// 与 TOOLWINDOW 相冲，必须显式清掉跳任务栏才生效。
/// 透明渲染由 Slint/winit 的 DWM 合成负责，不要手工加 WS_EX_LAYERED——
/// 未调 SetLayeredWindowAttributes 的分层窗口既不绘制也不命中鼠标
///
/// 速贴/tooltip 传 no_activate=true；搜索窗口需要接收键盘输入，
/// 传 false 保持可激活
pub fn apply_overlay_style(hwnd: isize, no_activate: bool) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut next = ex_style & !(WS_EX_APPWINDOW.0 as isize) | WS_EX_TOOLWINDOW.0 as isize;
        if no_activate {
            next |= WS_EX_NOACTIVATE.0 as isize;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
    }
}

/// 给无边框窗口挂系统悬浮阴影（原版 Tauri shadow(true) 的等效）：
/// DWM 非客户区渲染 + 1px 底部外框即可让合成器绘制标准投影
pub fn apply_dwm_shadow(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    let margins = MARGINS {
        cxLeftWidth: 0,
        cxRightWidth: 0,
        cyTopHeight: 0,
        cyBottomHeight: 1,
    };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }
}

/// 移除 Win11 窗口边框（DWMWA_BORDER_COLOR = NONE）：装饰窗不显式
/// 设置时 DWM 按系统「窗口边框」配色绘制边框，系统浅色模式下为白色
/// 边条，与深色内容割裂（深色标题栏也压不住它）。Win10 无此属性，
/// 调用失败即维持系统默认
pub fn remove_dwm_border(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    let color = DWMWA_COLOR_NONE;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &color as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// Win11 系统圆角（跟随系统半径与投影轮廓，对齐原版观感）：
/// tao 的无边框阴影窗保留 WS_CAPTION/WS_THICKFRAME 样式（WM_NCCALCSIZE
/// 吃掉非客户区），合成器按默认策略给这类窗口自动加圆角；winit 无边框
/// 窗是纯 WS_POPUP，须显式声明。Win10 无此属性，调用失败即无圆角
pub fn apply_dwm_rounded_corners(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const DWM_WINDOW_CORNER_PREFERENCE as *const _,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}
