use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tracing::{debug, error};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetWindowRect, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCLBUTTONDOWN, WM_NCMBUTTONDOWN,
    WM_NCRBUTTONDOWN, WM_RBUTTONDOWN,
};

use std::sync::atomic::{AtomicIsize, Ordering};

static APP_HANDLE: Mutex<Option<AppHandle>> = Mutex::new(None);

static MOUSE_HOOK: AtomicIsize = AtomicIsize::new(0);

pub struct PickerMouseMonitor;

impl PickerMouseMonitor {
    pub fn begin_session(app: AppHandle) {
        let mut handle = APP_HANDLE.lock().unwrap();
        *handle = Some(app);

        unsafe {
            if MOUSE_HOOK.load(Ordering::SeqCst) == 0 {
                let h = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0);
                match h {
                    Ok(hook_handle) => {
                        MOUSE_HOOK.store(hook_handle.0 as isize, Ordering::SeqCst);
                    }
                    Err(e) => {
                        error!("设置 WH_MOUSE_LL 钩子失败: {}", e);
                    }
                }
            }
        }
    }

    pub fn end_session() {
        let h = MOUSE_HOOK.swap(0, Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(h as *mut core::ffi::c_void));
            }
        }
        let mut handle = APP_HANDLE.lock().unwrap();
        *handle = None;
    }
}

extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_LBUTTONDOWN
            || msg == WM_RBUTTONDOWN
            || msg == WM_MBUTTONDOWN
            || msg == WM_NCLBUTTONDOWN
            || msg == WM_NCRBUTTONDOWN
            || msg == WM_NCMBUTTONDOWN
        {
            let close_picker = {
                let app_lock = APP_HANDLE.lock().unwrap();
                if let Some(app) = app_lock.as_ref() {
                    if let Some(window) = app.get_webview_window("picker") {
                        if window.is_visible().unwrap_or(false) {
                            if let Ok(hwnd_obj) = window.hwnd() {
                                // in windows 0.61, window.hwnd() usually returns an HWND
                                // or tauri returns a raw pointer which we cast.
                                // Actually tauri's hwnd() returns std::ffi::c_void pointer.
                                // wait, does it? tauri::WebviewWindow::hwnd() returns Result<HWND, tauri::Error>
                                // Let's check window_utils.rs we saw earlier:
                                // let tauri_hwnd = window.hwnd().unwrap();
                                // let hwnd_isize = tauri_hwnd.0 as isize;
                                // let hwnd = HWND(hwnd_isize as *mut _);
                                let hwnd_isize = hwnd_obj.0 as isize;
                                let hwnd = HWND(hwnd_isize as *mut _);

                                let mut rect = RECT::default();
                                unsafe {
                                    if GetWindowRect(hwnd, &mut rect).is_ok() {
                                        let hook_struct = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                                        let pt: &POINT = &hook_struct.pt;
                                        if pt.x < rect.left
                                            || pt.x > rect.right
                                            || pt.y < rect.top
                                            || pt.y > rect.bottom
                                        {
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if close_picker {
                let app = {
                    let lock = APP_HANDLE.lock().unwrap();
                    lock.as_ref().cloned()
                };
                if let Some(app_clone) = app {
                    debug!("点击了速贴窗口外部，准备隐藏窗口");
                    std::thread::spawn(move || {
                        if let Some(state) = app_clone.try_state::<crate::app_bootstrap::AppState>()
                        {
                            let _ = crate::services::window_coordinator::WindowCoordinator::hide_picker_and_restore_target(
                                &app_clone, &state,
                            );
                        }
                    });
                }
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
