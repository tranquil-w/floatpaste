//! 系统托盘：Shell_NotifyIcon 图标 + Win32 弹出菜单，独立线程消息循环。
//!
//! 对齐旧壳 tray_service.rs：
//! - 左键单击托盘 = 打开设置（show_menu_on_left_click(false) 语义）；
//! - 右键弹菜单：打开速贴面板 / 打开搜索 / 打开设置 / 暂停监听（或
//!   「恢复监听（当前已暂停）」）/ 退出，无分隔线；监听文案跟随当前
//!   设置——菜单每次右键现建，等价旧壳的 refresh_menu 推送；
//! - 图标按 DPI 取档（app_icon::tray_icon_hicon，同旧壳 tray_icon_image）；
//! - 监听 TaskbarCreated 广播，explorer 重启后自动重挂图标。

use std::sync::{Mutex, OnceLock};

use tracing::warn;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DispatchMessageW, DestroyMenu,
    GetMessageW, GetCursorPos, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, HICON, MF_STRING, MSG,
    TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_DESTROY,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

use crate::picker::App;

/// 托盘回调消息：wParam=图标 id，lParam=鼠标消息
const WM_APP_TRAY: u32 = WM_APP + 0x4E;
/// 菜单项 id（顺序对齐旧壳 MenuBuilder：速贴、搜索、设置、监听、退出）
const MENU_OPEN_PICKER: usize = 1;
const MENU_OPEN_SEARCH: usize = 2;
const MENU_OPEN_SETTINGS: usize = 3;
const MENU_TOGGLE_MONITORING: usize = 4;
const MENU_QUIT: usize = 5;

static TRAY_APP: OnceLock<Mutex<Option<App>>> = OnceLock::new();
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

/// 启动托盘线程（幂等：重复调用忽略）。线程持有 App 副本，菜单动作经
/// `slint::invoke_from_event_loop` 回事件循环执行。
pub fn start(app: App) {
    if TRAY_APP.set(Mutex::new(Some(app))).is_err() {
        return;
    }
    if let Err(error) = std::thread::Builder::new().name("tray".into()).spawn(|| unsafe { run() }) {
        warn!("托盘线程启动失败: {error}");
    }
}

/// 监听菜单文案（对齐旧壳 monitoring_menu_label）
fn monitoring_menu_label(paused: bool) -> &'static str {
    if paused {
        "恢复监听（当前已暂停）"
    } else {
        "暂停监听"
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
}

unsafe fn remove_icon(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
}

unsafe fn show_menu(hwnd: HWND) {
    let paused = TRAY_APP
        .get()
        .and_then(|lock| lock.lock().ok())
        .and_then(|guard| guard.as_ref().map(|app| app.state.current_settings().pause_monitoring))
        .unwrap_or(false);

    let Ok(menu) = CreatePopupMenu() else {
        warn!("创建托盘菜单失败");
        return;
    };
    let items: [(usize, &str); 5] = [
        (MENU_OPEN_PICKER, "打开速贴面板"),
        (MENU_OPEN_SEARCH, "打开搜索"),
        (MENU_OPEN_SETTINGS, "打开设置"),
        (MENU_TOGGLE_MONITORING, monitoring_menu_label(paused)),
        (MENU_QUIT, "退出"),
    ];
    for (id, text) in items {
        let wide = to_wide(text);
        if let Err(error) = AppendMenuW(menu, MF_STRING, id, PCWSTR(wide.as_ptr())) {
            warn!("托盘菜单项添加失败: {error}");
        }
    }

    // 标准托盘菜单三件套：先抢前台（否则点击菜单外不消失），TPM_RETURNCMD
    // 直接返回所选 id，结束后补 WM_NULL 让菜单正确收起
    let _ = SetForegroundWindow(hwnd);
    let mut point = POINT::default();
    let _ = GetCursorPos(&mut point);
    let chosen = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        point.x,
        point.y,
        None,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));

    match chosen.0 as usize {
        MENU_OPEN_PICKER => dispatch(|app| crate::picker::activate(app)),
        MENU_OPEN_SEARCH => dispatch(|app| crate::search::open_global(app)),
        MENU_OPEN_SETTINGS => dispatch(|app| crate::settings::open(app)),
        MENU_TOGGLE_MONITORING => dispatch(toggle_monitoring),
        MENU_QUIT => dispatch(quit),
        _ => {}
    }
}

/// 切换监听暂停（对齐旧壳 toggle-monitoring：翻转设置 + 应用运行时副作用，
/// 经 should_accept_monitoring_toggle 去抖过滤上游双触发）
fn toggle_monitoring(app: &App) {
    if !app.state.core.should_accept_monitoring_toggle() {
        return;
    }
    let mut settings = app.state.current_settings();
    settings.pause_monitoring = !settings.pause_monitoring;
    match app.state.core.update_settings(settings) {
        Ok(_) => {
            crate::settings::apply_side_effects(app);
        }
        Err(error) => warn!("托盘更新监听状态失败: {error}"),
    }
}

/// 退出：置退出标志并结束事件循环，main 在循环退出后统一收尾
/// （停钩子/监听/热键，对齐旧壳 prepare_for_exit + exit(0)）
fn quit(app: &App) {
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
        // explorer 重启后托盘图标丢失：重新挂载
        add_icon(hwnd);
        return LRESULT(0);
    }
    match msg {
        WM_APP_TRAY => {
            match lparam.0 as u32 {
                WM_LBUTTONUP => dispatch(|app| crate::settings::open(app)),
                WM_RBUTTONUP => show_menu(hwnd),
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
