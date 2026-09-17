//! 应用图标的 Windows 原生加载（窗口标题栏/任务栏图标 + 托盘图标）。
//!
//! 按系统 DPI 的 SM_CXSMICON / SM_CXICON·3/4 档位从内嵌 ICO 资源
//! LoadImageW，让 Windows 从 ICO 组里挑最近条目，避免单一 RGBA
//! 位图被系统拉伸发糊。
//! 资源 ID=1（floatpaste-native.rc: `1 ICON "assets/icon.ico"`）。

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SendMessageW, HICON, IMAGE_ICON, LR_DEFAULTSIZE,
    SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
};

const ICON_RESOURCE_ID: u16 = 1;

/// 已加载的 (small, big) HICON（存 isize 以便放入静态缓存）。
/// HICON 被窗口引用期间必须保持有效，且各窗口可复用同一份，
/// 因此常驻缓存、从不销毁；句柄按系统 DPI 加载，系统缩放变更需重启应用刷新。
static CACHED_WINDOW_ICONS: OnceLock<(isize, isize)> = OnceLock::new();

/// 给窗口设置小/大图标（WM_SETICON：0=标题栏小图标，1=任务栏大图标）。
/// 加载失败静默跳过（保持系统默认图标）。
pub fn apply_window_icon(hwnd: isize) {
    let Some((small, big)) = load_window_icons() else {
        return;
    };
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        SendMessageW(hwnd, WM_SETICON, Some(WPARAM(0)), Some(LPARAM(small)));
        SendMessageW(hwnd, WM_SETICON, Some(WPARAM(1)), Some(LPARAM(big)));
    }
}

/// 托盘图标：按小图标档位加载的 HICON；None=加载失败（调用方跳过图标）。
pub fn tray_icon_hicon() -> Option<isize> {
    load_window_icons().map(|(small, _)| small)
}

fn load_window_icons() -> Option<(isize, isize)> {
    if let Some(icons) = CACHED_WINDOW_ICONS.get() {
        return Some(*icons);
    }

    let icons = unsafe {
        let module = GetModuleHandleW(PCWSTR::null()).ok()?;
        let instance = HINSTANCE(module.0);
        let load = |width: i32, height: i32| -> Option<isize> {
            let handle = LoadImageW(
                Some(instance),
                PCWSTR(ICON_RESOURCE_ID as usize as _),
                IMAGE_ICON,
                width,
                height,
                LR_DEFAULTSIZE,
            )
            .ok()?;
            Some(HICON(handle.0).0 as isize)
        };
        let small = load(GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON))?;
        // 任务栏以 SM_CXICON 的 3/4（100% 缩放下 24px）绘制按钮图标，按该尺寸
        // 取档可 1:1 命中 ICO 中 24/30/36px 档位，避免 shell 二次缩放糊掉细线条
        let big = load(
            GetSystemMetrics(SM_CXICON) * 3 / 4,
            GetSystemMetrics(SM_CYICON) * 3 / 4,
        )?;
        (small, big)
    };

    if CACHED_WINDOW_ICONS.set(icons).is_err() {
        return CACHED_WINDOW_ICONS.get().copied();
    }
    Some(icons)
}
