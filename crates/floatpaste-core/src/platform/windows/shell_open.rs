//! 用系统默认方式打开路径（编辑窗口预览辅助）。
//!
//! ShellExecuteW "open"：文件按扩展名关联的程序打开（图片走系统照片
//! 查看器等），目录走资源管理器；递交系统后立即返回，不等待进程退出。

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IServiceProvider, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IFolderView, ILCreateFromPathW, ILFindLastID, ILFree, ILIsEqual, IPersistFolder2,
    IShellBrowser, IShellView, IShellWindows, IWebBrowserApp, SID_STopLevelBrowser, ShellExecuteW,
    ShellWindows, SVSI_DESELECTOTHERS, SVSI_ENSUREVISIBLE, SVSI_FOCUSED, SVSI_SELECT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOWNORMAL,
};

use crate::domain::error::AppError;

use super::wide_string::to_wide;

/// ShellExecuteW "open" 递交系统启动目标，>32 表示已受理启动（约定值，
/// 不代表目标程序最终启动成功）。前置合法性由各公开入口自行检查
fn shell_execute_open(target: &str, parameters: Option<&str>) -> Result<(), AppError> {
    unsafe {
        let verb = to_wide("open");
        let file = to_wide(target);
        let wide_parameters = parameters.map(to_wide);
        let instance = ShellExecuteW(
            None,
            PCWSTR::from_raw(verb.as_ptr()),
            PCWSTR::from_raw(file.as_ptr()),
            wide_parameters
                .as_deref()
                .map_or(PCWSTR::null(), |p| PCWSTR::from_raw(p.as_ptr())),
            None,
            SW_SHOWNORMAL,
        );
        if (instance.0 as usize) <= 32 {
            return Err(AppError::Message(format!(
                "系统拒绝打开请求（代码 {code}）",
                code = instance.0 as usize
            )));
        }
    }
    Ok(())
}

/// 用系统默认浏览器打开 URL。URL 不是文件系统路径，不做存在性检查。
pub fn open_url(url: &str) -> Result<(), AppError> {
    if url.is_empty() {
        return Err(AppError::Message("URL 为空".into()));
    }
    shell_execute_open(url, None)
}

/// 用系统默认程序打开路径。返回 >32 表示系统已受理启动（ShellExecuteW
/// 约定），但不代表目标程序最终启动成功。
pub fn open_path(path: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Err(AppError::Message("路径为空".into()));
    }
    if !std::path::Path::new(path).exists() {
        return Err(AppError::Message(format!("路径不存在：{path}")));
    }
    shell_execute_open(path, None)
}

/// 在资源管理器中定位目标：打开其所在目录并选中目标（文件与目录同样
/// 适用）。优先复用已经显示该目录的资源管理器窗口（在其中选中并前置，
/// 见 [`reuse_open_explorer_window`]），没有可复用窗口时新开；不存在
/// 的路径必须先拒绝——explorer 对失效目标会静默回退到默认视图，调用
/// 方无从得知失败。
pub fn reveal_in_explorer(path: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Err(AppError::Message("路径为空".into()));
    }
    let target = std::path::Path::new(path);
    if !target.exists() {
        return Err(AppError::Message(format!("路径不存在：{path}")));
    }
    // 根目录（盘符、UNC share 根）没有所在文件夹，也没有可复用的父
    // 视图：直接打开目录本身；也规避 `/select,"C:\"` 的尾随反斜杠把
    // 内嵌引号转义成字面引号、explorer 静默回退默认视图的问题
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let Some(parent) = parent else {
        return shell_execute_open(path, None);
    };
    if unsafe { reuse_open_explorer_window(target, parent) } {
        return Ok(());
    }
    // explorer 自行解析命令行参数，路径带空格时内嵌引号是标准形式
    shell_execute_open("explorer.exe", Some(&format!("/select,\"{path}\"")))
}

