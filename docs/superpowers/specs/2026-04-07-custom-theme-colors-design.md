# 自定义主题颜色设计

日期：2026-04-07

## 1. 背景

当前项目已经具备稳定的浅色 / 深色主题切换能力：

- 前端通过 `themeMode -> resolvedTheme(light | dark)` 决定当前主题。
- 大部分界面颜色集中收敛在 [src/index.css](C:/repos/floatpaste/src/index.css) 的 `pg-*` 语义 token。
- Picker、Search、Settings、Editor 与 Tooltip 的视觉层都在复用这套 token。

现状的问题不是“没有主题”，而是“主题颜色被写死”：

- 窗口背景色固定。
- 卡片背景色固定。
- 强调色固定为蓝色。
- Tooltip 只能跟随当前硬编码主题，而不能跟随用户自定义配色。

用户这次的核心诉求不是给单条记录上色，而是让整套界面支持按主题分别配置颜色：

- 浅色主题一套颜色。
- 深色主题一套颜色。
- 至少覆盖窗口背景、卡片背景。
- 最好支持强调色。
- Tooltip 需要一起变化。
- 设置页先使用十六进制颜色值输入，不做复杂取色器。

## 2. 目标

### 2.1 主要目标

- 为浅色主题和深色主题分别提供独立配色配置。
- 每套主题至少支持：
  - `窗口背景色`
  - `卡片背景色`
  - `强调色`
- 让 Picker、Search、Settings、Editor、Tooltip 在当前主题下统一读取同一套自定义颜色。
- 保持现有主题模式切换逻辑不变，只增加“主题下的颜色变体”能力。
- 设置页中提供简洁、可靠的十六进制输入方式。

### 2.2 非目标

- 不实现每条剪贴记录的单独颜色配置。
- 不在本轮引入图形化取色器、吸管或调色板 UI。
- 不把所有 token 都暴露给用户单独配置。
- 不在本轮重写整套视觉系统或重新设计所有组件样式。

## 3. 方案结论

本次采用：

`浅色 / 深色各自配置 windowBg、cardBg、accent 三个基础颜色，再由运行时自动派生其余语义 token`

也就是：

- 用户直接输入 6 个十六进制颜色值：
  - `light.windowBg`
  - `light.cardBg`
  - `light.accent`
  - `dark.windowBg`
  - `dark.cardBg`
  - `dark.accent`
- 组件层继续使用 `--pg-canvas-default`、`--pg-canvas-subtle`、`--pg-accent-fg` 等语义 token。
- 当前主题解析完成后，由运行时把用户颜色映射到这些 token 上。
- Tooltip 不额外配置独立颜色，而是复用当前主题下的窗口 / 卡片 / 强调色语义。

不采用：

- 只支持窗口背景和卡片背景，不支持强调色。
- 为 Tooltip 单独暴露独立颜色输入。
- 只存一套颜色，再自动生成浅色 / 深色版本。

原因如下：

1. 用户已经明确要求浅色和深色分别配置。
2. 仅改背景、不改强调色，会留下明显的视觉割裂。
3. Tooltip 单独配置会增加输入负担，也容易与主界面脱节。
4. 以“三个基础颜色 + 自动派生”为核心，能在控制复杂度的同时覆盖大多数感知点。

## 4. 设计原则

### 4.1 用户只配置少量基础色，系统负责派生

用户最关心的是窗口底色、卡片底色和整体强调感，而不是每一个 hover、border、selection 都逐项输入。因此本次只暴露最少但高价值的基础色，其余 token 由代码自动推导。

### 4.2 组件继续依赖语义 token，而不是直接读设置

项目现有结构已经把大部分组件收敛到 `pg-*` token 上。本次不让 Picker、Search、Settings 等组件直接读 `customColors`，而是继续走 token，这样改动集中、风险更低，也更容易覆盖 Tooltip。

### 4.3 浅色 / 深色是两套显式配置，不做“猜测式换算”

