//! 系统托盘：Shell_NotifyIcon 图标 + 右键转发，独立线程消息循环。
//!
//! - 左键单击托盘 = 打开设置（show_menu_on_left_click(false) 语义）；
//! - 右键转发到事件循环线程的 `tray_menu::toggle`——菜单本体是 Slint
//!   自绘浮层（跟随应用主题），见 tray_menu.rs；
//! - 图标按 DPI 取档（app_icon::tray_icon_hicon，同旧壳 tray_icon_image）；
//! - 监听 TaskbarCreated 广播，explorer 重启后自动重挂图标。

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use tracing::{info, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, HICON, MSG, WINDOW_EX_STYLE, WM_APP, WM_DESTROY,
    WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

use crate::picker::App;

/// 托盘回调消息：wParam=图标 id，lParam=鼠标消息
const WM_APP_TRAY: u32 = WM_APP + 0x4E;

static TRAY_APP: OnceLock<Mutex<Option<App>>> = OnceLock::new();
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
/// 托盘图标所在隐藏窗口句柄（气泡通知经它 NIM_MODIFY）
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);

/// 启动托盘线程（幂等：重复调用忽略）。线程持有 App 副本，菜单动作经
/// `slint::invoke_from_event_loop` 回事件循环执行。
pub fn start(app: App) {
    if TRAY_APP.set(Mutex::new(Some(app))).is_err() {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("tray".into())
        .spawn(|| unsafe { run() })
    {
        warn!("托盘线程启动失败: {error}");
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 把动作派发到事件循环线程执行（托盘线程不直接触碰 UI）
fn dispatch(action: impl FnOnce(&App) + Send + 'static) {
    let result = slint::invoke_from_event_loop(move || {
        let Some(lock) = TRAY_APP.get() else {
            return;
        };
        let Ok(guard) = lock.lock() else {
            return;
        };
        let Some(app) = guard.as_ref() else {
            return;
        };
        action(app);
    });
    if let Err(error) = result {
        warn!("托盘动作派发失败: {error}");
    }
}

/// 托盘图标悬停提示（szTip；explorer 重启重挂后同文案）
const TIP_TEXT: &str = "FloatPaste · 剪贴板历史";

unsafe fn add_icon(hwnd: HWND) {
    let Some(lock) = TRAY_APP.get() else {
        return;
    };
    let Ok(guard) = lock.lock() else {
        return;
    };
    let Some(_app) = guard.as_ref() else {
        return;
    };
    let hicon = crate::app_icon::tray_icon_hicon().unwrap_or_default();
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: if hicon != 0 {
            NIF_MESSAGE | NIF_ICON | NIF_TIP
        } else {
            NIF_MESSAGE | NIF_TIP
        },
        uCallbackMessage: WM_APP_TRAY,
        hIcon: HICON(hicon as *mut _),
        // szTip 是定长 UTF-16 数组，须以 NUL 结尾
        szTip: {
            let mut tip = [0u16; 128];
            for (slot, unit) in tip.iter_mut().zip(TIP_TEXT.encode_utf16()) {
                *slot = unit;
            }
            tip
        },
        ..Default::default()
    };
    if !Shell_NotifyIconW(NIM_ADD, &mut nid).as_bool() {
        warn!("添加托盘图标失败");
    }
    TRAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
}

unsafe fn remove_icon(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
    TRAY_HWND.store(0, Ordering::SeqCst);
}

/// 管理员目标提示气泡文案（szInfo 定长 128 UTF-16，控制在限量内）
const ADMIN_TARGET_INFO: &str =
    "目标窗口以管理员权限运行，FloatPaste 无法向其自动粘贴。内容已在剪贴板，可手动 Ctrl+V；或在设置中开启「以管理员身份运行」。";

/// 启动期提权自检未获确认（UAC 取消）的气泡文案
const ELEVATION_DECLINED_INFO: &str =
    "已开启「始终以管理员身份运行」，但本次启动未获得管理员权限（UAC 未确认）。向管理员窗口的自动粘贴将无法生效；重新打开 FloatPaste 并确认即可。";

/// 操作失败的托盘警告气泡（不占用窗口 UI）。Win10+ 转系统通知，无弹窗打扰。
/// 动态文案按 UTF-16 容量截断：szInfo 定长 256 且须以 NUL 结尾
pub fn notify_failure(info: &str) {
    let info: String = info.chars().take(200).collect();
    let hwnd = TRAY_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(hwnd as *mut _),
        uID: 1,
        uFlags: NIF_INFO,
        szInfo: {
            // szInfo 是定长 256 UTF-16 数组，须以 NUL 结尾
            let mut buffer = [0u16; 256];
            for (slot, unit) in buffer.iter_mut().zip(info.encode_utf16()) {
                *slot = unit;
            }
            buffer
        },
        szInfoTitle: {
            let mut title = [0u16; 64];
            for (slot, unit) in title.iter_mut().zip("FloatPaste".encode_utf16()) {
                *slot = unit;
            }
            title
        },
        dwInfoFlags: NIIF_WARNING,
        ..Default::default()
    };
    unsafe {
        if !Shell_NotifyIconW(NIM_MODIFY, &mut nid).as_bool() {
            warn!("托盘气泡通知失败");
        }
    }
}

/// 管理员目标无法自动回贴时的一次性说明
pub fn notify_admin_target() {
    notify_failure(ADMIN_TARGET_INFO);
}

/// 启动期提权自检未获确认（用户在 UAC 中取消）时的一次性说明
pub fn notify_elevation_declined() {
    notify_failure(ELEVATION_DECLINED_INFO);
}

/// 切换监听暂停（对齐旧壳 toggle-monitoring：翻转设置 + 应用运行时副作用）。
/// 菜单经 TrackPopupMenu+TPM_RETURNCMD 返回值分发，一次点击恰好一次调用，
/// 无需旧壳时代的菜单事件去抖（那针对 Tauri 菜单库的双触发缺陷）。
/// E2E 测试经命名事件复用同一路径（main.rs 装载，见 single_instance 模块）。
pub(crate) fn toggle_monitoring(app: &App) {
    let mut settings = app.state.current_settings();
    settings.pause_monitoring = !settings.pause_monitoring;
    match app.state.core.update_settings(settings) {
        Ok(saved) => {
            info!(
                "托盘切换监听已生效：pause_monitoring={}",
                saved.pause_monitoring
            );
            crate::settings::apply_side_effects(app);
        }
        Err(error) => warn!("托盘更新监听状态失败: {error}"),
    }
}

/// 退出：置退出标志并结束事件循环，main 在循环退出后统一收尾
/// （停钩子/监听/热键，对齐旧壳 prepare_for_exit + exit(0)）
pub(crate) fn quit(app: &App) {
    // 「事件循环已返回」日志无法区分 quit 来源，此处留痕：出现非用户
    // 操作的退出时可排查菜单命令是否被误配
    info!("托盘退出菜单命中，请求事件循环退出");
    app.state.core.begin_quit();
    let _ = slint::quit_event_loop();
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = *TASKBAR_CREATED.get_or_init(|| unsafe {
        RegisterWindowMessageW(PCWSTR(to_wide("TaskbarCreated").as_ptr()))
    });
    if msg == taskbar_created {
        // explorer 重启后托盘图标丢失：重新挂载；同时系统对 Win+V 的
        // 持有状态已翻转，交设置模块补注册或解除恢复过渡提示
        add_icon(hwnd);
        dispatch(crate::settings::on_explorer_restarted);
        return LRESULT(0);
    }
    match msg {
        WM_APP_TRAY => {
            match lparam.0 as u32 {
                WM_LBUTTONUP => dispatch(|app| crate::settings::open(app)),
                // 菜单本体是 Slint 浮层（tray_menu.rs），托盘线程只转发
                WM_RBUTTONUP => dispatch(crate::tray_menu::toggle),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_icon(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn run() {
    let module = match GetModuleHandleW(PCWSTR::null()) {
        Ok(module) => module,
        Err(error) => {
            warn!("托盘线程获取模块句柄失败: {error}");
            return;
        }
    };
    // 宽字符串缓冲必须绑定到局部变量保活：PCWSTR(to_wide(..).as_ptr())
    // 的临时 Vec 在语句结束即释放，CreateWindowExW 读到悬垂指针会间歇性
    // 报 ERROR_CANNOT_FIND_WND_CLASS (0x8007057F)
    let class_wide = to_wide("FloatPasteTrayWnd");
    let title_wide = to_wide("FloatPaste 托盘");
    let class_name = PCWSTR(class_wide.as_ptr());
    let wc = WNDCLASSW {
        lpfnWndProc: Some(tray_wndproc),
        hInstance: HINSTANCE(module.0),
        lpszClassName: class_name,
        ..Default::default()
    };
    if RegisterClassW(&wc) == 0 {
        warn!("注册托盘窗口类失败");
        return;
    }
    // 隐藏的普通顶层窗口（非 message-only）：只有它才能收到 TaskbarCreated
    let hwnd = match CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class_name,
        PCWSTR(title_wide.as_ptr()),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        None,
        None,
        Some(HINSTANCE(module.0)),
        None,
    ) {
        Ok(hwnd) => hwnd,
        Err(error) => {
            warn!("创建托盘窗口失败: {error}");
            return;
        }
    };

    add_icon(hwnd);

    let mut message = MSG::default();
    while GetMessageW(&mut message, None, 0, 0).as_bool() {
        let _ = DispatchMessageW(&message);
    }
    remove_icon(hwnd);
}