/// 复用已打开的资源管理器：枚举 Shell 窗口，找到当前正显示目标父目录
/// 的资源管理器窗口时，在其中选中目标并前置，返回 true。
///
/// 枚举或任一窗口查询失败静默跳过该窗口，全路径失败返回 false 交由
/// 调用方新开窗口——复用是优化路径，不承诺找到。
///
/// # Safety
/// 调用方须保证线程可安全执行 COM STA 调用。
unsafe fn reuse_open_explorer_window(target: &std::path::Path, parent: &std::path::Path) -> bool {
    unsafe {
        // 已初始化为其他模型（S_FALSE / RPC_E_CHANGED_MODE）均不影响使用
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let wide_target = to_wide(&target.to_string_lossy());
        let wide_parent = to_wide(&parent.to_string_lossy());
        let target_pidl = ILCreateFromPathW(PCWSTR::from_raw(wide_target.as_ptr()));
        let parent_pidl = ILCreateFromPathW(PCWSTR::from_raw(wide_parent.as_ptr()));
        if target_pidl.is_null() || parent_pidl.is_null() {
            free_pidl(target_pidl);
            free_pidl(parent_pidl);
            return false;
        }
        let reused = enumerate_and_reuse(target_pidl, parent_pidl);
        free_pidl(target_pidl);
        free_pidl(parent_pidl);
        reused
    }
}

/// ILCreateFromPathW 分配的 PIDL 按 Shell 契约用 ILFree 释放（与
/// CoTaskMemFree 等价，逐条注明配对来源）；null 容忍成对释放的拼参
unsafe fn free_pidl(pidl: *mut ITEMIDLIST) {
    unsafe {
        if !pidl.is_null() {
            ILFree(Some(pidl));
        }
    }
}

/// 遍历 Shell 窗口找活动视图显示 `parent_pidl` 的资源管理器窗口。
/// COM 接口指针（IShellWindows 等）由 windows crate RAII 释放。
unsafe fn enumerate_and_reuse(target_pidl: *mut ITEMIDLIST, parent_pidl: *mut ITEMIDLIST) -> bool {
    unsafe {
        let shell_windows: IShellWindows = match CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) {
            Ok(windows) => windows,
            Err(_) => return false,
        };
        let Ok(count) = shell_windows.Count() else {
            return false;
        };
        // 目标相对 PIDL 是绝对 PIDL 的最后一段；ILFindLastID 不拥有
        // 内存，随 target_pidl 一同释放
        let child = ILFindLastID(target_pidl);
        let flags =
            (SVSI_SELECT.0 | SVSI_DESELECTOTHERS.0 | SVSI_ENSUREVISIBLE.0 | SVSI_FOCUSED.0) as u32;
        for index in 0..count {
            if let Some((hwnd, view)) = explorer_window_showing(&shell_windows, index, parent_pidl)
            {
                if view.SelectItem(child, flags).is_ok() {
                    bring_to_front(hwnd);
                    return true;
                }
            }
        }
        false
    }
}

/// 读第 `index` 个 Shell 窗口：是资源管理器主窗口且活动视图正显示
/// `parent_pidl` 时返回其 HWND 与活动视图（用于选中目标项）。
unsafe fn explorer_window_showing(
    shell_windows: &IShellWindows,
    index: i32,
    parent_pidl: *mut ITEMIDLIST,
) -> Option<(HWND, IShellView)> {
    unsafe {
        let dispatch = shell_windows.Item(&VARIANT::from(index)).ok()?;
        let browser: IWebBrowserApp = dispatch.cast().ok()?;
        let hwnd = HWND(browser.HWND().ok()?.0 as *mut core::ffi::c_void);
        if hwnd.is_invalid() || !is_explorer_frame(hwnd) {
            return None;
        }
        let provider: IServiceProvider = browser.cast().ok()?;
        let shell_browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser).ok()?;
        let view: IShellView = shell_browser.QueryActiveShellView().ok()?;
        let folder_view: IFolderView = view.cast().ok()?;
        let persist: IPersistFolder2 = folder_view.GetFolder().ok()?;
        let current = persist.GetCurFolder().ok()?;
        let showing = ILIsEqual(current, parent_pidl).as_bool();
        // GetCurFolder 的产物按其文档约定用 CoTaskMemFree（与 ILFree
        // 等价，但配对契约分开注明）
        CoTaskMemFree(Some(current.cast()));
        showing.then_some((hwnd, view))
    }
}