用户已经明确希望浅色主题和深色主题分别一套颜色，因此系统不尝试从一套颜色自动推导另一套主题的版本。两套主题的颜色输入和值存储彼此独立。

### 4.4 Tooltip 必须是同一主题系统的一部分

Tooltip 不是额外的浮层皮肤，而是界面主题的延伸。它的背景、边框、标签、阴影和强调信息应复用同一套自定义 token，而不是保留老的蓝灰默认样式。

## 5. 数据模型设计

### 5.1 前端 `UserSetting` 扩展

在 [src/shared/types/settings.ts](C:/repos/floatpaste/src/shared/types/settings.ts) 中新增：

```ts
export type ThemeColorPalette = {
  windowBg: string;
  cardBg: string;
  accent: string;
};

export type CustomThemeColors = {
  light: ThemeColorPalette;
  dark: ThemeColorPalette;
};
```

并在 `UserSetting` 中新增：

```ts
customThemeColors: CustomThemeColors;
```

### 5.2 Rust `UserSetting` 扩展

在 [src-tauri/src/domain/settings.rs](C:/repos/floatpaste/src-tauri/src/domain/settings.rs) 中新增与前端对齐的数据结构：

- `ThemeColorPalette`
- `CustomThemeColors`
- `UserSetting.custom_theme_colors`

字段继续使用 `camelCase` 序列化，以保持前后端桥接一致。

### 5.3 默认值

默认值直接取当前主题系统中的现有基础色，保证升级后视觉不突变：

- 浅色：
  - `windowBg = 当前浅色 --pg-canvas-default`
  - `cardBg = 当前浅色主要卡片面`
  - `accent = 当前浅色 --pg-blue-5`
- 深色：
  - `windowBg = 当前深色 --pg-canvas-default`
  - `cardBg = 当前深色主要卡片面`
  - `accent = 当前深色 --pg-accent-fg`

这意味着老配置文件即使没有新字段，也会自动回退到当前产品现状，不需要显式迁移脚本。

### 5.4 数据兼容与清洗

`UserSetting::sanitized()` 需要增加以下规则：

- 对每个颜色值执行 `trim()`。
- 仅接受 `#RRGGBB` 格式。
- 非法值回退到对应主题的默认值，而不是保留脏数据。
- 大小写统一标准化为大写或小写，其中一种固定格式即可。

前端也要做同样的轻校验，但最终数据兜底以 Rust 侧为准。

## 6. 主题运行时设计

### 6.1 继续保留现有主题解析流程

[src/shared/theme.ts](C:/repos/floatpaste/src/shared/theme.ts) 中现有逻辑负责：

- 解析 `themeMode`
- 监听系统深浅变化
- 给根节点写入 `html.dark`、`data-theme`、`color-scheme`

本次不改变这条主链路。

### 6.2 新增“颜色应用层”

在主题解析之后，新增一个专门的运行时颜色应用函数，例如：

- `applyCustomThemeColors(resolvedTheme, customThemeColors)`

它的职责是：

1. 根据 `resolvedTheme` 选出 `light` 或 `dark` 配色。
2. 把这组三色写到根节点 CSS 变量。
3. 基于这组三色派生更多语义 token。

建议仍然放在主题相关的共享层，而不是散落在 Settings 页面内部，这样 Search、Picker、Tooltip 也能自然继承。

### 6.3 token 映射策略

本次不推翻 [src/index.css](C:/repos/floatpaste/src/index.css) 的默认定义，而是在其上增加“运行时可覆盖”的 token 层。

新增基础运行时变量，例如：

- `--pg-user-window-bg`
- `--pg-user-card-bg`
- `--pg-user-accent`
- `--pg-user-accent-rgb`

然后把现有语义 token 映射为：

