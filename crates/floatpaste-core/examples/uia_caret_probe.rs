//! UIA 插入符探针：诊断「某应用上速贴定位不对」的常驻工具，不随用随删。
//!
//! 用法（在仓库根）：
//!   cargo run -p floatpaste-core --example uia_caret_probe -- dump <进程名>
//!
//! dump 按生产链路（`uia_caret.rs`）的取值顺序输出：目标进程各顶层窗口 →
//! 窗口元素 → 后代里的文本元素（标记持焦点者）→ 每个文本元素的插入符范围/
//! 选区范围的折叠性与各粒度展开几何。对照期望的光标位置，即可判断问题落在
//! 哪个桶（范围语义 / 大框 / 无文本树）。
//!
//! 注意：`GetCaretRange` 读的是**当时**的插入符——跑探针前把目标应用的光标
//! 放到要诊断的位置；目标不是前台窗口时部分 provider 会拒绝或给出旧值，
//! dump 会如实输出。

use std::mem::size_of;

use windows::core::{Interface, BOOL};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest,
    IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextPattern2,
    IUIAutomationTextRange, TextPatternRangeEndpoint, TextPatternRangeEndpoint_End,
    TextPatternRangeEndpoint_Start, TextUnit, TextUnit_Character, TextUnit_Line,
    TextUnit_Paragraph, TextUnit_Word, TreeScope_Descendants, UIA_BoundingRectanglePropertyId,
    UIA_ClassNamePropertyId, UIA_HasKeyboardFocusPropertyId, UIA_IsTextPatternAvailablePropertyId,
    UIA_NamePropertyId, UIA_TextPattern2Id, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

/// 每个窗口最多 dump 的文本元素数——Chromium 全树可达数千，诊断用不到
const MAX_TEXT_ELEMENTS: i32 = 20;

const EXPAND_UNITS: [(&str, TextUnit); 4] = [
    ("字符", TextUnit_Character),
    ("词", TextUnit_Word),
    ("行", TextUnit_Line),
    ("段", TextUnit_Paragraph),
];

fn main() {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_default();
    let Some(command) = args.next() else {
        eprintln!("用法: {program} dump <进程名>");
        std::process::exit(2);
    };
    let process = args.next().unwrap_or_default();
    if command != "dump" || process.is_empty() {
        eprintln!("用法: {program} dump <进程名>");
        std::process::exit(2);
    }

    let windows = top_windows_of(&process);
    if windows.is_empty() {
        eprintln!("未找到进程 {process} 的可见顶层窗口");
        std::process::exit(1);
    }

    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _ = com;
    let client = create_uia_client();

    let foreground = unsafe { GetForegroundWindow() };
    for hwnd in windows {
        println!(
            "\n=== 窗口 hwnd={}{} ===",
            hwnd.0 as isize,
            if foreground == hwnd { "（前台）" } else { "" }
        );
        dump_window(&client, hwnd);
    }
}

/// 进程名（不区分大小写，不含 .exe 后缀也可）对应的所有可见顶层窗口
fn top_windows_of(process: &str) -> Vec<HWND> {
    let want = process.to_ascii_lowercase();
    let mut found: Vec<HWND> = Vec::new();

    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut found as *mut _ as *mut std::ffi::c_void as isize),
        );
    }
    found
        .into_iter()
        .filter(|hwnd| {
            let name = process_name(*hwnd).to_ascii_lowercase();
            name == want || name.trim_end_matches(".exe") == want.trim_end_matches(".exe")
        })
        .collect()
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = &mut *(lparam.0 as *mut Vec<HWND>);
    if IsWindowVisible(hwnd).as_bool() && !window_title(hwnd).is_empty() {
        found.push(hwnd);
    }
    BOOL(1)
}

fn window_title(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

fn process_name(hwnd: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return String::new();
    }
    let mut image = String::new();
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buffer = [0u16; 512];
            let mut length = buffer.len() as u32;
            if QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
            .is_ok()
            {
                image = String::from_utf16_lossy(&buffer[..length as usize]);
            }
            let _ = CloseHandle(handle);
        }
    }
    image.rsplit(['\\', '/']).next().unwrap_or_default().to_string()
}

