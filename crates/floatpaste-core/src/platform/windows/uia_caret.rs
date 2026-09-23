//! UI Automation 插入符探测：Win32 线程插入符读不到时的兜底。
//!
//! 自绘插入符的应用（Chromium/Electron、WinUI/UWP、WPF…）不走
//! `CreateCaret`，其所在线程的 `hwndCaret` 恒为空——这正是
//! `GetGUIThreadInfo` 在浏览器、编辑器、IM 这类日常输入环境上普遍失效的
//! 原因。这类应用基本都通过 UI Automation 的 TextPattern 暴露插入符/选区
//! 范围，本模块负责把插入符的屏幕坐标从范围几何里取出来。
//!
//! 三个踩过的坑（都是「范围几何不可尽信」的不同侧面）：
//!
//! 1. 插入符范围是**退化（零长）范围**，而 `GetBoundingRectangles` 对退化范围
//!    按设计返回空数组，直接取几何必然一无所获——所以要把范围先展开到某个
//!    「文本单元」再取几何；
//! 2. **不能靠「矩形是否为空」判断范围折叠**：VS Code 的 EditContext 宿主对
//!    折叠范围照样返回整行矩形，据此把整行右沿当插入符会让速贴跑到远处；
//! 3. **展开粒度不能只认字符级**：CodeMirror（Obsidian）展开到字符返回空矩形，
//!    放大到词才拿得到几何。
//!
//! 还有三个更隐蔽的（真机探针在 Obsidian 上实测发现）：
//!
//! 4. **边界插入符的展开会跳到后面的单元**。贴在行尾/段尾/文末的插入符，
//!    展开结果不是它所在的单元，而是**下一个**单元——行尾展开落到下一行
//!    行首的空格、文末落到页面页脚（端点比较为负）。照探针取左沿，行中时
//!    碰巧等于插入符位置（下一字符的左沿就是插入符 x），行尾/文末则整整
//!    偏出一行或被甩到页脚——这正是「行中正常、行尾乱跳」的根源。识别出
//!    这种探针后，改为取**上一条视觉行**的末端（回退一行再展开）。
//! 5. **Move 在插入符范围的克隆上会虚报移动量**（返回 -1 但位置没动），
//!    在探针派生的范围上才可靠——收回一律基于探针范围做，不碰插入符克隆。
//!    另外软换行边界的探针端点比较为 0（伪装成正常贴起点），只能靠几何
//!    识别：探针落在跟随段落的非首条视觉行上即为软换行。
//! 6. **有些字段级焦点根本没有范围几何**。Obsidian 的空搜索框：插入符范围
//!    在字符/行/段粒度上都非折叠但矩形为空、词粒度干脆保持折叠；Obsidian
//!    的内联标题（点进去编辑）：焦点元素是无 TextPattern 的容器，连根文档
//!    的插入符范围都落在 0 且无几何。这类插入符就在焦点元素框内，范围路径
//!    全空时改用**元素框底边中心**兜底（provider 无列信息时，行带中间才是
//!    「贴着这一行」的预期，左沿会压住行号栏/输入框左端）——但仅当元素框是
//!    目标窗口客户区内的真
//!    子矩形：焦点散在整页（元素框≈客户区）或 Chromium 把滚动区外的坐标
//!    也算进框（编辑器文档框 bottom 比窗口底还低 488px，实测）时，框与插入
//!    符的相对位置无从谈起，宁可回退鼠标。
//!
//! 7. **焦点有时被报在无 TextPattern 的容器上**。部分应用（实测 WorkBuddy、
//!    Obsidian 内联标题）把 UIA 焦点报在容器/窗口根上，真正的文本编辑框在
//!    其后代里——此时对文本框调 GetCaretRange，provider 给出的就是真实插入
//!    符。所以焦点元素没有任何 TextPattern 时下钻后代搜文本框，搜不到再走
//!    字段框兜底；下钻是跨进程扫树，靠收紧事务超时兜住 provider 卡顿。
//!
//! 8. **范围几何会带进相邻元素的块，折叠与非折叠一视同仁**。实测夸克搜索框
//!    （折叠插入符）段展开返回图标容器、输入行、按钮三块；VS Code（EditContext）
//!    段展开带进别行的 emoji 块。越界块按末块/行块取会把锚点甩出输入行
//!    （夸克曾恒落图标容器右下角），还会让软换行判定把行粒度探针误判成
//!    「段落非首行」而走上一行回退（VS Code 锚点跳上一行行尾）。所有范围
//!    几何先按焦点元素框做垂直重叠过滤；同一视觉行的块高度不一（emoji
//!    16px、文本行 20px），行带取同行块的纵向并集。
//!
//! 9. **坏锚点必须当成没结果，在链内拦截而不是链外一票否决**。实测夸克新
//!    标签页搜索框：provider 首查（可访问性树刚构建）返回非折叠选区、几何
//!    只带 143px 高的图标容器块，行高校验若只在外层做，会把整条 UIA 路径
//!    判死直接回退鼠标（「第一次贴错、第二次才对」的根源）——而紧随其后的
//!    字段框兜底明明能给正确锚点。所以行高超出真实文本行上限的锚点在
//!    [`anchor_from_range`]/[`field_rect_anchor`] 出口就地过滤，让调用链
//!    继续找下一个来源。
//!
//! 客户端按线程缓存：COM 对象不能跨公寓共享，而探测固定发生在事件循环
//! 线程上，缓存即等价于复用 UIA 的跨进程连接（连接一旦建立，后续调用
//! 不再受连接超时约束）。

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;

use windows::core::{Interface, BOOL};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::{
    Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, SAFEARRAY,
    },
    Ole::{SafeArrayAccessData, SafeArrayDestroy, SafeArrayUnaccessData},
    Variant::VARIANT,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest, IUIAutomationElement,
    IUIAutomationTextPattern, IUIAutomationTextPattern2, IUIAutomationTextRange,
    TextPatternRangeEndpoint, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    TextUnit, TextUnit_Character, TextUnit_Line, TextUnit_Paragraph, TextUnit_Word,
    TreeScope_Descendants, UIA_ClassNamePropertyId, UIA_HasKeyboardFocusPropertyId,
    UIA_IsTextPatternAvailablePropertyId, UIA_NamePropertyId, UIA_TextPattern2Id,
    UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use crate::domain::error::AppError;

use super::active_app::ActiveAppResolver;
use super::picker_position::{Anchor, MAX_CARET_LINE_HEIGHT, ScreenPoint};

/// 连接超时：UIA 要跨进程连 provider，目标卡死时不设上限就会拖住事件循环
/// （定位在开窗路径上同步执行）。连接建立后被客户端缓存，后续调用不再受限
const UIA_CONNECTION_TIMEOUT_MS: u32 = 150;

/// 事务超时：后代下钻的 `FindFirstBuildCache` 一次会发起多个 provider 往返，
/// 每个往返各算一个事务，默认超时以数十秒计，provider 一卡顿就把开窗路径
/// 拖死。与连接超时一起在客户端创建时收紧
const UIA_TRANSACTION_TIMEOUT_MS: u32 = 300;

/// 失败现场日志的单字段上限：UIA 的 Name 可能是整段文档文本，截断防刷屏
const LOG_FIELD_CHARS: usize = 40;

/// 边界矩形数组每个矩形占的 double 个数（left / top / width / height）
const RECT_FIELDS: usize = 4;

/// 折叠插入符的展开粒度，由小到大：单元越小、几何越贴近插入符，所以逐级放大
/// 只在小说不清时退让（CodeMirror 的字符级几何就是空的）
const EXPAND_UNITS: [TextUnit; 4] = [
    TextUnit_Character,
    TextUnit_Word,
    TextUnit_Line,
    TextUnit_Paragraph,
];

thread_local! {
    /// 每线程一个 UIA 客户端；初始化失败也记 `None` 缓住，避免每次定位都
    /// 重试一遍 COM 创建
    static UIA_CLIENT: RefCell<Option<Option<IUIAutomation>>> = const { RefCell::new(None) };
}

