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
                BringWindowToTop, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo,
                GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, SetWindowPos,
                ShowWindow, GUITHREADINFO, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE,
                SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
            },
        },
    },
};

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowFocusTarget {
    pub window_hwnd: Option<isize>,
    pub focus_hwnd: Option<isize>,
}

/// 桌面窗口判定：Progman（桌面图标宿主，普通桌面态的前台）与
/// WorkerW（壁纸层，Win+D 等场景下的前台桌面窗口）
pub fn is_desktop_window(hwnd: isize) -> bool {
    let hwnd = HWND(hwnd as *mut _);
    let mut buf = [0u16; 32];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    matches!(
        String::from_utf16_lossy(&buf[..len as usize]).as_str(),
        "Progman" | "WorkerW"
    )
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
        // 前台是桌面（无应用聚焦态，Progman 承载图标层、WorkerW 是壁纸层）
        // 时无需归还——速贴/搜索窗口无激活显示并未改变前台；而
        // SetForegroundWindow(Progman) 在桌面空闲态实测阻塞 1.3s
        // （explorer 桌面线程慢路径，2026-09-25 真机日志定位），跳过
        if is_desktop_window(hwnd) {
            return true;
        }
        let hwnd = HWND(hwnd as *mut _);
        unsafe { IsWindow(Some(hwnd)).as_bool() && SetForegroundWindow(hwnd).as_bool() }
    }

    /// 把窗口带到前台并激活（编辑器打开路径）。
    ///
    /// 裸 SetForegroundWindow 受 Windows 前台锁约束：调用进程不是前台
    /// 进程时调用被拒绝，窗口留在原 Z 序位被当前前台窗口遮挡（从速贴
    /// 面板打开编辑器时前台在目标应用上，必踩）。绕过方式对齐旧壳 tao
    /// set_focus：先 AttachThreadInput 到当前前台线程共享输入状态，
    /// SetForegroundWindow 即被放行；仍失败时以 TOPMOST 提升→立即回落
    /// 顶到 Z 序带顶，保证可见且不常驻置顶
    pub fn force_foreground_window(hwnd: isize) -> bool {
        let hwnd = HWND(hwnd as *mut _);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return false;
        }
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let foreground = GetForegroundWindow();
            let foreground_thread = if foreground.0.is_null() {
                0
            } else {
                GetWindowThreadProcessId(foreground, None)
            };
            let current_thread = GetCurrentThreadId();
            let attached = foreground_thread != 0
                && foreground_thread != current_thread
                && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
            // BringWindowToTop 必须显式调用：实测 AttachThreadInput 路径下
            // SetForegroundWindow 激活成功不代表窗口 Z 序升起，视觉上仍会
            // 被原前台窗口遮挡
            let _ = BringWindowToTop(hwnd);
            let foregrounded = SetForegroundWindow(hwnd).as_bool();
            let _ = SetFocus(Some(hwnd));
            if attached {
                let _ = AttachThreadInput(current_thread, foreground_thread, false);
            }
            if foregrounded && GetForegroundWindow().0 == hwnd.0 {
                return true;
            }
            // 前台锁兜底：TOPMOST 提升越过所有普通窗口，随即回落普通带
            // 顶部（不常驻置顶）；提升不带 NOACTIVATE，顺带完成激活
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_NOTOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            GetForegroundWindow().0 == hwnd.0
        }
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

    /// 当前前台窗口句柄；无前台窗口时返回 None。
    pub fn current_foreground_hwnd() -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.0.is_null()).then(|| hwnd.0 as isize)
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