- `--pg-canvas-default` -> `windowBg`
- `--pg-canvas-subtle` -> `cardBg`
- `--pg-canvas-inset` -> 由 `windowBg` / `cardBg` 派生
- `--pg-accent-fg` -> `accent`
- `--pg-accent-emphasis` -> `accent`
- `--pg-accent-subtle` -> `accent` 的低透明度版本
- `--pg-border-accent` -> `accent`

### 6.4 自动派生规则

为了避免只替换 3 个 token 后界面层次塌掉，需要补一层稳定派生：

- `canvas-default`：直接使用 `windowBg`
- `canvas-subtle`：直接使用 `cardBg`
- `canvas-inset`：按当前主题模式对 `windowBg` 做轻微偏移
  - 浅色主题：略微更亮或更靠近 `windowBg`
  - 深色主题：略微更暗或更靠近背景层
- `border-default` / `border-muted` / `border-subtle`：
  - 基于 `cardBg` 与 `windowBg` 的相对明度差生成
  - 目标是保证卡片边界仍可见，而不是直接沿用旧灰色
- `accent-subtle`：
  - 使用 `accent` 对应的 `rgba(..., 0.12~0.18)`，具体透明度可按深浅主题分别设定

本次不要求“完全色彩科学化”，但要求结果稳定、可预测、跨浅深主题都能保留层次。

### 6.5 运行时覆盖优先级

优先级明确如下：

1. [src/index.css](C:/repos/floatpaste/src/index.css) 提供默认 token。
2. `resolvedTheme` 决定当前是 light 还是 dark 基底。
3. `customThemeColors` 覆盖当前主题下的运行时变量。

也就是说，没有自定义值时仍然回退到默认主题；有自定义值时只覆盖当前主题相关的 token，不改变另一主题的数据。

## 7. 组件与窗口映射

### 7.1 窗口背景

以下窗口 / 视图的主体背景统一走当前主题的 `windowBg`：

- Settings
- Search
- Editor
- Manager（若当前仍存在对应视图）
- 非透明区域的 Picker 容器底色

对于透明 Picker 窗口，本次不改变透明机制本身，只改变其内部承载容器的背景语义。

### 7.2 卡片背景

以下常见界面单元统一走当前主题的 `cardBg`：

- Settings 中的 `SettingCard`
- Search / Picker 列表项的实心卡片底色
- 错误提示以外的普通信息面板
- Tooltip 主体容器

### 7.3 强调色

以下交互元素统一走当前主题的 `accent`：

- 选中态边框 / 底色强调
- 聚焦 outline / ring
- 主按钮和激活态标签
- 导航 active 态
- Tooltip 中的类型标签或强调信息

### 7.4 Tooltip 跟随策略

Tooltip 不新增独立设置项。

Tooltip 统一遵循当前 `resolvedTheme` 下的自定义 token：

- 主背景优先使用 `cardBg`
- 边框颜色由当前 `cardBg` / `windowBg` 派生
- 强调标签使用 `accent`
- 文本颜色继续沿用现有前景语义 token

如果 Tooltip 当前通过 `showTooltip(..., theme)` 只接收 `light | dark`，则保持这一接口不变。Tooltip 前端页面只需要根据主题读取根节点上的运行时 token，而不是额外通过 IPC 传递颜色值。

## 8. 设置页交互设计

### 8.1 放置位置

在 [src/features/settings/SettingsShell.tsx](C:/repos/floatpaste/src/features/settings/SettingsShell.tsx) 的 `外观` 分组中新增一张独立设置卡片：

`自定义颜色`

它位于“界面主题”之后，“速贴窗口显示位置”之前或之后都可以，但必须与主题相关设置集中放置。

### 8.2 表单结构

该卡片内部拆成两个子分区：

- `浅色主题`
- `深色主题`

每个子分区提供 3 个输入项：

- `窗口背景色`
- `卡片背景色`
- `强调色`

每个输入项：

- 使用普通文本输入框
- 占位符采用 `#RRGGBB`
- 下方提供简短提示，例如“示例：`#EFF2F5`”

