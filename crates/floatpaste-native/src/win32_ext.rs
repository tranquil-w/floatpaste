//! Slint 窗口的 Win32 侧装配：HWND 提取、无边框置顶工具窗样式、
//! DWM 悬浮阴影与系统圆角。

use floatpaste_core::platform::windows::window_control;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{ClientToScreen, InvalidateRect, UpdateWindow};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, GWL_EXSTYLE, GWL_STYLE, SIZE_RESTORED,
    WM_ACTIVATE, WM_SHOWWINDOW, WM_SIZE, WM_WINDOWPOSCHANGED, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
    WS_SYSMENU,
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

/// 强制下一帧全量重绘：软渲染按 damage 矩形局部呈现（present 逐矩形
/// BitBlt），「仅颜色变化」的帧依赖局部渲染器的脏区跟踪覆盖整窗，该
/// 跟踪在停屏/恢复时序下不可靠（浅↔深主题切换实测速贴/搜索窗残留
/// 旧主题像素）。真实往返一次窗口高度，迫使 softbuffer 重分配缓冲、
/// 渲染器转入 NewBuffer 全量渲染，不再依赖脏区。回位延迟 50ms 留出
/// 一帧渲染间隔；期间窗口可能已上屏，±1px 不可感
pub fn force_full_repaint(hwnd: isize) {
    let Some(rect) = physical_rect(hwnd) else {
        return;
    };
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return;
    }
    window_control::set_window_size_no_activate(hwnd, width, height + 1);
    warm_surface(hwnd);
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        window_control::set_window_size_no_activate(hwnd, width, height);
        warm_surface(hwnd);
    });
}

/// 系统是否开启「透明效果」（设置 > 个性化 > 颜色）。注册表值缺失
/// 或读取失败（异常环境）按开启处理，不因检测问题阻断材质
pub fn system_transparency_enabled() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

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
/// 合成机制（2026-09-26 实测）：挂 backdrop 后表面像素 alpha 参与合成，
/// 材质透过率 = 1 - 底色 alpha。UI 侧分两层：前景脚下的内容面用
/// theme.rs `material_layer`（深 0.90/浅 0.62），材质感留给
/// 窗缘的 `material_base`（深 0.85/浅 0.45）；勿改用透明底直露
/// （100% 材质，用户实测否决：亮背景下深色 UI 对比全乱）。浅色 Acrylic
/// 系统 tint 白雾重，属系统形态（常驻窗浅色用 Mica，不受此限）。
///
/// **无焦点窗口（速贴）勿走本函数**：SystemBackdrop Acrylic 在窗口
/// 非前台时自动降级为纯色（系统行为，无开关），速贴须用
/// [`apply_window_host_backdrop_acrylic`]。
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
    let backdrop = if transient {
        DWMSBT_TRANSIENTWINDOW
    } else {
        DWMSBT_MAINWINDOW
    };
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


