# Search 图片 Tooltip 与统一延时设计

**日期：** 2026-04-05  
**范围：** `Search` 图片项 tooltip 接入，`Picker` / `Search` tooltip 显示延时统一收敛

## 1. 背景

当前仓库中只有 `Picker` 接入了 tooltip 能力，并且触发速度偏快。  
`Search` 虽然已经开始显示图片缩略图，但仍缺少更大的悬停预览与补充信息。

这带来两个问题：

1. `Search` 图片项只能看小缩略图，无法像 `Picker` 一样快速放大确认
2. `Picker` 当前 tooltip 出现过快，扫列表时容易被误触发

本轮目标不是扩展一整套新浮层系统，而是在现有 tooltip 能力上，把 `Search` 的图片项接入进来，并统一两处的悬停节奏。

## 2. 目标

- `Search` 仅在图片项悬停时显示 tooltip
- `Picker` 与 `Search` 统一 tooltip 显示前延时
- 两者共用现有 tooltip 窗口、HTML 生成与定位逻辑
- 鼠标离开、列表切换、窗口关闭时立即隐藏 tooltip

## 3. 非目标

- 不为文本或文件项新增 tooltip
- 不重做 tooltip 的视觉样式
- 不新增第二套 `Search` 专属 tooltip 窗口
- 不修改 tooltip 的基础定位策略

## 4. 方案结论

采用：

- `Search` 只在 `image` 类型条目悬停时触发 tooltip
- `Picker` 保持现有 tooltip 语义，但延时统一加长
- 两边统一使用 `400ms` 作为首版 tooltip 显示前延时

不采用：

- `Search` 所有条目都支持 tooltip
- `Search` 只对当前选中项显示 tooltip
- `Picker` 与 `Search` 使用不同延时值

## 5. 交互规则

### 5.1 Search 的触发范围

- 只有 `item.type === "image"` 的结果项允许触发 tooltip
- 文本项、文件项完全不触发
- 图片项即使已经显示小缩略图，仍然允许 tooltip 出现

### 5.2 延时策略

- `Picker` 与 `Search` 统一使用 `400ms` 延时
- 只有鼠标稳定停留到阈值后才展示 tooltip
- 鼠标快速扫过列表时不应频繁弹出

### 5.3 取消时机

以下情况立即取消 tooltip：

- 鼠标离开条目
- 当前列表切换
- 窗口关闭
- 会话结束

## 6. 实现方式

### 6.1 复用现有 tooltip 能力

继续复用已有实现：

- `showTooltip`
- `hideTooltip`
- `buildTooltipHtml`
- `resolveTooltipShowPosition`

不新建 `Search` 专用 tooltip 组件。

### 6.2 延时常量收敛

将 tooltip 显示前延时提取为共享常量，供 `Picker` 和 `Search` 共用，避免两边维护两份魔法数字。

### 6.3 Search 接入

在 `SearchShell.tsx` 中：

- 为图片项绑定 `onMouseMove`
- 复用当前图片 URL 缓存
- 复用 tooltip request/cancel 机制
- 非图片项直接跳过 tooltip 逻辑

## 7. 影响文件

预计主要修改：

- `src/features/search/SearchShell.tsx`
- `src/features/picker/PickerShell.tsx`

可能新增：

- `src/shared/ui/tooltipConfig.ts` 或其他轻量共享配置文件

## 8. 验证策略

### 8.1 构建验证

前端改动后至少执行：

`./scripts/win-pnpm build`

### 8.2 交互验证

重点检查：

- `Picker` 中悬停图片/文本项时 tooltip 是否明显更晚出现
- `Search` 中只有图片项悬停才会出现 tooltip
- `Search` 中文本/文件项悬停时不会弹 tooltip
- 鼠标离开条目时 tooltip 是否立即关闭
- 快速扫过多条结果时是否不会频繁闪烁

## 9. 验收标准

- `Search` 成功接入图片项 tooltip
- `Search` 非图片项不显示 tooltip
- `Picker` 与 `Search` 使用统一延时
- tooltip 延时明显长于当前实现
- tooltip 取消时机保持干净，不残留