### 8.3 输入规则

输入体验按“宽松编辑，失焦 / 保存时校验”的思路设计：

- 编辑过程中允许用户临时输入不完整值。
- 当自动保存触发前，前端会先做格式校验。
- 非法值不提交到后端，并在当前输入项下显示轻量错误提示。
- 当用户改回合法值后恢复自动保存。

这样可以避免用户输入到一半时被强制纠正，也避免把无效颜色写入设置。

### 8.4 预览策略

本轮不额外实现复杂实时配色预览面板。

预览依赖当前应用本身：

- 当前主题对应的窗口在保存成功后立即应用新颜色。
- 如果用户当前处于浅色主题，则优先能看到浅色配置的实时结果。
- 深色配置则在切换到深色模式后生效。

如果后续需要更直观的对比，可在下一轮补轻量示意块，但不属于本轮必做项。

### 8.5 恢复默认

建议在 `自定义颜色` 卡片中提供一个轻量操作：

- `恢复当前主题默认颜色`
- 或 `恢复全部默认颜色`

推荐本轮至少提供：

- `恢复全部默认颜色`

这样用户在输入混乱后可以快速回到项目默认视觉，而不用手动查默认 hex 值。

## 9. Tooltip 与桥接层设计

### 9.1 前端 Tooltip HTML 不直接内联固定颜色

[src/features/picker/tooltipHtml.ts](C:/repos/floatpaste/src/features/picker/tooltipHtml.ts) 负责拼装内容结构，但不应该内联写死背景色、边框色和强调色。

本次要求 Tooltip 页面继续依赖共享 CSS 变量：

- 内容 HTML 只输出结构和语义 class。
- 样式层从当前主题 token 中取色。

### 9.2 `showTooltip` 接口保持轻量

[src/bridge/commands.ts](C:/repos/floatpaste/src/bridge/commands.ts) 里的：

```ts
showTooltip(requestId, x, y, html, theme)
```

可以继续只传 `theme = "light" | "dark"`。

原因是：

- Tooltip 需要知道深浅主题上下文。
- 具体颜色应由 Tooltip 页面自己通过全局 token 读取。
- 不必把 6 个颜色字段通过每次 hover IPC 传来传去。

### 9.3 Tooltip 页面颜色同步

Tooltip 页面需要在显示前拥有与当前应用一致的 token。

推荐实现方式：

- Tooltip 页面和主界面一样，在加载时读取设置并应用主题颜色；
或
- 在 `window.showTooltip(...)` 触发时，同时同步当前主题与当前颜色 token。

本轮优先推荐后者的简化版：

- Tooltip 仍由 `theme` 决定 `html.dark` 切换。
- 同时在 show 阶段把当前主题对应的运行时 token 写入 tooltip 页面根节点。

这样 Tooltip 不需要独立订阅设置变更，也能在每次展示时获得最新配色。

## 10. 错误处理与边界

### 10.1 非法颜色输入

当输入值不是合法十六进制颜色时：

- 当前字段显示错误提示。
- 自动保存不提交本次 payload。
- 其他合法字段不受影响。

### 10.2 老配置兼容

当旧配置文件中不存在 `customThemeColors` 时：

- 前后端都自动回退到默认值。
- 不视为错误。
- 一旦用户保存设置，新字段自然写入。

### 10.3 Tooltip 同步失败

如果 Tooltip 未能同步最新 token：

- 至少仍需保证它能退回当前默认主题颜色。
- 不应因为 tooltip 颜色同步失败影响 hover、定位或隐藏逻辑。

### 10.4 对比度风险

用户可以输入任意十六进制颜色，这天然会带来可读性风险。本轮不限制用户只能输入“安全颜色”，但要做两层基础保护：

- 文本色继续沿用浅 / 深主题既有前景语义。
- 派生边框和 subtle 背景时，优先选择能保留分层的保守算法。

