use std::mem::size_of;
use std::path::Path;

use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, HWND},
        System::Threading::{
            AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
            PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::{
            Input::KeyboardAndMouse::SetFocus,
            WindowsAndMessaging::{
                GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindow,
                SetForegroundWindow, GUITHREADINFO,
            },
        },
    },
};

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowFocusTarget {
    pub window_hwnd: Option<isize>,
    pub focus_hwnd: Option<isize>,
}

pub struct ActiveAppResolver;

impl ActiveAppResolver {
    pub fn current_foreground_window_handle() -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            None
        } else {
            Some(hwnd.0 as isize)
        }
    }

    pub fn restore_foreground_window(hwnd: isize) -> bool {
        let hwnd = HWND(hwnd as *mut _);
        unsafe { IsWindow(Some(hwnd)).as_bool() && SetForegroundWindow(hwnd).as_bool() }
    }

    pub fn current_foreground_focus_target() -> WindowFocusTarget {
        let window_hwnd = Self::current_foreground_window_handle();
        let focus_hwnd = window_hwnd.and_then(Self::focus_handle_for_window);

        WindowFocusTarget {
            window_hwnd,
            focus_hwnd,
        }
    }

    pub fn focus_handle_for_window(hwnd: isize) -> Option<isize> {
        let hwnd = HWND(hwnd as *mut _);
        if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
            return None;
        }

        let thread_id = unsafe { GetWindowThreadProcessId(hwnd, None) };
        if thread_id == 0 {
            return None;
        }

        let mut gui_info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetGUIThreadInfo(thread_id, &mut gui_info) }.ok()?;

        if !gui_info.hwndFocus.0.is_null() {
            return Some(gui_info.hwndFocus.0 as isize);
        }

        if !gui_info.hwndCaret.0.is_null() {
            return Some(gui_info.hwndCaret.0 as isize);
        }

        None
    }

    pub fn restore_foreground_window_with_focus(
        window_hwnd: isize,
        focus_hwnd: Option<isize>,
    ) -> bool {
        if !Self::restore_foreground_window(window_hwnd) {
            return false;
        }

        let Some(focus_hwnd) = focus_hwnd else {
            return true;
        };

        let focus_hwnd = HWND(focus_hwnd as *mut _);
        if !unsafe { IsWindow(Some(focus_hwnd)).as_bool() } {
            return true;
        }

        let current_thread_id = unsafe { GetCurrentThreadId() };
        let target_thread_id = unsafe { GetWindowThreadProcessId(focus_hwnd, None) };
        let needs_attach = target_thread_id != 0 && target_thread_id != current_thread_id;

        if needs_attach {
            let _ = unsafe { AttachThreadInput(current_thread_id, target_thread_id, true) };
        }

        let _ = unsafe { SetFocus(Some(focus_hwnd)) };

        if needs_attach {
            let _ = unsafe { AttachThreadInput(current_thread_id, target_thread_id, false) };
        }

        true
    }

    pub fn current_foreground_process_name() -> Option<String> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return None;
        }

        let mut process_id = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        }

        if process_id == 0 {
            return None;
        }

        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()? };
        let mut buffer = vec![0u16; 260];
        let mut length = buffer.len() as u32;
        let query_result = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        };
        let _ = unsafe { CloseHandle(handle) };
        query_result.ok()?;

        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        Path::new(&path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
    }
}
