use tauri::WebviewWindow;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::SetActiveWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetWindowLongPtrW, HWND_TOPMOST, IsIconic, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
    SW_SHOWNOACTIVATE, WM_SYSCOMMAND, WS_EX_NOACTIVATE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
    WS_SYSMENU, SC_KEYMENU,
};

fn strip_system_menu_style(style: isize) -> isize {
    style
        & !(WS_SYSMENU.0 as isize)
        & !(WS_MINIMIZEBOX.0 as isize)
        & !(WS_MAXIMIZEBOX.0 as isize)
}

const ALT_MENU_BLOCKER_SUBCLASS_ID: usize = 0x4650_0101;

fn is_alt_menu_syscommand(wparam: usize) -> bool {
    (wparam & 0xFFF0) == SC_KEYMENU as usize
}

pub fn remove_window_system_menu(window: &WebviewWindow) -> Result<(), String> {
    let tauri_hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd_isize = tauri_hwnd.0 as isize;
    let hwnd = HWND(hwnd_isize as *mut _);

    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, strip_system_menu_style(style));
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }

    Ok(())
}

pub fn block_alt_menu_activation(window: &WebviewWindow) -> Result<(), String> {
    let tauri_hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd_isize = tauri_hwnd.0 as isize;
    let hwnd = HWND(hwnd_isize as *mut _);

    unsafe {
        let _ = SetWindowSubclass(
            hwnd,
            Some(alt_menu_blocker_subclass_proc),
            ALT_MENU_BLOCKER_SUBCLASS_ID,
            0,
        );
    }

    Ok(())
}

pub fn show_window_no_activate(window: &WebviewWindow) -> Result<(), String> {
    let tauri_hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd_isize = tauri_hwnd.0 as isize;
    let hwnd = HWND(hwnd_isize as *mut _);

    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_NOACTIVATE.0 as isize);
    }

    window.show().map_err(|e| e.to_string())?;

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

pub fn is_window_minimized(window: &WebviewWindow) -> Result<bool, String> {
    let tauri_hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd_isize = tauri_hwnd.0 as isize;
    let hwnd = HWND(hwnd_isize as *mut _);

    Ok(unsafe { IsIconic(hwnd).as_bool() })
}

pub fn restore_window_and_focus(window: &WebviewWindow) -> Result<(), String> {
    let tauri_hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd_isize = tauri_hwnd.0 as isize;
    let hwnd = HWND(hwnd_isize as *mut _);

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = BringWindowToTop(hwnd);

        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let _ = SetActiveWindow(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }

    Ok(())
}

pub fn apply_picker_window_shape(_window: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

unsafe extern "system" fn alt_menu_blocker_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if msg == WM_SYSCOMMAND && is_alt_menu_syscommand(wparam.0) {
        return LRESULT(0);
    }

    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::{is_alt_menu_syscommand, strip_system_menu_style};
    use windows::Win32::UI::WindowsAndMessaging::{
        SC_CLOSE, SC_KEYMENU, WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
    };

    #[test]
    fn strip_system_menu_style_removes_system_menu_related_flags() {
        let style = (WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0) as isize;

        let stripped = strip_system_menu_style(style);

        assert_ne!(stripped & WS_CAPTION.0 as isize, 0);
        assert_eq!(stripped & WS_SYSMENU.0 as isize, 0);
        assert_eq!(stripped & WS_MINIMIZEBOX.0 as isize, 0);
        assert_eq!(stripped & WS_MAXIMIZEBOX.0 as isize, 0);
    }

    #[test]
    fn alt_menu_syscommand_detection_only_matches_keymenu() {
        assert!(is_alt_menu_syscommand(SC_KEYMENU as usize));
        assert!(is_alt_menu_syscommand((SC_KEYMENU as usize) | 0x0001));
        assert!(!is_alt_menu_syscommand(SC_CLOSE as usize));
    }
}