/// 无焦点窗口的系统 Acrylic：SWCA `ACCENT_STATE_ENABLE_HOSTBACKDROP`
/// （Windows 11 新增的 accent 状态，未文档化，由 Windhawk Translucent
/// Windows mod 逆向证实）声明「窗口表面之下由 DWM 渲染 host backdrop」，
/// 配合 `DWMWA_SYSTEMBACKDROP_TYPE`（Acrylic）即可在**非前台窗口**上
/// 获得与前台一致的系统材质——SystemBackdrop 单独使用时非前台自动降级
/// 纯色（系统行为），HOSTBACKDROP 组合绕过该降级。
///
/// 材质态面板底用 material-base 半透明色（与搜索窗同构，系统 Acrylic
/// 自带 tint 与噪点，无需应用侧补）。已知取舍：API 未文档化，未来
/// Windows 版本存在变更风险（SetWindowCompositionAttribute 失败即
/// 返回 false 走 canvas 回退）。
pub fn apply_window_host_backdrop_acrylic(hwnd: isize, prefers_dark: bool) -> bool {
    #[repr(C)]
    struct AccentPolicy {
        accent_state: i32,
        accent_flags: i32,
        gradient_color: u32,
        animation_id: i32,
    }
    #[repr(C)]
    struct WindowCompositionAttribData {
        attrib: u32,
        data: *mut AccentPolicy,
        size_of_data: usize,
    }
    type SetWindowCompositionAttributeFn =
        unsafe extern "system" fn(HWND, *const WindowCompositionAttribData) -> i32;

    if !system_transparency_enabled() {
        return false;
    }
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        // user32.dll 未文档化导出，SDK user32.lib 无符号 → 运行时动态
        // 加载；符号缺失（未来系统移除）即返回 false 走 canvas 回退
        use windows::core::{w, PCSTR};
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let set_window_composition_attribute =
            LoadLibraryW(w!("user32.dll")).ok().and_then(|module| {
                std::mem::transmute::<_, Option<SetWindowCompositionAttributeFn>>(
                    GetProcAddress(module, PCSTR(b"SetWindowCompositionAttribute\0".as_ptr())),
                )
            });
        let Some(set_window_composition_attribute) = set_window_composition_attribute else {
            return false;
        };
        let dark = u32::from(prefers_dark);
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
        let acrylic = 3; // DWMSBT_TRANSIENTWINDOW
        if DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &acrylic as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        )
        .is_err()
        {
            return false;
        }
        // ACCENT_STATE_ENABLE_HOSTBACKDROP(5):DWM 在窗口表面之下渲染
        // SystemBackdrop 材质,不随焦点降级
        let mut policy = AccentPolicy {
            accent_state: 5,
            accent_flags: 0,
            gradient_color: 0,
            animation_id: 0,
        };
        let data = WindowCompositionAttribData {
            attrib: 19, // WCA_ACCENT_POLICY
            data: &mut policy as *mut AccentPolicy,
            size_of_data: std::mem::size_of::<AccentPolicy>(),
        };
        let applied = set_window_composition_attribute(hwnd, &data) != 0;
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        applied
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

/// 内容窗（编辑/设置）的 TOOLWINDOW 样式开关。停屏期 tool=true：任务栏
/// 不出现按钮；上屏前 tool=false：恢复普通 APPWINDOW 外观（任务栏按钮
/// 随显示出现）。样式切换在屏外完成，无可见跳变
pub fn set_toolwindow_style(hwnd: isize, tool: bool) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = if tool {
            ex_style & !(WS_EX_APPWINDOW.0 as isize) | WS_EX_TOOLWINDOW.0 as isize
        } else {
            ex_style & !WS_EX_TOOLWINDOW.0 as isize | WS_EX_APPWINDOW.0 as isize
        };
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

/* ───────────────── 最小化恢复的材质重挂 ───────────────── */

/// DWM 对最小化恢复的窗口偶发不重新应用 SystemBackdrop（用户实测：
/// 编辑窗最小化再恢复后整窗失去材质），需要显示流程重挂。这里用窗口
/// 子类拦截 WM_SIZE(SIZE_RESTORED)，交由注册的闭包在事件循环执行
static RESTORE_HOOK: std::sync::Mutex<Option<Box<dyn Fn(isize) + Send>>> =
    std::sync::Mutex::new(None);

const MATERIAL_RESTORE_SUBCLASS_ID: usize = 0x4D52_4D31; // 'MRM1'

/// 给内容窗安装「最小化恢复 → 重挂材质」子类。回调收到 hwnd，由调用方
/// 闭包决定如何重挂（需在事件循环线程做 Slint 属性写入）。
/// 约束：RESTORE_HOOK 是单槽，全部窗口必须共用同一个按 hwnd 路由的
/// 闭包——后装的覆盖前装，分窗口回调需先扩为 per-hwnd 槽
pub fn install_material_restore_subclass(hwnd: isize, on_restore: impl Fn(isize) + Send + 'static) {
    if RESTORE_HOOK
        .lock()
        .map(|mut slot| slot.replace(Box::new(on_restore)))
        .is_err()
    {
        return;
    }
    unsafe {
        let _ = SetWindowSubclass(
            HWND(hwnd as *mut _),
            Some(material_restore_subclass_proc),
            MATERIAL_RESTORE_SUBCLASS_ID,
            0,
        );
    }
}