/// 目标窗口焦点元素的插入符锚点（下沿中点 + 插入符行高）。
///
/// 前置条件是目标窗口**仍是前台窗口**：UIA 的「焦点元素」是全局概念，
/// 目标不是前台时读到的会是别的窗口的插入符，宁可不给
pub fn caret_point_via_uia(target_hwnd: isize) -> Result<Anchor, AppError> {
    if ActiveAppResolver::current_foreground_hwnd() != Some(target_hwnd) {
        return Err(AppError::Message("目标窗口已不是前台窗口".to_string()));
    }

    let element = unsafe { uia_client()?.GetFocusedElement() }
        .map_err(|error| AppError::Message(format!("读取焦点元素失败: {error}")))?;

    // 窗口根元素：焦点条带（VS Code 的 EditContext 宿主）自身无 TextPattern
    // 且无文本后代时，从窗口根搜文本元素——根文档的选区几何是实时光标行
    let window_root = unsafe {
        uia_client()?
            .ElementFromHandle(HWND(target_hwnd as *mut _))
    }
    .ok();

    let attempt = caret_anchor(&element, window_client_rect(target_hwnd), window_root.as_ref());
    match attempt.anchor {
        Some(anchor) => Ok(anchor),
        None => {
            // 定位最终失败：补采一轮轻量现场、一次输出——新应用出问题时
            // 诊断不用从零开始；成功路径完全不碰这里
            tracing::debug!(
                "UIA 插入符定位失败现场: {}",
                failure_trace(&element, &attempt.drilldown)
            );
            Err(AppError::Message(
                "焦点元素未暴露可用的插入符几何".to_string(),
            ))
        }
    }
}

/// 目标窗口客户区的屏幕坐标（UIA 的元素矩形是屏幕物理像素，同一坐标系才可比）
fn window_client_rect(target_hwnd: isize) -> Option<RECT> {
    let hwnd = HWND(target_hwnd as *mut _);
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    let mut top_left = POINT {
        x: rect.left,
        y: rect.top,
    };
    let mut bottom_right = POINT {
        x: rect.right,
        y: rect.bottom,
    };
    if !unsafe { ClientToScreen(hwnd, &mut top_left) }.as_bool()
        || !unsafe { ClientToScreen(hwnd, &mut bottom_right) }.as_bool()
    {
        return None;
    }

    Some(RECT {
        left: top_left.x,
        top: top_left.y,
        right: bottom_right.x,
        bottom: bottom_right.y,
    })
}

/// 一次焦点元素定位的产出：锚点 + 下钻记录。下钻记录只在定位最终失败时
/// 被失败现场 dump 读取
struct CaretAttempt {
    anchor: Option<Anchor>,
    drilldown: DrilldownRecord,
}

fn caret_anchor(
    element: &IUIAutomationElement,
    window_rect: Option<RECT>,
    window_root: Option<&IUIAutomationElement>,
) -> CaretAttempt {
    let mut attempt = CaretAttempt {
        anchor: None,
        drilldown: DrilldownRecord::default(),
    };
    attempt.anchor = element_caret_anchor(
        element,
        window_rect,
        true,
        &mut attempt.drilldown,
        window_root,
    );
    attempt
}

/// 元素级插入符定位（GetCaretRange → GetSelection → 字段框兜底）。
///
/// `allow_drilldown` 控制元素没有任何 TextPattern 时是否下钻后代搜文本框：
/// 只有焦点元素这一层允许——下钻命中的候选复用本函数时必须关掉，候选已按
/// 「支持 TextPattern」筛过，病态 provider 连 pattern 查询都不兑现时再下钻
/// 只会把搜索递归下去
fn element_caret_anchor(
    element: &IUIAutomationElement,
    window_rect: Option<RECT>,
    allow_drilldown: bool,
    drilldown: &mut DrilldownRecord,
    window_root: Option<&IUIAutomationElement>,
) -> Option<Anchor> {
    // 元素框只服务一件事：识别「provider 拿整个元素充当字符」的无效几何
    let element_rect = unsafe { element.CurrentBoundingRectangle() }.ok();

    // 插入符范围（TextPattern2）优先：选区反向时它也指向真正的插入点
    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) }
    {
        let mut active = BOOL::default();
        if let Ok(range) = unsafe { pattern.GetCaretRange(&mut active) } {
            if let Some(anchor) = anchor_from_range(&range, element_rect) {
                debug_anchor("插入符范围", &anchor);
                return Some(anchor);
            }
        }
    }

    // 退路：选区范围（只有 TextPattern 的老 provider）。未选中文本时它折叠在
    // 插入点上，与插入符范围同义
    let Some(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }.ok()
    else {
        // 连 TextPattern 都没有的字段级焦点（如 Obsidian 内联标题）：provider
        // 常把焦点报在容器/窗口根上，真实键盘焦点在后代文本框里，先下钻，
        // 搜不到再从窗口根下钻（焦点条带自身无文本后代，实测 VS Code 的
        // EditContext 宿主条带就是这样，且条带矩形会陈旧、跟丢光标行），
        // 仍搜不到才用焦点元素框兜底
        if allow_drilldown {
            drilldown.attempted = true;
            if let Some(anchor) = descendant_caret_anchor(element, window_rect, drilldown)
                .or_else(|| {
                    window_root.and_then(|root| {
                        drilldown.from_window_root = true;
                        descendant_caret_anchor(root, window_rect, drilldown)
                    })
                })
            {
                return Some(anchor);
            }
        }
        return field_rect_anchor(element_rect, window_rect);
    };
    let ranges = unsafe { pattern.GetSelection() }.ok()?;
    let count = unsafe { ranges.Length() }.unwrap_or(0);

    let selected = (0..count).find_map(|index| {
        let range: IUIAutomationTextRange = unsafe { ranges.GetElement(index) }.ok()?;
        anchor_from_range(&range, element_rect)
    });
    if let Some(anchor) = &selected {
        debug_anchor("选区范围", anchor);
        return selected;
    }
    field_rect_anchor(element_rect, window_rect)
}

/// 焦点元素没有任何 TextPattern 时的后代下钻：对 `TreeScope_Descendants` 用
/// `FindFirstBuildCache` 搜文本框。两级条件从紧到松：先「持有键盘焦点且支持
/// TextPattern」——下钻场景正是真实键盘焦点在文本框里；搜不到再放宽为只
/// 「支持 TextPattern」（HasKeyboardFocus 更新不可靠的 provider 上兜一手）。
/// 命中候选后复用 [`element_caret_anchor`]（此时调 GetCaretRange，provider
/// 会给出真实插入符）；搜不到返回 `None`，由调用方走字段框兜底
fn descendant_caret_anchor(
    element: &IUIAutomationElement,
    window_rect: Option<RECT>,
    record: &mut DrilldownRecord,
) -> Option<Anchor> {
    let client = uia_client().ok()?;
    let cache = candidate_cache(&client)?;
    let text_pattern = unsafe {
        client.CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &VARIANT::from(true))
    }
    .ok()?;
    let focused = unsafe {
        client.CreatePropertyCondition(UIA_HasKeyboardFocusPropertyId, &VARIANT::from(true))
    }
    .ok()?;
    let focused_text = unsafe { client.CreateAndCondition(&focused, &text_pattern) }.ok()?;

    for condition in [&focused_text, &text_pattern] {
        // FindFirst 跨进程扫树，provider 卡顿靠事务超时兜住（见
        // UIA_TRANSACTION_TIMEOUT_MS）；无匹配时 windows crate 返回 Err
        let Ok(candidate) =
            (unsafe { element.FindFirstBuildCache(TreeScope_Descendants, condition, &cache) })
        else {
            continue;
        };
        record.candidate = Some(candidate_label(&candidate));
        tracing::debug!(
            "UIA 下钻命中候选 {}",
            record.candidate.as_deref().unwrap_or("")
        );
        let mut discarded = DrilldownRecord::default();
        return element_caret_anchor(&candidate, window_rect, false, &mut discarded, None);
    }
    None
}

