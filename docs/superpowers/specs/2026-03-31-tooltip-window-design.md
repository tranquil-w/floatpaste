# Tooltip 独立窗口设计

## 背景

Picker 窗口的列表条目目前使用 HTML `title` 属性显示原生 tooltip。该 tooltip 存在以下问题：
- 样式无法自定义，与 Primer 设计系统不一致
- 在 Tauri webview 中行为不可靠（延迟高、偶尔不显示）
- 内容被限制为纯文本，无法展示结构化的元数据信息

## 需求

1. Tooltip 视觉样式与 picker 保持一致（同主题、同配色、同边框/阴影风格）
2. Tooltip 必须能够显示在 picker 窗口之外（不被 picker 裁剪）
3. Tooltip 出现在鼠标右下方
4. 显示完整文本预览 + 元数据（来源应用、时间、类型）

## 约束

- 不得影响 picker 的拖动、缩放、点击和键盘操作
- 不得出现透明遮罩拦截鼠标事件
- 代码改动尽量小，优先复用现有 token/样式变量

## 方案：独立 Tauri Tooltip 窗口

### 架构

```
Picker 窗口 (现有)                    Tooltip 窗口 (新增)
┌──────────────────┐                ┌──────────────────┐
│  ClipItem hover  │──mousemove──→│  独立 Tauri 窗口   │
│  计算屏幕坐标     │──invoke────→│  transparent      │
│                  │              │  无装饰、鼠标穿透   │
└──────────────────┘              └──────────────────┘
```

Picker 的 list item 上监听 `mousemove`，获取鼠标在屏幕坐标系中的位置，通过 Tauri IPC 通知后端将 tooltip 窗口移到鼠标右下方并显示内容。

### 前端改动

**PickerShell.tsx**：
- 在每个 `<button>` 上添加 `onMouseMove` 和 `onMouseLeave` 处理
- `onMouseMove` 时调用 `invoke("show_tooltip", { html, x, y })`
  - `x, y` 通过 `getCurrentWindow().outerPosition()` + 鼠标在 webview 中的偏移计算屏幕绝对坐标
- `onMouseLeave` 时调用 `invoke("hide_tooltip")`
- 移除现有 `title` 属性

**Tooltip 内容**：
- `buildTooltipHtml(item)` 函数将完整文本 + 元数据格式化为 HTML
- 使用内联样式引用 `pg-*` CSS 变量（tooltip 窗口加载相同的 CSS）

**bridge 层**：
- 在 `bridge/commands.ts` 中封装 `showTooltip` 和 `hideTooltip`

### 后端改动

**新增 `services/tooltip_window.rs`**：
- `ensure_tooltip_window(app)` — 懒创建 tooltip 窗口
- `show_tooltip(app, x, y, html)` — 设置内容、定位、显示
- `hide_tooltip(app)` — 隐藏

**Tooltip 窗口属性**：
- `decorations(false)`, `transparent(true)`, `shadow(false)`
- `always_on_top(true)`, `skip_taskbar(true)`
- `resizable(false)`
- `WS_EX_NOACTIVATE` — 不抢焦点
- `WS_EX_TRANSPARENT` — 鼠标事件穿透

**新增命令**（`commands/windows.rs`）：
- `show_tooltip(x: f64, y: f64, html: String)`
- `hide_tooltip()`

### 样式

复用 `pg-*` CSS 变量：

```css
background: var(--pg-canvas-default);
border: 1px solid var(--pg-border-muted);
box-shadow: var(--pg-shadow-lg);
border-radius: 6px;
color: var(--pg-fg-default);
```

### 约束处理

| 约束 | 解决方式 |
|------|----------|
| 不影响拖动/缩放/点击/键盘 | `WS_EX_TRANSPARENT` 鼠标穿透 |
| 不出现透明遮罩 | 独立窗口 + 鼠标穿透 |
| 代码改动小 | ~150 行 Rust + ~50 行 TS |
| 复用 token/变量 | 共享 CSS，复用 `window_utils.rs` |

### 触发时机

- 仅鼠标移动时显示（`onMouseMove`）
- 键盘上下导航时不显示 tooltip
- picker 隐藏时自动隐藏 tooltip
