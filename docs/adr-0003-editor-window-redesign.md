---
status: accepted
---

# 编辑窗口重设计与标签输入

编辑窗口沿用旧壳 800×600 骨架复刻而来，存在三个确认的体验痛点：标签建议浮层出现/消失是硬切换（`if` 条件实例化，无动画）；已选标签与内嵌输入框同处一条固定 38px 高的条内，输入框常驻 `min-width: 112px` 并 `horizontal-stretch: 1` 吃掉全部剩余宽度，标签少时大片空白、稀疏不紧凑；整帧重绘覆盖层（`force-repaint`）是黑盒 workaround。本 ADR 记录调研结论与实施方案。

## 调研结论

### 外部成熟模式（GitHub / Notion / Linear / Jira / NSTokenField / React Aria / M3 Chips）

- 编辑态标签容器的主流是 **flex-wrap 换行自增高**；横向滚动仅用于展示/筛选条。换行的前提是有流式布局能力。
- 建议浮层惯例：**fade + 2–4px 位移，100–200ms**（M3 短时长 token 150–250ms，Tailwind 默认 150ms）；键盘高亮移动**即时切换、不加过渡**。
- 浮层项采纳用 **pointer-down 而非 pointer-up**（Radix / React Aria 惯例），避免「浮层先消失、点击落空」。
- 占位符：有标签后收短为「添加标签…」；焦点态高亮容器整体而非裸输入框；点击容器任意空白即聚焦输入框是标配。
- chip 密度：主流 20–28px 高（本仓 24px 已贴合 GitHub/Linear 档位），间距 4–6px，删除按钮命中区 ≥16px。

### Slint 1.17 能力约束

- **无 FlowLayout**：布局仅横/纵/网格/路径四种，`for` 循环内无跨项累加能力，声明式换行不可行；自实现需 Rust 侧字体度量 + 行分组两阶段渲染，复杂度与穿帮风险不值。
- `TouchArea.pointer-event(PointerEventKind.down)` 可用（速贴窗已用）；「常驻实例 + `opacity` 绑定 + `animate`」是仓内既有浮层淡入惯用法（搜索窗悬停快捷粘贴）。
- 软件渲染器无应用侧 API 强制全帧重绘（见下文整帧重绘一节）。

## 方案

### 标签输入

- **建议浮层最初为单行横滚 + 行内输入框**，用户验收后否决：体验不及成熟编辑器。**改用 Notion/GitHub 式多选面板**——标签行只保留已选 chips（行内 × 删除，紧凑靠左）与「+ 添加标签」入口；添加/搜索/创建收敛到下拉面板（搜索框 + 已选区勾选行（点击移除）+ 建议区（match/create/exists，pointer-down 采纳））。面板是垂直单列 Flickable，**绕开了 Slint 无 FlowLayout 的约束**（换行收纳不可行，但单列列表完全可行），交互对齐成熟产品。
- 面板交互：打开（点「+ 添加」/标签行空白）即聚焦搜索框；回车采纳高亮或按输入创建、Tab 补全、上下键导航（高亮行滚动跟随）、Backspace 空输入删末位；Esc 分层退出（有输入先清空、再按关面板、焦点回窗口级）；点击面板外（搜索框失焦）即收起。
- 建议数据流不变（`EditorTagSuggestion`、`build_suggestions` 排除已选、空输入给常用标签）。
- chip 与删除钮 hover 底色反馈（120ms `animate background`，仓内惯例）。

### 编辑区命中与滚动

- **命中区修复**：原内容层 `x: 20px` 且窄 40px，点击左右边距区无任何反应（用户感知「点击失灵/很久才聚焦」）；内容层改为铺满视口宽（`x:0, width: flick.width`），TextInput 内移 `x:20px`，padding/空白区点击由下层 TouchArea 承接——聚焦并把光标置尾（旧版 textarea 整块可点语义）。
- **滚动**：viewport 内容高 = 全文排版高 + 上下 padding，超出视口即滚轮/拖拽可滚；光标跟随滚动的坐标换算随 TextInput 内移同步修正（cursor.y + 16px 转视口系）。

### 按钮与 tooltip

- **修复两段式删除按钮点击无效**：组件 `in property <bool> enabled;` 未初始化（Slint 属性缺省取类型默认值 false），实例化处又未赋值，TouchArea 自始被禁用——删除该属性（无使用方）。
- 键位提示从底栏移入按钮 tooltip（悬浮气泡 simple 模式，锚点=按钮下方）：保存「保存（Ctrl+S）」、关闭「关闭（Esc）」、删除「删除条目/再次点击确认删除」、图片打开「用系统查看器打开」。底栏随之精简（52→44px，仅行列 + 未保存标记 | 关闭 + 保存）。
- **tooltip 锚点基准修复**：气泡定位原以 `GetWindowRect`（含标题栏与边框）为宿主基准，而 Slint `absolute-position` 相对客户区——无框窗口两者重合所以搜索/速贴一直正常，带框的编辑窗则偏移一个非客户区高度，叠加越界翻转后气泡完全脱离按钮。改为以客户区原点（`ClientToScreen(0,0)`，`win32_ext::client_origin`）为基准，对两类窗口通用。