/// 下钻候选的 CacheRequest：ClassName/Name 随 FindFirst 一次带回，候选摘要
/// 读取零跨进程往返。按需缓存——条件属性（HasKeyboardFocus 等）由引擎匹配
/// 时自取，不必进缓存
fn candidate_cache(client: &IUIAutomation) -> Option<IUIAutomationCacheRequest> {
    let cache = unsafe { client.CreateCacheRequest() }.ok()?;
    unsafe { cache.AddProperty(UIA_ClassNamePropertyId) }.ok()?;
    unsafe { cache.AddProperty(UIA_NamePropertyId) }.ok()?;
    Some(cache)
}

/// 后代下钻的执行记录：定位最终失败时由失败现场 dump 读取
#[derive(Default)]
struct DrilldownRecord {
    /// 触发了后代搜索（焦点元素没有任何 TextPattern 时）
    attempted: bool,
    /// 焦点元素自身无文本后代后改从窗口根搜（VS Code EditContext 宿主条带）
    from_window_root: bool,
    /// 命中的候选元素摘要（字段已截断）；`None` = 没搜到候选
    candidate: Option<String>,
}

impl DrilldownRecord {
    /// 失败现场「下钻」字段的文本：区分「焦点元素自己有 TextPattern，压根
    /// 没下钻」「下钻了但没搜到候选」「窗口根下钻」几种失败形态
    fn summary(&self) -> String {
        let scope = if self.from_window_root {
            "窗口根"
        } else {
            "焦点元素"
        };
        match (self.attempted, &self.candidate) {
            (false, _) => "未触发".to_string(),
            (true, None) => format!("从{scope}搜无候选"),
            (true, Some(candidate)) => format!("从{scope}命中候选 {candidate}"),
        }
    }
}

/// 行高超出真实文本行上限的锚点当作「没结果」就地过滤（见模块文档坑 9）：
/// 校验若只在外层做，一个坏锚点会否决整条 UIA 路径让直接回退鼠标，链内
/// 更准的字段框兜底反而没有机会跑
fn trusted(anchor: Option<Anchor>) -> Option<Anchor> {
    anchor.filter(|anchor| anchor.line_height <= MAX_CARET_LINE_HEIGHT)
}

/// 从一个范围取插入点锚点：
///
/// - **非折叠范围**（用户选中了文本）本身就有几何，插入点取活动端——UIA 不
///   直接给出哪端是活动端，取末个矩形的右沿（正向选区的常见情形）；
/// - **折叠范围**是插入符，按规范没有几何，逐级放大粒度直到拿到可用几何。
///
/// 两条路取到的矩形都要过 [`covers_element`]：几何分辨不出插入符位置时
/// 宁可不给，让调用方回退鼠标位置
fn anchor_from_range(range: &IUIAutomationTextRange, element_rect: Option<RECT>) -> Option<Anchor> {
    // 范围几何统一先过元素框过滤，折叠与非折叠一视同仁：段展开跨元素是
    // 常态谎言（实测夸克搜索框带进图标容器与按钮块、VS Code 带进上一行的
    // emoji 块），越界块会把软换行判定、上一行回退和末块选取全部带偏
    if !is_collapsed(range) {
        // 跨行选区给出多块矩形，插入点在选区**末**端（正向选区的常见情形），
        // 取末块右沿
        return trusted(
            caret_rect(
                &blocks_within_element(&bounding_rectangles(range), element_rect),
                false,
            )
            .filter(|rect| !covers_element(*rect, element_rect))
            .map(|rect| anchor_from_rect(rect, false)),
        );
    }

    // 行级锚点（无列信息的整行几何）只当兜底：后面粒度的展开里可能藏着
    // 插入符条（列位置），扫完全部粒度再落兜底
    let mut line_anchor: Option<Anchor> = None;
    for unit in EXPAND_UNITS {
        let Some(probe) = expanded_to(range, unit) else {
            continue;
        };
        let forward_rects = blocks_within_element(&bounding_rectangles(&probe), element_rect);

        // VS Code 的 EditContext 把整行矩形当元素框（covers_element 拒掉、
        // 列位置丢失），但段展开里藏着 1px 宽的插入符条本身——命中即拿回
        // 列位置，优先于任何行级锚点
        if let Some(anchor) = trusted(caret_bar_anchor(&forward_rects)) {
            return Some(anchor);
        }

        let (Some(vs_start), Some(vs_end)) = (
            compare(range, &probe, TextPatternRangeEndpoint_Start),
            compare(range, &probe, TextPatternRangeEndpoint_End),
        ) else {
            continue;
        };
        let Some(attachment) = attachment(vs_start, vs_end) else {
            continue;
        };

        // 插入符贴在单元边界（行尾/段尾/文末，含软换行）时，探针是插入符
        // **之后**的单元，几何与插入符无关——真正贴着插入符的是**上一条
        // 视觉行**，其末块右沿正是插入符位置
        let needs_previous_line = match attachment {
            Attachment::AfterCaret => true,
            Attachment::AtStart => is_soft_wrap(
                &blocks_within_element(
                    &expanded_rects(range, TextUnit_Paragraph),
                    element_rect,
                ),
                &forward_rects,
            ),
            Attachment::AtEnd => false,
        };
        if line_anchor.is_none() {
            // 赋值时就地校验：坏锚点占了位会挡住后续粒度的好锚点
            line_anchor = trusted(if needs_previous_line {
                previous_line_rect(&probe, element_rect)
                    .map(|previous| anchor_from_rect(previous, false))
                    // 回退不了上一行（Move 不动/拿不到几何）时退回探针自身：
                    // 探针行就是插入符自己的行（实测 VS Code 空行的字符展开
                    // 即整条空行），其左沿正是插入符位置
                    .or_else(|| probe_anchor(&forward_rects, attachment, element_rect))
            } else {
                probe_anchor(&forward_rects, attachment, element_rect)
            });
        }
    }
    line_anchor
}

/// 探针几何转锚点：整块被元素框吞掉（provider 拿元素充当字符）时不算数
fn probe_anchor(
    forward_rects: &[f64],
    attachment: Attachment,
    element_rect: Option<RECT>,
) -> Option<Anchor> {
    let at_start = attachment == Attachment::AtStart;
    let rect = single_rect(forward_rects)?;
    if covers_element(rect, element_rect) {
        return None;
    }
    Some(anchor_from_rect(rect, at_start))
}

/// 插入符贴在展开探针的哪一端
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Attachment {
    /// 探针起点在插入符**之后**：provider 把贴在单元末端的插入符的展开跳到了
    /// 下一个单元（行尾→下一行行首、文末→页脚），探针几何与插入符无关，
    /// 要改用上一条视觉行的末端
    AfterCaret,
    /// 插入符贴在探针起点（行中展开的正常形态），取首块左沿
    AtStart,
    /// 插入符贴在探针末端之后（允许越过：Chromium 的插入符可能落在单元的
    /// 段落分隔符之后，比单元末端大 1），取末块右沿
    AtEnd,
}

/// 按端点比较值归类插入符与探针的位置关系：`vs_start` 是插入符起点与探针
/// 起点的比较值，`vs_end` 是插入符起点与探针终点的比较值。插入符夹在探针
/// 内部（两个比较值一正一负，如行中的词）时水平位置无从得知，返回 `None`
fn attachment(vs_start: i32, vs_end: i32) -> Option<Attachment> {
    match vs_start {
        n if n < 0 => Some(Attachment::AfterCaret),
        0 => Some(Attachment::AtStart),
        _ => (vs_end >= 0).then_some(Attachment::AtEnd),
    }
}

/// 探针是否落在跟随段落的**非首条视觉行**上。是，则说明插入符贴在软换行
/// 边界：探针（下一视觉行的首单元）的左沿是下一行行首，插入符其实在上一条
/// 视觉行的行尾。段落首行上的探针不受影响（真行首/行中，探针左沿即插入符）。
/// 软换行是局部现象：探针离段落首块超过 `SOFT_WRAP_MAX_LINE_SPANS` 个
/// 首块行高就不是换行边界，而是段展开带进来的远处相邻元素块（实测 VS Code
/// 根文档的段展开把顶部 breadcrumb 块带进来，离光标行 365px）
fn is_soft_wrap(para_rects: &[f64], probe_rects: &[f64]) -> bool {
    let Some(first_line) = para_rects
        .chunks_exact(RECT_FIELDS)
        .filter(|rect| is_usable(rect))
        .next()
    else {
        return false;
    };
    let Some(probe_rect) = probe_rects
        .chunks_exact(RECT_FIELDS)
        .filter(|rect| is_usable(rect))
        .next()
    else {
        return false;
    };

    let gap = probe_rect[1] - first_line[1];
    gap > first_line[3] / 2.0 && gap <= SOFT_WRAP_MAX_LINE_SPANS * first_line[3]
}