若用户输入极端颜色导致对比不足，本轮允许存在“用户自担的视觉风险”，但不能出现界面结构完全消失或 tooltip 不可见的情况。

## 11. 实现边界

### 11.1 需要改动的主要层

- [src/shared/types/settings.ts](C:/repos/floatpaste/src/shared/types/settings.ts)
- [src/shared/theme.ts](C:/repos/floatpaste/src/shared/theme.ts)
- [src/features/settings/SettingsShell.tsx](C:/repos/floatpaste/src/features/settings/SettingsShell.tsx)
- [src/index.css](C:/repos/floatpaste/src/index.css)
- [src/bridge/commands.ts](C:/repos/floatpaste/src/bridge/commands.ts)（如需桥接字段同步）
- [src-tauri/src/domain/settings.rs](C:/repos/floatpaste/src-tauri/src/domain/settings.rs)
- [src-tauri/src/repository/sqlite_repository.rs](C:/repos/floatpaste/src-tauri/src/repository/sqlite_repository.rs)（仅确认默认序列化与加载兼容，无需额外 schema 迁移）
- Tooltip 相关样式 / 注入位置

### 11.2 不应扩散的范围

- 不新增数据库表。
- 不修改剪贴记录数据模型。
- 不引入独立“主题管理器”窗口。
- 不把所有现有组件逐个改成直接读取颜色字段。

## 12. 验证方案

### 12.1 自动验证

前端改动后至少执行：

`./scripts/win-pnpm build`

如果 Rust 设置模型被改动，额外执行：

`./scripts/win-cargo test`

重点覆盖：

- `UserSetting` 新字段的默认值与反序列化兼容
- 非法颜色值回退逻辑
- 老配置缺少新字段时的默认回退

### 12.2 手动验证

至少验证以下场景：

1. 在浅色主题下修改窗口背景色，Settings / Search / Picker 的对应背景是否立即变化。
2. 在浅色主题下修改卡片背景色，设置卡片与列表卡片是否统一变化。
3. 在浅色主题下修改强调色，按钮、选中态、focus ring、导航 active 是否同步变化。
4. Tooltip 在浅色主题下是否跟随新颜色。
5. 切换到深色主题后，浅色配置不串色，深色配置独立生效。
6. 输入非法 hex 值时是否阻止保存并显示错误提示。
7. 恢复默认后，界面是否回到当前项目原始视觉。

### 12.3 最小状态矩阵

| 场景 | 预期 |
|------|------|
| 旧配置首次加载 | 自动回退默认颜色，无报错 |
| 浅色主题编辑浅色配色 | 当前界面立即更新 |
| 浅色主题编辑深色配色 | 保存成功，但当前界面不立刻切换 |
| 深色主题编辑深色配色 | 当前界面立即更新 |
| 非法 hex 输入 | 不保存，字段有错误提示 |
| Tooltip 展示 | 与当前主题配色一致 |

## 13. 风险与取舍

### 13.1 主要风险

- 用户输入极端颜色导致对比度下降。
- 若派生规则过弱，卡片与窗口边界可能不明显。
- 若派生规则过强，不同组件可能看起来“颜色跳得太多”。

### 13.2 风险控制

- 用户只输入 3 个基础色，缩小可变范围。
- 组件继续依赖语义 token，减少散点式样式漂移。
- Tooltip 不单独配置，避免主题分叉。
- 默认值严格复用当前线上视觉，保证回退路径稳定。

## 14. 实施摘要

本次设计为 FloatPaste 增加“按浅色 / 深色主题分别配置自定义颜色”的能力。

用户只需要为每个主题输入 `窗口背景色`、`卡片背景色`、`强调色` 三个十六进制颜色值；系统在运行时将它们映射到现有 `pg-*` 语义 token，并自动派生边框、弱高亮等辅助颜色。这样 Settings、Picker、Search、Editor 与 Tooltip 都能在不改动核心结构的前提下，统一获得同一套自定义配色能力。
