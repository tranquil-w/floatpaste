use tauri::WebviewWindow;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE,
};

pub fn show_window_no_activate(window: &WebviewWindow) -> Result<(), String> {
    let tauri_hwnd = window.hwnd().map_err(|e| e.to_string())?;
    // tauri_hwnd is either `isize` directly or a tuple struct `HWND(isize)`.
    let hwnd_isize = tauri_hwnd.0 as isize;
    let hwnd = HWND(hwnd_isize as *mut _);

    unsafe {
        // 第一步：在调用显示之前，确保窗口带有 WS_EX_NOACTIVATE 属性
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_NOACTIVATE.0 as isize);
    }

    // 第二步：必须调用 Tauri 的 show() 以驱动 WebView2 渲染管线，避免界面只出现边框并卡死
    window.show().map_err(|e| e.to_string())?;

    // 第三步：以不激活的方式刷新窗口位置与显示状态
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
        );
    }

    Ok(())
}