/// 软换行判定里探针与段落首块的最大行距（以首块行高计）：真软换行只隔
/// 一两条视觉行，隔得更远的是跨元素垃圾块
const SOFT_WRAP_MAX_LINE_SPANS: f64 = 3.0;

/// 上一条视觉行的**末块**矩形（右沿即贴在探针起点边界上的插入符位置）：
/// 塌回探针起点、回退一行、展开到行粒度。已在文档首行（回退不动）或拿不到
/// 几何时返回 `None`。Move 在探针派生范围上语义可靠（真机探针实测），
/// 在插入符克隆上则会虚报移动量，所以不用后者。行几何同样过元素框过滤：
/// 夸克的段把图标容器也算作「上一行」，不过滤会把锚点甩到图标块右下角
fn previous_line_rect(
    probe: &IUIAutomationTextRange,
    element_rect: Option<RECT>,
) -> Option<[f64; 4]> {
    let line = unsafe { probe.Clone() }.ok()?;
    unsafe {
        line.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &line,
            TextPatternRangeEndpoint_Start,
        )
    }
    .ok()?;
    let moved = unsafe { line.Move(TextUnit_Line, -1) }.ok()?;
    if moved >= 0 {
        return None;
    }
    unsafe { line.ExpandToEnclosingUnit(TextUnit_Line) }.ok()?;

    let rects = blocks_within_element(&bounding_rectangles(&line), element_rect);
    caret_rect(&rects, false)
}

/// 展开到指定粒度后的边界矩形；展开失败返回空
fn expanded_rects(range: &IUIAutomationTextRange, unit: TextUnit) -> Vec<f64> {
    expanded_to(range, unit)
        .map(|probe| bounding_rectangles(&probe))
        .unwrap_or_default()
}

/// 把范围展开到指定粒度。展开失败、或这个粒度上没有可展开的内容（空输入框）
/// 都返回 `None`
fn expanded_to(range: &IUIAutomationTextRange, unit: TextUnit) -> Option<IUIAutomationTextRange> {
    let probe = unsafe { range.Clone() }.ok()?;
    unsafe { probe.ExpandToEnclosingUnit(unit) }.ok()?;

    (!is_collapsed(&probe)).then_some(probe)
}

/// 范围内两个端点谁在前；读不到返回 `None`
fn compare(
    range: &IUIAutomationTextRange,
    other: &IUIAutomationTextRange,
    other_endpoint: TextPatternRangeEndpoint,
) -> Option<i32> {
    unsafe { range.CompareEndpoints(TextPatternRangeEndpoint_Start, other, other_endpoint) }.ok()
}

/// 范围是否零长（折叠）。**不能用「矩形是否为空」代替**：VS Code 的
/// EditContext 宿主给折叠范围也返回整行矩形
fn is_collapsed(range: &IUIAutomationTextRange) -> bool {
    compare(range, range, TextPatternRangeEndpoint_End) == Some(0)
}

/// 范围内**恰好一块**可用矩形。多块说明展开跨了元素或行的边界（Chromium 的
/// 文档级插入符展开碰到段落分隔符就会带上相邻元素的框），那种几何定位不了
/// 插入符，宁可不给
fn single_rect(rects: &[f64]) -> Option<[f64; 4]> {
    let mut rects = rects.chunks_exact(RECT_FIELDS).filter(is_usable);

    let rect = rects.next()?;
    rects
        .next()
        .is_none()
        .then(|| [rect[0], rect[1], rect[2], rect[3]])
}

/// 插入点所在的那块矩形：`at_start` 决定取**首块**还是**末块**可用矩形（对应
/// 插入点贴在范围的哪一端）
fn caret_rect(rects: &[f64], at_start: bool) -> Option<[f64; 4]> {
    let mut rects = rects.chunks_exact(RECT_FIELDS).filter(is_usable);

    let rect = if at_start {
        rects.next()?
    } else {
        rects.next_back()?
    };

    Some([rect[0], rect[1], rect[2], rect[3]])
}

/// 保留与焦点元素框**垂直重叠**的块（元素框读不到时原样返回）。范围几何
/// 跨元素是常态谎言：非折叠选区与折叠插入符的段展开都会把相邻元素块带上
/// （实测夸克搜索框带上 143 高的图标容器与按钮块、VS Code 带上别行的
/// emoji 块），插入点必在焦点元素内，越界块当行块/末块取会把锚点甩出输入
/// 行、还会把软换行判定与上一行回退带偏；全被过滤时回退原数组，过滤本身
/// 不成为新的失败源
fn blocks_within_element(rects: &[f64], element_rect: Option<RECT>) -> Vec<f64> {
    let Some(element) = element_rect else {
        return rects.to_vec();
    };
    let (top, bottom) = (f64::from(element.top), f64::from(element.bottom));
    let kept: Vec<f64> = rects
        .chunks_exact(RECT_FIELDS)
        .filter(|rect| rect[1] < bottom && top < rect[1] + rect[3])
        .flatten()
        .copied()
        .collect();
    if kept.is_empty() {
        rects.to_vec()
    } else {
        kept
    }
}

/// 多块展开几何里找「插入符条」并转锚点：窄条 x + 行带的下沿与行高。
/// VS Code 的 EditContext 只给整行矩形——列位置丢失（行块 ≈ 元素框，会被
/// `covers_element` 拒掉落到字段框兜底，锚点恒在行首），但段展开会顺带
/// 返回 1px 宽的插入符条本身，从它拿回列位置
fn caret_bar_anchor(rects: &[f64]) -> Option<Anchor> {
    let (bar, line) = caret_bar_with_line(rects)?;
    Some(anchor_from_rect([bar[0], line[1], 1.0, line[3]], true))
}

/// 窄条 = 宽 ≤2px、高 ≥8px 的竖条；行带 = 与窄条 y 重叠的所有宽块（宽 >8px、
/// 高 ≥8px）的**纵向并集**——同一视觉行的文本块与 emoji/图标块高度不同
/// （实测 VS Code 行内 emoji 16px、文本行块 20px），取单块当行会把行底与
/// 行高都带偏（锚点整行错位）。窄条可能不止一条（行号栏指示器等装饰也是
/// 窄条形状、位于行块左侧），取**最右**的——装饰固定在文本区左沿之外，
/// 插入符条跟着插入符走。实测 VS Code 段展开返回 `[836,415,1,17]`（插入
/// 符条）与整行块 `[836,414,331,20]`
fn caret_bar_with_line(rects: &[f64]) -> Option<([f64; 4], [f64; 4])> {
    let blocks: Vec<&[f64]> = rects.chunks_exact(RECT_FIELDS).filter(is_usable).collect();
    let block_of = |rect: &[f64]| [rect[0], rect[1], rect[2], rect[3]];

    let bar = blocks
        .iter()
        .copied()
        .filter(|block| block[2] <= 2.0 && block[3] >= 8.0)
        .max_by(|a, b| a[0].total_cmp(&b[0]))
        .map(block_of)?;
    let row: Vec<&[f64]> = blocks
        .iter()
        .copied()
        .filter(|block| block[2] > 8.0 && block[3] >= 8.0)
        .filter(|block| block[1] < bar[1] + bar[3] && bar[1] < block[1] + block[3])
        .collect();
    if row.is_empty() {
        return None;
    }
    let top = row.iter().map(|block| block[1]).fold(f64::INFINITY, f64::min);
    let bottom = row
        .iter()
        .map(|block| block[1] + block[3])
        .fold(f64::NEG_INFINITY, f64::max);
    let left = row.iter().map(|block| block[0]).fold(f64::INFINITY, f64::min);
    let right = row
        .iter()
        .map(|block| block[0] + block[2])
        .fold(f64::NEG_INFINITY, f64::max);
    Some((bar, [left, top, right - left, bottom - top]))
}