### 命中拦截与布局观感（第二轮验收反馈）

- **隐形面板拦截点击**：常驻标签面板 opacity=0 时**仍参与鼠标命中**，且搜索框（TextInput）未做 enabled 门控——面板覆盖编辑区上部 320×288px，点击文本开头即被隐形输入框抢走焦点（「鼠标还是经常失效」的根因）。`panel-input` 补 `enabled: root.tag-open`。教训：常驻+opacity 隐藏方案中，**每一个可命中元素都必须门控**，opacity 不隔离事件。
- **Flickable 拖拽拦截吞点击（「只有一行字时不容易失效」的根因）**：`interactive: true` 且内容可滚时，Flickable 在 pressed 阶段为拖拽滚动判定把事件 `Intercept`（源码 `input_event_filter_before_children`），**子元素收不到任何点击**——单行文本视口不可滚所以点击正常，多行可滚就失效，与真机观察吻合。标签行、文件列表、标签面板三处 Flickable 改 `interactive: false`（非滚轮事件 `ForwardAndIgnore` 完全透传）。
- **编辑区滚动最终弃用 Flickable，改自管滚动**：滚轮在此前的 Flickable 结构下始终无法可靠工作，改为完全显式的实现——内容层 `y: -scroll-y` 平移；`TouchArea.scroll-event`（公开 API，`PointerScrollEvent.delta_y`）驱动滚动，Windows 滚轮向下 `delta_y` 为负，故 `scroll-y -= event.delta_y`（与 Flickable 同源约定一致），上限 `max(0, 全文排版高 + 32 - 视口高)`。**结构要点：TouchArea 必须是 TextInput 的祖先**——滚轮在 TextInput 忽略后沿父链冒泡，兄弟元素收不到；TouchArea 同时承接空白区点击（聚焦 + 光标置尾）。光标跟随滚动改为绝对定位公式（`scroll-y = clamp(cursor.y + 16 - 8, 0, max)`），载入条目时 `scroll-y` 复位、跟随逻辑自动把视口推到置尾光标处。置尾光标的偏移按 **UTF-8 字节**传给 `set-selection-offsets`（Slint 内置语义，`docs` 原文 "between two UTF-8 offsets"）；Slint 侧无字节长度内建，由 Rust 维护 `draft-byte-len` 供给——用字符数 `char-count` 会让中文文本的光标落在字符中间。
- **tooltip 气泡居中**：静态提示锚点改为按钮中心、气泡按自身宽度水平居中（原左缘对齐在气泡宽于按钮时观感歪斜）。
- **布局去卡片化**：编辑区与标签行不再画灰底圆角容器（border/background 全去）——窗口即一张纸（Notion/Typora 式平铺），嵌套卡片是「表单感」的来源；图片/文件预览保留容器（图片需要界定）。标签在上方的位置保留（属性区在内容上方是编辑器惯例）。**但编辑区容器的不透明背景（`canvas-default`，与窗口同色）必须保留**：软件渲染器擦除光标闪烁时按「重画脏区内 items」进行，容器无背景则闪烁区域无底色可画。
- **首行文字上方小白点的真根因（离屏实验证实）**：非整数 DPI 缩放（125% 实测）下软件渲染器把光标条顶端**画出 TextInput 元素几何 1-2px**（parley 行盒顶高于元素顶；`.artifacts/white-dot-repro` 离屏复现：光标条物理顶 116 vs 元素顶 117.5，v=238 与用户截图白点 (237,238,240) 同值），而增量重绘的脏区只认元素几何——越界段「画得上、擦不掉」，在光标列位的文字上方残留孤立白点。100% 缩放整像素对齐不触发（开发环境复现不了、用户环境必现的原因）。**绕过**：`edit-ta` 内声明在 TextInput 之后的 4px 同色遮罩条（`y:12px`），把越界像素盖在同色之下——不遮首行墨迹（墨迹顶从其下 1.5px 起）、滚动后盲区随内容移出视口被裁。上游修复后可整体移除。

### 通知条与确认框

- 通知/错误条：`height: cond ? 36px : 0px` + `animate height` + `clip` + `opacity` 联动——0 高时布局分配为 0、无需 visible；不能用 `visible`（Slint 布局引擎不读 visible，隐藏仍占位，会让下方 flex 区高度跳变）。
- 未保存确认框：进场 `opacity` 淡入（`opacity: open ? 1 : 0` + `visible: open`，退场伴随窗口隐藏无需动画）。

### 编辑体验基线（对齐成熟编辑器）

功能集保持简单，交互对齐 Windows 编辑基线（Win32 Edit 控件 1985 年起的标准：Notepad/浏览器 textarea 全套）。源码级盘点 Slint 1.17 TextInput 内置能力后，标准键位**零成本获得**：

