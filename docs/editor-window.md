# 编辑窗口行为规格

> 活文档：编辑窗口的现行交互规格与实现约束，随实现更新。选型决策与
> 被否方案见 [adr-0003](adr-0003-editor-window-redesign.md)。

## 标签输入（多选面板）

- 标签行只保留已选 chips（行内 × 删除，紧凑靠左）与「+ 添加标签」入口；添加/搜索/创建收敛到下拉面板——搜索框 + 已选区勾选行（点击移除）+ 建议区（match / create / exists 三态）。
- 建议项采纳用 pointer-down 而非 pointer-up——浮层在 up 前消失会让点击落空。
- 面板交互：打开（点「+ 添加」/标签行空白）即聚焦搜索框；回车采纳高亮或按输入创建、Tab 补全、上下键导航（高亮行滚动跟随）、Backspace 空输入删末位；Esc 分层退出（有输入先清空、再按关面板、焦点回窗口级）；点击面板外（搜索框失焦）即收起。
- 建议数据流：`EditorTagSuggestion`、`build_suggestions` 排除已选、空输入给常用标签。
- 常驻 + opacity 隐藏方案中**每一个可命中元素都必须门控**（`enabled` 绑开合态）：opacity 不隔离鼠标命中，隐形面板/输入框会拦截下层编辑区的点击。

## 编辑区命中与自管滚动

- 内容层铺满视口宽（`x:0`），`TextInput` 内移留白，空白区点击由下层 `TouchArea` 承接——聚焦并把光标置尾。
- 滚轮滚动自管：内容层 `y: -scroll-y` 平移，`TouchArea.scroll-event` 即时应用（Windows 滚轮向下 `delta_y` 为负，`scroll-y -= delta_y`），上限 `max(0, 全文排版高 + 上下 padding - 视口高)`。
- **结构要点：`TouchArea` 必须是 `TextInput` 的祖先**——滚轮在 TextInput 忽略后沿父链冒泡，兄弟元素收不到；该 TouchArea 同时承接空白区点击。
- 置尾光标偏移按 **UTF-8 字节**传 `set-selection-offsets`（Slint 内置语义），Slint 侧无字节长度内建，由 Rust 维护 `draft-byte-len` 供给；用字符数会让中文光标落在字符中间。
- 光标跟随滚动：`scroll-y = clamp(cursor.y + 16 - 8, 0, max)`；载入条目时 `scroll-y` 复位、跟随逻辑自动把视口推到置尾光标处。
- 编辑区与标签行不画灰底圆角容器——窗口即一张纸的平铺语言；但编辑区容器的不透明背景（`canvas-default`）**必须保留**：软渲擦除光标闪烁按「重画脏区内 items」进行，无背景则闪烁区无底色可画。图片/文件预览保留容器（图片需要界定）。
- 通知/错误条：`height` 条件动画（`cond ? 36px : 0px` + `animate height` + `clip`），不能用 `visible`——Slint 布局引擎不读 visible，隐藏仍占位，会让下方 flex 区高度跳变。

## tooltip 与状态呈现

- 静态提示锚点 = 按钮中心、气泡按自身宽度水平居中（左缘对齐在气泡宽于按钮时观感歪斜）。
- 气泡定位以**客户区原点**（`win32_ext::client_origin`）为宿主基准：`GetWindowRect` 含非客户区，带框窗口上按它对齐会偏移一个标题栏高度，叠加越界翻转后气泡完全脱离按钮。
- 未保存提示三重：标题栏 `●`（动态绑定 `title`）、底栏未保存点、保存按钮禁用态。
- 底栏显示「第 x 行 · 第 y 列」（1 起、列按字符数中英文同权），依赖 TextInput 未公开属性 `cursor-position_byte-offset` 回调 Rust 换算；`set-selection-offsets` 以 TriggerCallbacks 触发光标回调，载入置尾时行列与视口滚动自动同步。

## 已知绕行（上游依赖，标注移除条件）

- **首行文字上方白点**：非整数 DPI 缩放（125% 实测）下软件渲染器把光标条顶端画出 TextInput 元素几何 1-2px（parley 行盒顶高于元素顶），而增量重绘的脏区只认元素几何——越界段「画得上、擦不掉」，在光标列位上方残留孤立白点。100% 缩放整像素对齐不触发。绕过：TextInput 外包一层顶边与其重合的裁剪容器（`clip: true`），越界段在绘制管线直接被裁、不依赖底色（早期的同色遮罩条方案与透明底不兼容，已退役）；上游修复后可整体移除。
- **整帧重绘覆盖层**：`SLINT_DESTROY_WINDOW_ON_HIDE` 下窗口 hide→show（表面销毁重建）后，软渲按 softbuffer buffer age 选重绘模式，新 buffer 的 age 不可信、脏区基准指向已不存在的旧帧，静态区域不被重画；软件渲染器无应用侧强制 NewBuffer API。绕过：show 前翻转属性增删一层全窗口覆盖元素，使所有区域与上次场景不同，脏区覆盖全窗；成本为每帧一次全窗软件绘制（普通带框窗，无 60fps 压力）。上游修复 buffer-age 判断后整体移除。

## 编辑体验基线

- 标准键位走 Slint TextInput 内置（Windows 编辑控件惯例全套）：Ctrl+Z 撤销 / Ctrl+Y 重做 / Ctrl+A/C/X/V、方向键、Home/End、Ctrl+Home/End、Ctrl+←/→ 按词跳转、Shift 选区、Ctrl+Delete/Backspace 按词删除、双击选词、三击选段；IME 组词作为原子单元进撤销栈。
- 内置撤销栈合并粒度（相邻连续插入合并、换行/删除/粘贴断开、输入后清空重做栈）满足本窗口场景，不自研撤销。
- 按钮无键盘焦点态（TouchArea 限制）：Tab 穿梭只达编辑区 ↔ 标签输入框；高频动作有全键盘路径（Ctrl+S 保存、Esc 关闭、两段式删除单击直达），按钮 Tab 化留给通用按钮组件立项。
- 图片预览 contain 缩略 + 右下角「打开」胶囊按钮交系统查看器细看；文件条目路径行点击用系统默认程序打开（hover 描边 + 文字提亮 + pointer 光标提示可点），统一走 `core::platform::windows::shell_open`。
- 明确不做：Insert 覆盖模式、自动保存（手动保存 + 脏确认是本窗口设计）、图片滚轮缩放/拖拽平移（预览定位快速识别，「打开」承接细看）。