/// 宽度为 0 是折叠插入符的正常形态；高度为 0、非有限值说明 provider 没给出
/// 真实几何，用它会定位到错误位置，宁可不给
fn is_usable(rect: &&[f64]) -> bool {
    rect.iter().all(|value| value.is_finite()) && rect[2] >= 0.0 && rect[3] > 0.0
}

/// 矩形转锚点：横向取插入点所贴的那条边，纵向取下沿、行高取矩形高度——与
/// Win32 插入符路径的 `rcCaret` 同语义，速贴贴这一行往下展开、翻到上方时
/// 让开整行
fn anchor_from_rect(rect: [f64; 4], at_start: bool) -> Anchor {
    Anchor {
        point: ScreenPoint {
            x: (if at_start { rect[0] } else { rect[0] + rect[2] }).round() as i32,
            y: (rect[1] + rect[3]).round() as i32,
        },
        line_height: rect[3].round() as i32,
        centered: false,
    }
}

/// provider 是否把**整个焦点元素**当成了一块「字符」几何。真会这样：Chromium
/// 对空输入框返回编辑器框、VS Code 的 EditContext 宿主返回整行——这种框分辨
/// 不出插入符落在哪，当锚点用会把速贴甩到元素右下角，不如回退鼠标位置
fn covers_element(rect: [f64; 4], element_rect: Option<RECT>) -> bool {
    let Some(element_rect) = element_rect else {
        return false;
    };
    let near = |value: f64, edge: i32| (value - f64::from(edge)).abs() <= 1.0;

    near(rect[0], element_rect.left)
        && near(rect[1], element_rect.top)
        && near(rect[2], element_rect.right - element_rect.left)
        && near(rect[3], element_rect.bottom - element_rect.top)
}

/// 范围几何一无所获时的字段兜底：插入符就在焦点元素框内，取框的**底边中心**
/// （provider 给不出插入符列位置时，行带/字段框中间是「贴着这一行」的合理
/// 位置——左沿会让速贴压住行号栏/输入框左端）。仅当元素框是窗口客户区内的
/// **真子矩形**才兜底：焦点散在整页（元素框≈客户区）或框滚出窗口（见模块
/// 文档坑 6）时框与插入符的相对位置无从谈起；拿不到元素框/客户区同理
fn field_rect_anchor(element_rect: Option<RECT>, window_rect: Option<RECT>) -> Option<Anchor> {
    let rect = element_rect?;
    let window = window_rect?;
    if rect.left >= rect.right || rect.top >= rect.bottom {
        return None;
    }

    let slack = 2;
    let contained = rect.left >= window.left - slack
        && rect.top >= window.top - slack
        && rect.right <= window.right + slack
        && rect.bottom <= window.bottom + slack;
    let near = |value: i32, edge: i32| (value - edge).abs() <= slack;
    let page_sized = near(rect.left, window.left)
        && near(rect.top, window.top)
        && near(rect.right, window.right)
        && near(rect.bottom, window.bottom);
    if !contained || page_sized {
        return None;
    }

    // provider 只给行带/字段框、分辨不出插入符列位置时，锚点取框**底边中心**
    // 而非左下角：贴左沿会让速贴压住行号栏/输入框左端（VS Code 焦点条带宽
    // 1038px 时曾恒落行首），行带中间才是「贴着这一行」的预期；有列信息的
    // 来源（插入符窄条、字符/词单元、上一行末块）都走不到这里，优先级照旧
    let height = f64::from(rect.bottom - rect.top);
    let center_x = f64::from(rect.left) + f64::from(rect.right - rect.left) / 2.0;
    let anchor = trusted(Some(Anchor {
        point: ScreenPoint {
            x: center_x.round() as i32,
            y: f64::from(rect.bottom).round() as i32,
        },
        line_height: height.round() as i32,
        centered: true,
    }))?;
    debug_anchor("字段框兜底", &anchor);
    Some(anchor)
}

/// UIA 定位最终失败时的一次性现场：焦点元素身份、模式支持、选区现状与下钻
/// 记录。全部字段都是轻量属性读取（或下钻路径上已带回的缓存值），只在失败
/// 路径补采一轮、拼成一行输出，成功路径零开销
fn failure_trace(element: &IUIAutomationElement, drilldown: &DrilldownRecord) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 焦点元素身份。读不到的字段直接跳过——dump 不能成为新的失败源
    if let Ok(class) = unsafe { element.CurrentClassName() } {
        parts.push(format!("ClassName='{}'", log_label(&class.to_string())));
    }
    if let Ok(control_type) = unsafe { element.CurrentControlType() } {
        parts.push(format!("ControlType={}", control_type.0));
    }
    if let Ok(name) = unsafe { element.CurrentName() } {
        parts.push(format!("Name='{}'", log_label(&name.to_string())));
    }
    if let Ok(rect) = unsafe { element.CurrentBoundingRectangle() } {
        parts.push(format!(
            "BoundingRectangle=({},{},{},{})",
            rect.left, rect.top, rect.right, rect.bottom
        ));
    }

    // 模式支持：与定位路径同口径（GetCurrentPatternAs 拿不到即视为不支持）
    let text_pattern =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) };
    parts.push(format!("TextPattern2={}", unsafe {
        element
            .GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
            .is_ok()
    }));
    parts.push(format!("TextPattern={}", text_pattern.is_ok()));

    // 支持 TextPattern 时补选区现状：数量 + 首个选区是否折叠
    if let Ok(pattern) = text_pattern {
        if let Ok(ranges) = unsafe { pattern.GetSelection() } {
            let count = unsafe { ranges.Length() }.unwrap_or(0);
            let mut selection = format!("选区数={count}");
            if count > 0 {
                if let Ok(first) = unsafe { ranges.GetElement(0) } {
                    selection.push_str(&format!(" 首选区折叠={}", is_collapsed(&first)));
                }
            }
            parts.push(selection);
        }
    }

    parts.push(format!("下钻={}", drilldown.summary()));
    parts.join(" ")
}

/// 下钻候选元素的日志摘要：ClassName/Name 已随 CacheRequest 带回，零跨进程
/// 往返
fn candidate_label(element: &IUIAutomationElement) -> String {
    let class = unsafe { element.CachedClassName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let name = unsafe { element.CachedName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!(
        "ClassName='{}' Name='{}'",
        log_label(&class),
        log_label(&name)
    )
}

/// 成功路径的锚点日志：贴错位置时锚点的来源与坐标是定位问题的唯一线索，
/// 每次 UIA 定位一行
fn debug_anchor(source: &str, anchor: &Anchor) {
    tracing::debug!(
        "UIA 锚点 来源={source} 点=({},{}) 行高={}",
        anchor.point.x,
        anchor.point.y,
        anchor.line_height
    );
}

/// 失败现场的单字段文本：控制字符（换行/制表）折成空格防日志破行，超长按
/// 字符数截断加省略号
fn log_label(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= LOG_FIELD_CHARS {
        return flat;
    }
    let mut truncated: String = flat.chars().take(LOG_FIELD_CHARS).collect();
    truncated.push('…');
    truncated
}

/// 范围的边界矩形数组（`[left, top, width, height] × N`）。调用失败与形状不符
/// 都返回空数组——对调用方而言都只意味着「这个范围没有可用几何」
fn bounding_rectangles(range: &IUIAutomationTextRange) -> Vec<f64> {
    let array = match unsafe { range.GetBoundingRectangles() } {
        Ok(array) if !array.is_null() => array,
        _ => return Vec::new(),
    };

    let rects = unsafe { take_bounding_rectangles(array) };
    // SAFETY: 数组是本次调用交出的，读完即销毁；访问计数已在 take_* 内减回
    let _ = unsafe { SafeArrayDestroy(array) };

    rects
}

/// 复制 `[left, top, width, height] × N` 的 double 数组。句柄所有权仍在调用
/// 方——这里只借读，读完解锁，由调用方销毁
unsafe fn take_bounding_rectangles(array: *mut SAFEARRAY) -> Vec<f64> {
    let (dims, element_size, count) = unsafe {
        (
            (*array).cDims,
            (*array).cbElements,
            (*array).rgsabound[0].cElements,
        )
    };
    // 形状不对就不是「每四个 double 一个矩形」，读下去只会读错
    if dims != 1 || element_size != size_of::<f64>() as u32 {
        return Vec::new();
    }

    let mut data: *mut c_void = std::ptr::null_mut();
    if unsafe { SafeArrayAccessData(array, &mut data) }.is_err() || data.is_null() {
        return Vec::new();
    }

    let values = (0..count)
        .map(|offset| unsafe { *(data as *const f64).add(offset as usize) })
        .collect();
    let _ = unsafe { SafeArrayUnaccessData(array) };

    values
}

fn uia_client() -> Result<IUIAutomation, AppError> {
    UIA_CLIENT.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| create_uia_client().ok())
            .clone()
            .ok_or_else(|| AppError::Message("UI Automation 客户端不可用".to_string()))
    })
}