- Ctrl+Z 撤销 / Ctrl+Y 重做（Windows 惯例）/ Ctrl+A / Ctrl+C/X/V；方向键、Home/End、Ctrl+Home/End、Ctrl+←/→ 按词跳转、Shift+方向选区、Ctrl+Delete/Backspace 按词删除；双击选词、三击选段；IME 组词作为原子单元进撤销栈。
- 内置撤销栈的合并粒度：位置相邻的连续插入合并为一个单元，换行/删除/粘贴自然断开，重做栈在输入后清空——满足编辑窗口场景，不自研撤销。

本版补强（内置之外的差距项）：

- **标题栏脏标记**：`title` 动态绑定 `●`（VS Code 惯例），与底栏「未保存」点、保存按钮禁用态构成三重提示。
- **行列状态**：底栏显示「第 x 行 · 第 y 列」（1 起、列按字符数中英文同权）。Slint 层读 TextInput 内部 out property `cursor-position_byte-offset`（UTF-8 byte，undocumented、升级有脆弱风险）回调 Rust，按 draft 文本换算；`set-selection-offsets` 以 TriggerCallbacks 触发光标回调，载入置尾时行列与视口滚动自动同步。
- **图片/文件打开**：图片预览是 contain 缩略，右下角「打开」胶囊按钮交系统查看器细看（对齐 Ditto 类剪贴板管理器的预览边界）；文件条目路径行点击用系统默认程序打开，hover 描边 + 文字提亮 + pointer 光标提示可点。ShellExecuteW "open" 封装落 `core::platform::windows::shell_open`。

取舍（明确不做）：

- **按钮键盘焦点**：Slint TouchArea 按钮无焦点态支持，Tab 穿梭只达编辑区↔标签输入框（可聚焦元素间）；高频动作已有全键盘路径（Ctrl+S 保存、Esc 关闭、两段式删除单击直达），按钮 Tab 化留给通用按钮组件立项。
- **图片滚轮缩放/拖拽平移**：预览定位是快速识别，「打开」承接细看；缩放平移属图像查看器范畴。
- Insert 覆盖模式、自动保存：不做（手动保存 + 脏确认是本窗口设计）。

### 整帧重绘覆盖层（调查结论，保留现状）

现象：`SLINT_DESTROY_WINDOW_ON_HIDE` 下编辑窗 hide→show 后，静态区域（背景/头部/底栏）保持表面初始白底，Slint 只重绘「与上次渲染不同的区域」。

根因（源码级）：winit 后端每次 `render()` 无条件按 softbuffer 的 **buffer age** 选择重绘模式——age=1 → `RepaintBufferType::ReusedBuffer`（脏区跟踪，基准是上次场景图）。窗口隐藏销毁 surface 重建后，新 buffer 内容未初始化而 age 不可信，脏区基准指向已不存在的旧帧，静态区域因此不被重画。`occluded()` 回调虽有 `set_repaint_buffer_type(NewBuffer)` 清缓存，但会被下一次 `render()` 开头的 age 判断覆盖；软件渲染器亦无应用侧 API 强制 `NewBuffer`。

决策：**保留 `force-repaint` 覆盖层翻转方案**——在 show 前翻转属性增删一层全窗口低透明度覆盖元素，使所有区域与上次场景不同，脏区覆盖全窗；成本为每帧一次全窗软件绘制（编辑器为普通带框窗，无 60fps 压力），已真机验证有效。停屏替代路线（hide 改平移屏外保表面）因普通窗口的任务栏按钮与 Alt-Tab 残留被否决；全局关闭 `SLINT_DESTROY_WINDOW_ON_HIDE` 牺牲全窗口内存优化，否决。

## Considered Options

- **标签换行收纳（外部主流）**：否决。Slint 无 FlowLayout，自实现需 Rust 字体度量与两阶段渲染，穿帮风险高。
- **单行横滚 + 行内输入框**（首版方案）：被用户验收否决——行内混排输入框宽度难收敛、交互不及成熟产品，升级为多选面板。
- **「已选列表 + 独立输入行」常驻两段式**：否决。纵向空间常驻成本高；最终面板方案保留其精神（已选与输入分离）但按需弹出，不占内容区。
- **整帧重绘改停屏/关 DESTROY_ON_HIDE**：否决，见上。

## Consequences

- 标签交互收敛到面板后，`tag-open` 从绑定表达式改为显式状态（打开/关闭分别由「+ 添加」、Esc/搜索框失焦驱动）；建议数据流（`EditorTagSuggestion`、采纳/补全/移除回调）不变，`tag-escape` 语义改为「关面板 + 焦点回窗口级」。
- 新增 `core::platform::windows::shell_open`（ShellExecuteW "open" 打开路径），供编辑窗口与后续场景复用。
- 行列计算依赖 TextInput 未公开属性 `cursor-position_byte-offset`，Slint 升级若移除该属性则底栏行列退化（编译期即可发现），不影响其他功能。
- `force-repaint` 机制注释已详尽，若上游修复 buffer-age 判断可整体移除。
