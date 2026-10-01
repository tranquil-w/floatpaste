---
status: accepted
---

# 编辑窗口重设计与标签输入

编辑窗口沿用旧壳骨架复刻而来，存在三个确认的体验痛点：标签建议浮层出现/消失是硬切换；已选标签与内嵌输入框同处固定高度的条内、标签少时大片空白；整帧重绘覆盖层（force-repaint）是黑盒 workaround。本 ADR 记录重设计的选型决策；现行交互规格与实现约束见 [editor-window.md](editor-window.md)。

## 决策

- **标签收纳放弃换行，改多选面板**：Slint 无 FlowLayout，声明式换行不可行，自实现需 Rust 侧字体度量 + 行分组两阶段渲染，复杂度与穿帮风险不值。标签行只留已选 chips 与「+ 添加标签」入口，添加/搜索/创建收敛到下拉面板（垂直单列 Flickable，完全绕开换行约束）。
- **编辑区滚动弃用 Flickable，改自管滚动**：Flickable 滚轮在此结构下始终不可靠，且 `interactive: true` 时 pressed 阶段为拖拽滚动判定把事件 Intercept（源码 `input_event_filter_before_children`），子元素收不到点击——内容可滚时点击失效、不可滚时正常。标签行、文件列表、标签面板三处 Flickable 均 `interactive: false`（非滚轮事件完全透传）。
- **整帧重绘保留 force-repaint 覆盖层**：见 editor-window.md「已知绕行」。停屏替代路线因普通窗口的任务栏按钮与 Alt-Tab 残留被否决；全局关闭 `SLINT_DESTROY_WINDOW_ON_HIDE` 牺牲全窗口内存优化，否决。
- **按钮键盘焦点不做**：Slint TouchArea 按钮无焦点态支持；高频动作已有全键盘路径，按钮 Tab 化留给通用按钮组件立项。

## Considered Options

- **标签换行收纳（成熟编辑器主流形态）**：否决——Slint 无 FlowLayout，见上。
- **单行横滚 + 行内输入框**（首版方案）：否决——行内混排输入框宽度难收敛，交互不及成熟编辑器。
- **「已选列表 + 独立输入行」常驻两段式**：否决——纵向空间常驻成本高；最终面板方案保留其精神（已选与输入分离）但按需弹出，不占内容区。
- **整帧重绘改停屏 / 关 DESTROY_ON_HIDE**：否决，见上。

## Consequences

- 标签交互收敛到面板后，`tag-open` 从绑定表达式改为显式状态（打开/关闭分别由「+ 添加」、Esc / 搜索框失焦驱动）；建议数据流（`EditorTagSuggestion`、采纳/补全/移除回调）不变，`tag-escape` 语义改为「关面板 + 焦点回窗口级」。
- 新增 `core::platform::windows::shell_open`（ShellExecuteW "open" 打开路径），供编辑窗口与后续场景复用。
- 行列计算依赖 TextInput 未公开属性 `cursor-position_byte-offset`，Slint 升级若移除该属性则底栏行列退化（编译期即可发现），不影响其他功能。
- 白点遮罩条与 force-repaint 覆盖层均已标注上游修复后的移除条件（editor-window.md「已知绕行」）。