/// 选中已就位后前置窗口：最小化先还原；前台锁定（进程未持有前台权）
/// 只闪任务栏，不视为复用失败。
unsafe fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// 只认资源管理器主窗口（CabinetWClass）：IShellWindows 还包含桌面等
/// 其他 Shell 宿主，它们的视图不是用户理解的「已打开的文件夹窗口」
unsafe fn is_explorer_frame(hwnd: HWND) -> bool {
    unsafe {
        let cabinet_class = windows::core::w!("CabinetWClass");
        let mut buffer = [0u16; 32];
        let len = GetClassNameW(hwnd, &mut buffer);
        buffer[..len as usize] == *cabinet_class.as_wide()
    }
}

#[cfg(test)]
mod tests {
    use super::reveal_in_explorer;

    // 只测拒绝分支：成功分支会真开资源管理器窗口，属桌面验收范围
    #[test]
    fn reveal_rejects_empty_and_missing_paths_without_dispatch() {
        assert!(reveal_in_explorer("").is_err());
        assert!(reveal_in_explorer(r"Z:\floatpaste-不存在的目录\x.txt").is_err());
    }

    // 以下为桌面验收（真开资源管理器窗口），交互桌面手动执行：
    // `cargo test -p floatpaste-core reveal_ -- --ignored`
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

    /// 收集资源管理器主窗口 HWND（经 lparam 传出，避免全局计数口径混乱）
    extern "system" fn collect_explorer_frames(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let frames = &mut *(lparam.0 as *mut Vec<isize>);
            if super::is_explorer_frame(hwnd) {
                frames.push(hwnd.0 as isize);
            }
            true.into()
        }
    }

    fn explorer_frames() -> Vec<isize> {
        let mut frames = Vec::new();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
                Some(collect_explorer_frames),
                LPARAM(&mut frames as *mut _ as isize),
            );
        }
        frames
    }

    /// 只关闭 `before` 之后新出现的窗口（本测试自己开的），不触碰
    /// 用户已有窗口
    fn close_new_explorer_windows(before: &[isize]) {
        for hwnd in explorer_frames() {
            if before.contains(&hwnd) {
                continue;
            }
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(HWND(hwnd as *mut core::ffi::c_void)),
                    windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                    WPARAM::default(),
                    LPARAM::default(),
                );
            }
        }
    }

    #[test]
    #[ignore = "桌面验收：真开资源管理器窗口并切换前台，交互桌面手动执行"]
    fn reveal_reuses_open_window_and_falls_back_to_new() {
        use std::process::Command;
        // 独占临时目录做目标：验收前桌面不会有显示它的窗口，复用/新开
        // 两段的前置条件不受本次会话之外的遗留窗口干扰
        let stamp = std::process::id();
        let dir = std::env::temp_dir().join(format!("floatpaste-reveal-accept-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("目标 文件.txt"); // 文件名带空格，顺带覆盖引号形态
        std::fs::write(&target, "reveal 验收").unwrap();
        let target = target.to_string_lossy().to_string();
        let parent = dir.to_string_lossy().to_string();

        // 回退路径：没有已打开的父目录窗口时新开并选中
        let before = explorer_frames();
        reveal_in_explorer(&target).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert_eq!(
            explorer_frames().len(),
            before.len() + 1,
            "无可复用窗口时应新开一个资源管理器"
        );
        close_new_explorer_windows(&before);
        std::thread::sleep(std::time::Duration::from_secs(1));

        // 复用路径：先打开父目录，再定位目标不应新开窗口
        let before = explorer_frames();
        Command::new("explorer.exe").arg(&parent).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert_eq!(
            explorer_frames().len(),
            before.len() + 1,
            "前置：父目录已打开"
        );
        reveal_in_explorer(&target).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert_eq!(
            explorer_frames().len(),
            before.len() + 1,
            "已打开父目录的窗口应被复用，不得新开"
        );
        close_new_explorer_windows(&before);
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::fs::remove_dir_all(&dir).expect("验收窗口已关闭，临时目录应可删除");
    }
}