fn create_uia_client() -> IUIAutomation {
    let client: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
        .expect("UIA 客户端创建失败");
    // 与生产一致的超时收紧，探针不因 provider 卡死而挂住
    if let Ok(client2) = client.cast::<IUIAutomation2>() {
        unsafe {
            let _ = client2.SetConnectionTimeout(150);
            let _ = client2.SetTransactionTimeout(300);
        }
    }
    client
}

fn dump_window(client: &IUIAutomation, hwnd: HWND) {
    let element = match unsafe { client.ElementFromHandle(hwnd) } {
        Ok(element) => element,
        Err(error) => {
            println!("窗口元素读取失败: {error}");
            return;
        }
    };

    println!("窗口元素 {}", element_summary(&element));

    // 生产入口读的是全局焦点元素（而非窗口元素的后代里挑）：它可能落在
    // 窗口外/落在无 TextPattern 的条带上（实测 VS Code 焦点在顶部条带时
    // 光标行在编辑区中部），单独 dump 才能对上生产的失败形态
    match unsafe { client.GetFocusedElement() } {
        Ok(focused) => println!("焦点元素 {}", element_summary(&focused)),
        Err(error) => println!("焦点元素读取失败: {error}"),
    }

    let text_elements = find_text_elements(client, &element);
    if text_elements.is_empty() {
        println!("后代无任何 TextPattern 元素（自绘不暴露文本树）");
        return;
    }
    println!("后代 TextPattern 元素 {} 个：", text_elements.len());
    for element in &text_elements {
        dump_text_element(element);
    }
}