unsafe extern "system" fn material_restore_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if msg == WM_SIZE && wparam.0 as u32 == SIZE_RESTORED {
        let hwnd_addr = hwnd.0 as isize;
        // 在子类线程直接调用：重挂 DWM 属性线程安全，Slint 属性写入
        // 由实现方经 invoke_from_event_loop 转投
        if let Ok(slot) = RESTORE_HOOK.lock() {
            if let Some(f) = slot.as_ref() {
                f(hwnd_addr);
            }
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/* ───────────────── 系统窗控按钮残影抑制 ───────────────── */

const CAPTION_STRIP_SUBCLASS_ID: usize = 0x4D52_4D32; // 'MRM2'

/// 自绘标题栏窗（编辑/设置/搜索）的「系统窗控按钮重影」抑制。
///
/// 这些窗口是 tao 无边框阴影窗：保留 WS_CAPTION/WS_THICKFRAME，且
/// winit 会在显示前后异步带回 WS_SYSMENU/最小化/最大化样式位。DWM
/// 据此在 backdrop 层按窗口创建时的度量绘制原生窗控按钮，透过半透明
/// 自绘标题栏显现为灰色重影（自绘按钮仿了原生 46px 间距，两套字形
/// 几乎重叠、错位数像素——用户看到的是「标题栏图标有重影」）。真
/// WS_POPUP 浮层（速贴/tooltip）无 CAPTION，DWM 不画按钮，无此问题。
///
/// 剥掉 SYSMENU/最小化/最大化样式位后 DWM 不再绘制按钮（无边框阴影
/// 与系统材质不依赖这些位）。winit 带回的时机不定，定时器补剥靠猜，
/// 这里装子类在显示/激活/几何消息上幂等补剥：样式位未置时零操作。
/// 自绘最小化/最大化按钮走 ShowWindow，不依赖这些样式位
pub fn install_caption_strip_subclass(hwnd: isize) {
    unsafe {
        let _ = SetWindowSubclass(
            HWND(hwnd as *mut _),
            Some(caption_strip_subclass_proc),
            CAPTION_STRIP_SUBCLASS_ID,
            0,
        );
    }
}

unsafe extern "system" fn caption_strip_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if matches!(
        msg,
        WM_SHOWWINDOW | WM_ACTIVATE | WM_SIZE | WM_WINDOWPOSCHANGED
    ) {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if style & (WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0) as isize != 0 {
            let _ = window_control::remove_window_system_menu(hwnd.0 as isize);
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/* ───────────────── 点击穿透浮层的样式守护 ───────────────── */

const CLICK_THROUGH_SUBCLASS_ID: usize = 0x4D52_4D33; // 'MRM3'

/// tooltip 等点击穿透浮层的穿透位守护。winit 显示后的异步样式重排时机
/// 不定（可能晚于 after_show 的 50ms 兜底），重排会剥掉 TRANSPARENT/
/// LAYERED 位——此后 tooltip 变成可点击窗口，盖在面板条目上吞掉点击
/// （悬停预览弹出后点铅笔无响应的根因）。与标题栏剥样式同一武器：子类
/// 在显示/几何消息上幂等补挂，穿透位齐备时零操作
pub fn install_click_through_subclass(hwnd: isize) {
    unsafe {
        let _ = SetWindowSubclass(
            HWND(hwnd as *mut _),
            Some(click_through_subclass_proc),
            CLICK_THROUGH_SUBCLASS_ID,
            0,
        );
    }
}

unsafe extern "system" fn click_through_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    if matches!(
        msg,
        WM_SHOWWINDOW | WM_ACTIVATE | WM_SIZE | WM_WINDOWPOSCHANGED
    ) {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let required = (WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0) as isize;
        if ex_style & required != required {
            // 补挂含 SetLayeredWindowAttributes(alpha=255)：LAYERED 位
            // 加了但未设属性时窗口不渲染，两者必须同时到位
            let _ = window_control::set_window_click_through(hwnd.0 as isize);
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