fn create_uia_client() -> Result<IUIAutomation, AppError> {
    // winit 建窗时已 OleInitialize（STA）：同线程重复初始化返回 S_FALSE，
    // 线程已在 MTA 时返回 RPC_E_CHANGED_MODE——两种情况下 COM 都可用，
    // 不计较具体返回值
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    // 必须用 CUIAutomation8：v1 的 CUIAutomation 不支持 IUIAutomation2，
    // 也就设不了连接超时，跨进程 provider 卡死会一路拖住开窗
    let client: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)? };
    if let Ok(client2) = client.cast::<IUIAutomation2>() {
        let _ = unsafe { client2.SetConnectionTimeout(UIA_CONNECTION_TIMEOUT_MS) };
        let _ = unsafe { client2.SetTransactionTimeout(UIA_TRANSACTION_TIMEOUT_MS) };
    }

    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::{
        anchor_from_rect, attachment, blocks_within_element, caret_bar_with_line, caret_rect,
        covers_element, field_rect_anchor, is_soft_wrap, log_label, probe_anchor, single_rect,
        trusted, Attachment, DrilldownRecord, MAX_CARET_LINE_HEIGHT, LOG_FIELD_CHARS,
    };
    use crate::platform::windows::picker_position::{Anchor, ScreenPoint};
    use windows::Win32::Foundation::RECT;

    /// 锚点：下沿中点 + 行高
    fn anchor(x: i32, y: i32, line_height: i32) -> Anchor {
        Anchor {
            point: ScreenPoint { x, y },
            line_height,
            centered: false,
        }
    }

    #[test]
    fn caret_rect_at_start_uses_first_rect_left_edge() {
        let rect = caret_rect(&[1515.0, 1166.0, 1.0, 21.0], true);

        assert_eq!(rect, Some([1515.0, 1166.0, 1.0, 21.0]));
        assert_eq!(
            anchor_from_rect(rect.unwrap(), true),
            anchor(1515, 1187, 21)
        );
    }

    /// VS Code EditContext 只给整行矩形（列位置丢失），但段展开的多块里带
    /// 着插入符条——从最右窄条拿回列位置。数据为 VS Code 真机探针实测：
    /// 行块 [836,414,331,20]、插入符条 [836,415,1,17]、行号栏装饰条
    /// [826,407,1,34]（左侧，不取）、零几何块 [836,412,1,1]（不算）
    #[test]
    fn caret_bar_recovers_column_from_line_level_geometry() {
        let rects = [
            1193.0, 385.0, 16.0, 16.0, 826.0, 407.0, 1.0, 34.0, 836.0, 414.0, 331.0, 20.0, 836.0,
            412.0, 1.0, 1.0, 836.0, 415.0, 1.0, 17.0,
        ];

        let (bar, line) = caret_bar_with_line(&rects).unwrap();
        assert_eq!(bar, [836.0, 415.0, 1.0, 17.0]);
        assert_eq!(line, [836.0, 414.0, 331.0, 20.0]);
        assert_eq!(super::caret_bar_anchor(&rects), Some(anchor(836, 434, 20)));
    }

    /// 窄条必须与行块同现：单块窄条（如 Obsidian 行中的字符块，本身就是
    /// 1px 宽）没有配对行块，不当插入符条用，照常走首块左沿路径
    #[test]
    fn caret_bar_needs_line_block_companion() {
        assert_eq!(caret_bar_with_line(&[1515.0, 1166.0, 1.0, 21.0]), None);
        // Chromium 文末的跨元素垃圾：零几何块不构成窄条，只剩行块无窄条
        assert_eq!(
            caret_bar_with_line(&[410.0, 349.0, 1.0, 1.0, 322.0, 390.0, 900.0, 60.0]),
            None
        );
    }

    /// 同一视觉行的块高度不一（VS Code 行内 emoji 16px、文本行块 20px），
    /// 行带取纵向并集：锚点 y/行高跟文本行（404/20），而不是 emoji 的
    /// 401/16——旧实现按「最右行块」取 emoji，行底与行高都偏，速贴整行错位。
    /// 窄条 x 越过文本块右沿（emoji 探出文本块之外）也仍属同一行
    #[test]
    fn caret_bar_unions_row_blocks_of_mixed_heights() {
        let rects = [
            836.0, 384.0, 331.0, 20.0, 1193.0, 385.0, 16.0, 16.0, 1209.0, 385.0, 1.0, 17.0,
        ];

        assert_eq!(super::caret_bar_anchor(&rects), Some(anchor(1209, 404, 20)));
    }

    /// 段展开跨元素块（VS Code 带进别行 emoji）会让软换行判定误判：探针
    /// （行块）看似落在「段落非首行」上。元素框过滤后段落首块回到同行，
    /// 误判消除——否则行粒度上就会走「上一行回退」，锚点跳到上一行行尾
    #[test]
    fn element_filter_guards_soft_wrap_check() {
        let paragraph = [
            1193.0, 385.0, 16.0, 16.0, 826.0, 407.0, 1.0, 34.0, 836.0, 414.0, 331.0, 20.0,
            836.0, 415.0, 1.0, 17.0,
        ];
        let element = RECT {
            left: 836,
            top: 414,
            right: 1167,
            bottom: 434,
        };
        let probe = [836.0, 414.0, 331.0, 20.0];

        assert!(is_soft_wrap(&paragraph, &probe));
        let filtered = blocks_within_element(&paragraph, Some(element));
        assert!(!is_soft_wrap(&filtered, &probe));
    }

    /// 软换行局部性守卫：探针离段落首块超过 3 个首块行高的是跨元素垃圾
    /// 块，不是软换行。实测 VS Code 根文档段展开把顶部 breadcrumb 块
    /// [1313,331,64,17] 带进来，离光标行 [896,696,1449,23] 有 365px——
    /// 误判成软换行会把锚点甩去「上一行回退」，一路失败后落到窗口顶部的
    /// 焦点条带上
    #[test]
    fn soft_wrap_locality_guard_rejects_distant_paragraph_blocks() {
        assert!(!is_soft_wrap(
            &[1313.0, 331.0, 64.0, 17.0, 896.0, 696.0, 1449.0, 23.0],
            &[896.0, 696.0, 1449.0, 23.0]
        ));
        // 3 个行高内仍是软换行（多行段落）
        let wrapped = [
            1541.0, 1134.0, 667.0, 21.0, 1545.0, 1158.0, 81.0, 21.0,
        ];
        let probe = [1545.0, 1158.0, 17.0, 21.0];
        assert!(is_soft_wrap(&wrapped, &probe));
    }

    /// 探针几何转锚点：块被元素框吞掉（provider 拿元素充当「字符」）时
    /// 不算数；正常块按贴边取左右沿
    #[test]
    fn probe_anchor_rejects_element_sized_geometry() {
        let element = RECT {
            left: 896,
            top: 696,
            right: 2345,
            bottom: 719,
        };
        let whole_line = [896.0, 696.0, 1449.0, 23.0];

        assert_eq!(
            probe_anchor(&whole_line, Attachment::AtStart, Some(element)),
            None
        );
        // 下钻到根文档时元素框是整页，行块不被吞，正常给锚点（VS Code
        // 空行：插入符在行首，AtStart 取左沿）
        let page = RECT {
            left: 757,
            top: 261,
            right: 2375,
            bottom: 1277,
        };
        assert_eq!(
            probe_anchor(&whole_line, Attachment::AtStart, Some(page)),
            Some(anchor(896, 719, 23))
        );
        assert_eq!(
            probe_anchor(&whole_line, Attachment::AtEnd, Some(page)),
            Some(anchor(2345, 719, 23))
        );
    }

    /// 夸克新标签页首贴：provider 首查（可访问性树刚构建）返回非折叠选区、
    /// 几何只带 143px 高的图标容器块（实测日志锚点 (1755,762) 行高=143）。
    /// 行高超限的坏锚点在链内被拦成「没结果」，GetSelection 路径让位给
    /// 字段框兜底——外层一票否决会让首贴直接回退鼠标（第一次贴错的根源）
    #[test]
    fn oversized_anchor_is_rejected_in_chain_not_at_the_end() {
        let bad = anchor(1755, 762, 143);
        assert_eq!(trusted(Some(bad)), None);
        let good = anchor(1313, 801, 20);
        assert_eq!(trusted(Some(good)), Some(good));

        // 字段框兜底同口径：焦点元素框本身高 143（图标容器当焦点元素）时
        // 不兜底，宁缺毋滥
        let icon = RECT {
            left: 1539,
            top: 619,
            right: 1755,
            bottom: 762,
        };
        let client = RECT {
            left: 873,
            top: 446,
            right: 2349,
            bottom: 1329,
        };
        assert_eq!(field_rect_anchor(Some(icon), Some(client)), None);
        assert_eq!(MAX_CARET_LINE_HEIGHT, 100);
    }

    /// 夸克空搜索框（折叠插入符，真机探针）：各粒度展开都返回整条输入行
    /// （≈元素框，covers_element 会拒掉），段展开带进图标容器与按钮块。
    /// 过滤后只剩输入行，软换行判定不误判，最终落到字段框兜底＝输入行
    /// 底边中心，锚点贴住输入行
    #[test]
    fn quark_collapsed_caret_falls_to_field_anchor_not_icon_block() {
        let rects = [
            1503.0, 607.0, 216.0, 143.0, 1277.0, 769.0, 668.0, 20.0, 1277.0, 815.0, 16.0, 16.0,
        ];
        let element = RECT {
            left: 1277,
            top: 769,
            right: 1945,
            bottom: 789,
        };
        let line = [1277.0, 769.0, 668.0, 20.0];

        let kept = blocks_within_element(&rects, Some(element));
        assert_eq!(kept, line);
        // 行粒度探针与过滤后段落同块：不是软换行，不回退上一行（图标容器）
        assert!(!is_soft_wrap(&kept, &line));
        // 整行 == 元素框：covers_element 拒掉，字段框兜底接管（底边中心锚点）
        assert!(covers_element(line, Some(element)));
        assert_eq!(
            field_rect_anchor(Some(element), Some(RECT {
                left: 873,
                top: 446,
                right: 2349,
                bottom: 1329,
            })),
            Some(Anchor {
                point: ScreenPoint { x: (1277 + 1945) / 2, y: 789 },
                line_height: 20,
                centered: true,
            })
        );
    }

    /// 非折叠选区跨元素时越出焦点元素框的块要过滤：实测夸克搜索框选区
    /// 带上了上方图标容器块 [1503,654,216,143] 与下方按钮块 [1277,862,16,16]，
    /// 过滤后末块是真正的输入行，锚点落回输入行右沿
    #[test]
    fn blocks_within_element_drops_adjacent_element_blocks() {
        let rects = [
            1503.0, 654.0, 216.0, 143.0, 1277.0, 816.0, 668.0, 20.0, 1277.0, 862.0, 16.0, 16.0,
        ];
        let element = RECT {
            left: 1277,
            top: 816,
            right: 1945,
            bottom: 836,
        };

        let kept = blocks_within_element(&rects, Some(element));
        assert_eq!(kept, [1277.0, 816.0, 668.0, 20.0]);
        assert_eq!(
            anchor_from_rect(caret_rect(&kept, false).unwrap(), false),
            anchor(1945, 836, 20)
        );

        // 元素框读不到时不过滤；全部越界时回退原数组（过滤不成为失败源）
        assert_eq!(blocks_within_element(&rects, None), rects);
        let offscreen = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        assert_eq!(blocks_within_element(&rects, Some(offscreen)), rects);
    }

    /// 插入点贴在选区末端：取末个矩形的右沿（跨行选区即选区结尾那一行）
    #[test]
    fn caret_rect_at_end_uses_last_rect_right_edge() {
        let rects = [100.0, 100.0, 60.0, 18.0, 0.0, 118.0, 24.0, 18.0];

        assert_eq!(caret_rect(&rects, false), Some([0.0, 118.0, 24.0, 18.0]));
        assert_eq!(caret_rect(&rects, true), Some([100.0, 100.0, 60.0, 18.0]));
        assert_eq!(
            anchor_from_rect(caret_rect(&rects, false).unwrap(), false),
            anchor(24, 136, 18)
        );
    }

    /// 行高必须跟着矩形走：翻到上方时落位要靠它让开整行
    #[test]
    fn anchor_from_rect_reports_line_height_of_chosen_rect() {
        let rect = caret_rect(&[0.0, 118.0, 24.0, 31.0], false).unwrap();

        assert_eq!(anchor_from_rect(rect, false), anchor(24, 149, 31));
    }

    #[test]
    fn caret_rect_skips_rects_without_real_geometry() {
        // 末段矩形高度为 0（provider 未给出真实几何）：退回上一个有效矩形
        let rects = [100.0, 100.0, 60.0, 18.0, 400.0, 400.0, 0.0, 0.0];

        assert_eq!(caret_rect(&rects, false), Some([100.0, 100.0, 60.0, 18.0]));
    }

    /// 折叠插入符的展开几何必须是**单块**：Chromium 的文档级插入符在文末展开
    /// 会带上相邻元素的框（实测 `[[410,349,1,1],[322,390,900,60]]`），那种多块
    /// 结果定位不了插入符，必须让调用方继续往上找粒度或回退鼠标
    #[test]
    fn single_rect_rejects_expansions_that_crossed_element_borders() {
        let junk = [410.0, 349.0, 1.0, 1.0, 322.0, 390.0, 900.0, 60.0];

        assert_eq!(single_rect(&junk), None);
        assert_eq!(
            single_rect(&[322.0, 290.0, 88.0, 19.0]),
            Some([322.0, 290.0, 88.0, 19.0])
        );
        // 零几何的矩形不算数，不参与「恰好一块」的计数
        assert_eq!(
            single_rect(&[322.0, 290.0, 88.0, 19.0, 1.0, 2.0, 3.0, 0.0]),
            Some([322.0, 290.0, 88.0, 19.0])
        );
        assert_eq!(single_rect(&[]), None);
    }

    #[test]
    fn caret_rect_rejects_arrays_without_usable_rect() {
        // 高度为 0 / 非有限 / 空数组 / 不足一个矩形都不给结果，调用方回退鼠标
        assert_eq!(caret_rect(&[], true), None);
        assert_eq!(caret_rect(&[10.0, 10.0, 5.0, 0.0], true), None);
        assert_eq!(caret_rect(&[f64::NAN, 10.0, 5.0, 18.0], true), None);
        assert_eq!(caret_rect(&[10.0, 10.0, f64::INFINITY, 18.0], false), None);
        assert_eq!(caret_rect(&[10.0, 10.0, 5.0], true), None);
    }

    #[test]
    fn covers_element_flags_element_sized_rect() {
        let element = RECT {
            left: 1358,
            top: 811,
            right: 2321,
            bottom: 834,
        };

        // VS Code 的 EditContext 宿主：整行被当成一个「字符」
        assert!(covers_element([1358.0, 811.0, 963.0, 23.0], Some(element)));
        // 空输入框：整个编辑器框被当成一个「字符」
        assert!(covers_element([1359.0, 812.0, 962.0, 22.0], Some(element)));
        // 正常字符框不是元素框
        assert!(!covers_element([1515.0, 811.0, 9.0, 23.0], Some(element)));
        // 没有元素框时不拒绝（校验只是兜底，不该成为新的失败源）
        assert!(!covers_element([1358.0, 811.0, 963.0, 23.0], None));
    }

    /// 端点比较值的归类，数值全部来自 Obsidian 真机探针实测：
    /// 行中展开正常贴在探针起点；行尾/文末的展开被 provider 跳到后面的单元
    /// （比较值为负）；行中的词夹住插入符（一正一负）定位不了
    #[test]
    fn attachment_classifies_endpoint_comparisons() {
        // 行中「固｜定」：字符探针 = 下一字符「定」，起点即插入符
        assert_eq!(attachment(0, -1), Some(Attachment::AtStart));
        // 行尾插入符：探针 = 下一行行首空格/首词，探针起点在插入符之后
        assert_eq!(attachment(-1, -1), Some(Attachment::AfterCaret));
        // 行中「固｜定」：词探针 = 「固定」，插入符夹在单元内部
        assert_eq!(attachment(1, -1), None);
        // Chromium 段落分隔符越过：插入符比探针末端大 1
        assert_eq!(attachment(1, 1), Some(Attachment::AtEnd));
    }

    /// 软换行边界判定：探针落在跟随段落的**非首条视觉行**上（实测：软换行
    /// 段落「开启 win+v…/拦截下录制」，探针 = 第二行首字符「拦」）；行中与
    /// 段落首行的探针都不是软换行
    #[test]
    fn soft_wrap_detected_when_probe_sits_on_later_paragraph_line() {
        let wrapped_para = [1541.0, 1134.0, 667.0, 21.0, 1545.0, 1158.0, 81.0, 21.0];
        let probe_on_second_line = [1545.0, 1158.0, 17.0, 21.0];
        assert!(is_soft_wrap(&wrapped_para, &probe_on_second_line));

        let single_line = [1541.0, 400.0, 182.0, 21.0];
        let probe_mid_line = [1562.0, 400.0, 17.0, 21.0];
        assert!(!is_soft_wrap(&single_line, &probe_mid_line));

        let probe_on_first_line = [1557.0, 1134.0, 17.0, 21.0];
        assert!(!is_soft_wrap(&wrapped_para, &probe_on_first_line));

        // 段落/探针拿不到几何时都判成非软换行（交给其他粒度或回退）
        assert!(!is_soft_wrap(&[], &probe_mid_line));
        assert!(!is_soft_wrap(&single_line, &[]));
    }

    /// 行尾/文末插入符的锚点 = 上一条视觉行的末块右沿。实测：文末插入符
    /// 回退一行拿到内容行 [1541,1211,222,21]，右沿 1763 正是最后一个字符
    /// 「项」的右沿；旧实现取「下一单元」左沿会落到下一行行首甚至页脚
    #[test]
    fn boundary_anchor_uses_previous_line_right_edge() {
        let rects = [1541.0, 1211.0, 222.0, 21.0];

        let rect = caret_rect(&rects, false).unwrap();
        assert_eq!(
            anchor_from_rect(rect, false),
            Anchor {
                point: ScreenPoint { x: 1763, y: 1232 },
                line_height: 21,
                centered: false,
            }
        );
    }

    /// 上一条视觉行跨多块矩形（罕见）时取末块可用矩形，跳过零几何的垃圾块
    #[test]
    fn previous_line_rects_skip_unusable_blocks() {
        let rects = [1541.0, 1211.0, 222.0, 21.0, 10.0, 10.0, 0.0, 0.0];

        let rect = caret_rect(&rects, false).unwrap();
        assert_eq!(rect, [1541.0, 1211.0, 222.0, 21.0]);
    }

    /// 字段兜底：范围几何全空时用焦点元素框底边中心。数值来自 Obsidian 真机
    /// 探针：内联标题焦点元素（无 TextPattern）[1514,430,2215,462]、空搜索框
    /// （各粒度展开矩形为空）[1003,372,1223,402]，客户区 [947,320,2473,1251]
    #[test]
    fn field_rect_anchor_anchors_field_bottom_center() {
        let client = RECT {
            left: 947,
            top: 320,
            right: 2473,
            bottom: 1251,
        };
        let title = RECT {
            left: 1514,
            top: 430,
            right: 2215,
            bottom: 462,
        };
        assert_eq!(
            field_rect_anchor(Some(title), Some(client)),
            Some(Anchor {
                point: ScreenPoint { x: 1865, y: 462 },
                line_height: 32,
                centered: true,
            })
        );

        let search = RECT {
            left: 1003,
            top: 372,
            right: 1223,
            bottom: 402,
        };
        assert_eq!(
            field_rect_anchor(Some(search), Some(client)),
            Some(Anchor {
                point: ScreenPoint { x: 1113, y: 402 },
                line_height: 30,
                centered: true,
            })
        );
    }

    /// 字段兜底的守卫：焦点散在整页（元素框≈客户区）、框滚出窗口（Chromium
    /// 编辑器文档框 bottom 比窗口底低 488px）、缺元素框/客户区、退化框都不兜底
    #[test]
    fn field_rect_anchor_rejects_page_sized_or_bogus_rects() {
        let client = RECT {
            left: 947,
            top: 320,
            right: 2473,
            bottom: 1251,
        };

        assert_eq!(field_rect_anchor(Some(client), Some(client)), None);

        let editor_doc = RECT {
            left: 1514,
            top: 474,
            right: 2215,
            bottom: 1739,
        };
        assert_eq!(field_rect_anchor(Some(editor_doc), Some(client)), None);

        assert_eq!(field_rect_anchor(None, Some(client)), None);
        assert_eq!(
            field_rect_anchor(
                Some(RECT {
                    left: 1003,
                    top: 372,
                    right: 1223,
                    bottom: 402
                }),
                None
            ),
            None
        );
        assert_eq!(
            field_rect_anchor(
                Some(RECT {
                    left: 1003,
                    top: 402,
                    right: 1223,
                    bottom: 402
                }),
                Some(client)
            ),
            None
        );
    }

    /// 失败现场字段的截断与清洗：超长按**字符数**截断加省略号（中文等多字节
    /// 字符按字符计数，不撕开 UTF-8）；换行/制表等控制字符折成空格防日志破行
    #[test]
    fn log_label_caps_length_and_strips_control_chars() {
        assert_eq!(log_label(""), "");
        assert_eq!(log_label("搜索框"), "搜索框");

        let exactly: String = "汉".repeat(LOG_FIELD_CHARS);
        assert_eq!(log_label(&exactly), exactly);

        let long: String = "汉".repeat(LOG_FIELD_CHARS + 10);
        assert_eq!(
            log_label(&long),
            format!("{}…", "汉".repeat(LOG_FIELD_CHARS))
        );

        assert_eq!(log_label("第一行\n第二行\t第三行"), "第一行 第二行 第三行");
    }

    /// 下钻记录在失败现场里要能区分的形态：焦点元素自己有 TextPattern 时
    /// 未触发下钻；从焦点元素搜无候选；从窗口根搜（焦点条带无文本后代，
    /// VS Code EditContext 宿主）无候选；命中候选（带截断后的摘要）
    #[test]
    fn drilldown_record_summary_distinguishes_failure_shapes() {
        assert_eq!(DrilldownRecord::default().summary(), "未触发");

        assert_eq!(
            DrilldownRecord {
                attempted: true,
                from_window_root: false,
                candidate: None,
            }
            .summary(),
            "从焦点元素搜无候选"
        );

        assert_eq!(
            DrilldownRecord {
                attempted: true,
                from_window_root: true,
                candidate: None,
            }
            .summary(),
            "从窗口根搜无候选"
        );

        assert_eq!(
            DrilldownRecord {
                attempted: true,
                from_window_root: true,
                candidate: Some("ClassName='Edit' Name='搜索'".to_string()),
            }
            .summary(),
            "从窗口根命中候选 ClassName='Edit' Name='搜索'"
        );
    }
}