/// ClassName/Name/焦点/矩形 的摘要
fn element_summary(element: &IUIAutomationElement) -> String {
    let class = unsafe { element.CurrentClassName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let name = unsafe { element.CurrentName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let focused = unsafe { element.CurrentHasKeyboardFocus() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    let rect = unsafe { element.CurrentBoundingRectangle() }
        .map(|rect| format_rect(&rect))
        .unwrap_or_else(|_| "不可读".to_string());
    let control_type = unsafe { element.CurrentControlType() }
        .map(|value| value.0)
        .unwrap_or(0);
    format!("ClassName='{class}' ControlType={control_type} Name='{name}' 焦点={focused} 矩形={rect}")
}

fn build_cache(client: &IUIAutomation) -> Option<IUIAutomationCacheRequest> {
    let cache = unsafe { client.CreateCacheRequest() }.ok()?;
    for property in [
        UIA_ClassNamePropertyId,
        UIA_NamePropertyId,
        UIA_HasKeyboardFocusPropertyId,
        UIA_BoundingRectanglePropertyId,
    ] {
        unsafe { cache.AddProperty(property) }.ok()?;
    }
    Some(cache)
}

fn find_text_elements(
    client: &IUIAutomation,
    root: &IUIAutomationElement,
) -> Vec<IUIAutomationElement> {
    let cache = match build_cache(client) {
        Some(cache) => cache,
        None => return Vec::new(),
    };
    let condition = match unsafe {
        client.CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &VARIANT::from(true))
    } {
        Ok(condition) => condition,
        Err(_) => return Vec::new(),
    };
    // FindAllBuildCache 无匹配时 windows crate 返回 Err，与「找不到」同义
    unsafe { root.FindAllBuildCache(TreeScope_Descendants, &condition, &cache) }
        .map(|elements| {
            let count = unsafe { elements.Length() }.unwrap_or(0).min(MAX_TEXT_ELEMENTS);
            (0..count)
                .filter_map(|index| unsafe { elements.GetElement(index) }.ok())
                .collect()
        })
        .unwrap_or_default()
}

fn dump_text_element(element: &IUIAutomationElement) {
    let class = unsafe { element.CachedClassName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let name = unsafe { element.CachedName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let focused = unsafe { element.CachedHasKeyboardFocus() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    let rect = unsafe { element.CachedBoundingRectangle() }
        .map(|rect| format_rect(&rect))
        .unwrap_or_else(|_| "不可读".to_string());
    println!("\n--- 文本元素 ClassName='{class}' Name='{name}' 焦点={focused} 矩形={rect} ---");

    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) }
    {
        let mut active = BOOL::default();
        match unsafe { pattern.GetCaretRange(&mut active) } {
            Ok(range) => {
                println!("GetCaretRange(active={}):", active.as_bool());
                dump_range(&range, "  ");
            }
            Err(error) => println!("GetCaretRange 失败: {error}"),
        }
    } else {
        println!("无 TextPattern2");
    }

    match unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) } {
        Ok(pattern) => match unsafe { pattern.GetSelection() } {
            Ok(ranges) => {
                let count = unsafe { ranges.Length() }.unwrap_or(0);
                println!("GetSelection 共 {count} 个:");
                for index in 0..count {
                    if let Ok(range) = unsafe { ranges.GetElement(index) } {
                        dump_range(&range, "  ");
                    }
                }
            }
            Err(error) => println!("GetSelection 失败: {error}"),
        },
        Err(_) => println!("无 TextPattern"),
    }
}

/// 一个范围的完整画像：折叠性 + 直接几何 + 各粒度展开的几何与端点比较
fn dump_range(range: &IUIAutomationTextRange, indent: &str) {
    let collapsed = unsafe {
        range.CompareEndpoints(TextPatternRangeEndpoint_Start, range, TextPatternRangeEndpoint_End)
    }
    .map(|value| value == 0)
    .unwrap_or(false);
    println!(
        "{indent}折叠={collapsed} 直接几何={}",
        fmt_rects(&bounding_rectangles(range))
    );
    if !collapsed {
        return;
    }

    for (label, unit) in EXPAND_UNITS {
        let probe = match unsafe { range.Clone() } {
            Ok(probe) => probe,
            Err(error) => {
                println!("{indent}{label} 克隆失败: {error}");
                continue;
            }
        };
        if unsafe { probe.ExpandToEnclosingUnit(unit) }.is_err() {
            println!("{indent}{label} 展开失败");
            continue;
        }
        let vs_start = compare(range, &probe, TextPatternRangeEndpoint_Start);
        let vs_end = compare(range, &probe, TextPatternRangeEndpoint_End);
        println!(
            "{indent}{label} 端点比较(起点,终点)=({vs_start},{vs_end}) 几何={}",
            fmt_rects(&bounding_rectangles(&probe))
        );
    }
}

fn compare(
    range: &IUIAutomationTextRange,
    other: &IUIAutomationTextRange,
    endpoint: TextPatternRangeEndpoint,
) -> i32 {
    unsafe { range.CompareEndpoints(TextPatternRangeEndpoint_Start, other, endpoint) }
        .unwrap_or(i32::MIN)
}

fn format_rect(rect: &RECT) -> String {
    format!("({},{},{},{})", rect.left, rect.top, rect.right, rect.bottom)
}

fn fmt_rects(rects: &[f64]) -> String {
    let blocks: Vec<String> = rects
        .chunks_exact(4)
        .map(|rect| format!("[{:.0},{:.0},{:.0},{:.0}]", rect[0], rect[1], rect[2], rect[3]))
        .collect();
    if blocks.is_empty() {
        "空".to_string()
    } else {
        blocks.join(",")
    }
}

/// 与生产同款的边界矩形读取（SAFEARRAY → [left, top, width, height] × N）
fn bounding_rectangles(range: &IUIAutomationTextRange) -> Vec<f64> {
    let Ok(array) = (unsafe { range.GetBoundingRectangles() }) else {
        return Vec::new();
    };
    if array.is_null() {
        return Vec::new();
    }

    let (dims, element_size, count) = unsafe {
        (
            (*array).cDims,
            (*array).cbElements,
            (*array).rgsabound[0].cElements,
        )
    };
    let mut values = Vec::new();
    if dims == 1 && element_size == size_of::<f64>() as u32 {
        let mut data = std::ptr::null_mut();
        if unsafe { windows::Win32::System::Ole::SafeArrayAccessData(array, &mut data) }.is_ok()
            && !data.is_null()
        {
            values = (0..count)
                .map(|offset| unsafe { *(data as *const f64).add(offset as usize) })
                .collect();
            let _ = unsafe { windows::Win32::System::Ole::SafeArrayUnaccessData(array) };
        }
    }
    let _ = unsafe { windows::Win32::System::Ole::SafeArrayDestroy(array) };
    values
}
