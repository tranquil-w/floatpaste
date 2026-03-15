# Catppuccin 主题收口 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 FloatPaste 的 Catppuccin 主题收口，让 Picker 与 Manager 在统一的颜色、控件、卡片和文字层级语义下工作，并修复用户已指出的深色主题问题。

**Architecture:** 先在全局主题层整理语义化 token，再把共用容器、Picker、Manager 依次迁移到统一样式系统。实现过程中不改功能逻辑，只处理视觉层次、控件样式、轻微结构调整和手工验证。

**Tech Stack:** React 19、TypeScript、Tailwind CSS、Vite、Tauri

---

## 文件结构与职责映射

**现有文件职责**

- [src/index.css](c:/repos/floatpaste/src/index.css)
  负责全局主题变量、窗口级背景、滚动条与全局输入控件基线样式。
- [src/shared/ui/Panel.tsx](c:/repos/floatpaste/src/shared/ui/Panel.tsx)
  负责 Manager 三栏的共同面板外观，是统一窗口层级的最低成本入口。
- [src/features/picker/PickerShell.tsx](c:/repos/floatpaste/src/features/picker/PickerShell.tsx)
  负责 Picker 外壳、顶栏、列表项、选中态、数字快捷键与元信息行。
- [src/features/manager/ManagerShell.tsx](c:/repos/floatpaste/src/features/manager/ManagerShell.tsx)
  负责 Manager 左中右三栏、设置页控件、收藏预览、详情区按钮组与文字层级。

**本次仅修改以下文件**

- Modify: [src/index.css](c:/repos/floatpaste/src/index.css)
- Modify: [src/shared/ui/Panel.tsx](c:/repos/floatpaste/src/shared/ui/Panel.tsx)
- Modify: [src/features/picker/PickerShell.tsx](c:/repos/floatpaste/src/features/picker/PickerShell.tsx)
- Modify: [src/features/manager/ManagerShell.tsx](c:/repos/floatpaste/src/features/manager/ManagerShell.tsx)

**文件体量保护**

- `src/features/manager/ManagerShell.tsx` 已经较大。本轮允许继续在该文件中实现，但若按钮组、设置项卡片、历史列表项的 className 判断继续膨胀，必须优先抽出同文件内局部渲染函数或小组件，避免主题收口进一步恶化可维护性。
- `src/features/picker/PickerShell.tsx` 允许保留单文件，但圆角、高亮、数字键和顶栏层级必须按职责拆开处理，不能混成一段超长 className 调整。

**验证文件 / 命令**

- Verify: `pnpm build`
- Verify: 手工运行 `pnpm dev`
- Optional Verify: `pnpm tauri dev`

## Chunk 1: 全局 token 与面板基调

### Task 1: 整理全局主题 token

**Files:**
- Modify: `src/index.css`

- [ ] **Step 1: 盘点当前重复使用且职责混乱的颜色/表面类**

Run: `rg -n "cp-accent|cp-yellow|cp-surface0|rounded-3xl|border-cp-surface0" src/index.css src/shared/ui/Panel.tsx src/features/picker/PickerShell.tsx src/features/manager/ManagerShell.tsx`

Expected: 输出当前高频类名与变量位置，用于确认哪些值需要升格为语义 token。

- [ ] **Step 2: 在 `src/index.css` 中新增语义化 token**

实现内容：

```css
/* 语义化表面 */
--cp-window-shell: ...;
--cp-panel-surface: ...;
--cp-card-surface: ...;
--cp-control-surface: ...;
--cp-control-surface-hover: ...;

/* 语义化边框 */
--cp-border-soft: ...;
--cp-border-strong: ...;
--cp-focus-ring: ...;

/* 语义化强调 */
--cp-accent-primary: ...;
--cp-accent-primary-strong: ...;
--cp-accent-warm: ...;
--cp-danger: ...;

/* 语义化文本 */
--cp-text-primary: ...;
--cp-text-secondary: ...;
--cp-text-muted: ...;
```

要求：

- 浅色与深色主题都要提供对应值。
- 保留现有 Catppuccin 基础色，但组件层优先改用新语义 token。
- 不删除仍被现有 Tailwind 类引用的老 token，先做兼容过渡。

Expected:

- `src/index.css` 同时拥有表面、边框、强调、危险色、文本层级 5 组语义 token。
- 后续组件实现无需继续直接依赖 `cp-yellow` 决定主结构高亮。

- [ ] **Step 3: 收敛全局基线样式**

修改方向：

- 统一 `body.theme-manager`、`body.theme-picker` 的背景逻辑。
- 优化滚动条底色和悬停态，让深色主题不再出现突兀的亮灰。
- 为 `button / input / textarea / select` 补充更稳定的颜色继承与可编辑基线。

Expected:

- Picker 与 Manager 共享同一套全局背景和输入控件基线。
- 深色主题下滚动条和可编辑控件不会出现亮灰断层。

### Task 2: 调整共用 `Panel` 基调

**Files:**
- Modify: `src/shared/ui/Panel.tsx`
- Verify: `src/index.css`

- [ ] **Step 1: 调整 `Panel` 的共同基调**

在 `src/shared/ui/Panel.tsx` 中把当前：

```tsx
rounded-3xl border border-cp-surface0/60 bg-cp-mantle/70 ...
```

改为基于语义 token 的较弱边框、较稳定面板底色和更克制 hover，目标是：

- 三栏先像同一系统
- hover 不再抢戏
- 深色边框从“描线感”降到“层次感”

Expected:

- 三个 `Panel` 放在一起时，不再出现某一栏明显更厚或更亮。
- hover 仍可感知，但不会改变面板主次关系。

- [ ] **Step 2: 运行构建验证基础主题层未破坏**

Run: `pnpm build`

Expected: 构建成功，无 TypeScript 或 Vite 报错。

- [ ] **Step 3: 手工验证 `Panel` 共同基调**

Run: `pnpm dev`

手工验证：

- 打开 Manager 预览页
- 对比左中右三栏 `Panel` 的边框、底色和 hover
- 确认没有某一栏明显更亮、更厚或 hover 反馈突兀

Expected:

- 三栏并排时属于同一系统。
- hover 反馈可感知，但不会破坏整体主次关系。

- [ ] **Step 4: 提交这一小步**

```bash
git add src/index.css src/shared/ui/Panel.tsx
git commit -m "style: 整理 Catppuccin 全局主题与面板基调"
```

## Chunk 2: Picker 外壳与高亮系统

### Task 3: 重做 Picker 外壳与圆角关系

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`
- Verify: `src/index.css`

- [ ] **Step 1: 记录当前 Picker 的关键视觉点**

手工检查：

- 当前外壳圆角与真实窗口形状是否一致
- 当前高亮项是否仍偏黄
- 当前 `1~9` 标记在选中态下是否可读

Expected: 记录 3 个现状问题，作为改动后比对基线。

- [ ] **Step 2: 调整 Picker 外层壳体**

在 `PickerShell.tsx` 中把最外层容器改成“收敛型圆角”方案：

- 外层圆角比当前更利落
- 外层边框更弱
- 阴影更聚焦在悬浮窗口，而不是发散大光晕
- 顶栏与正文区有轻层次差，但不割裂

目标类结构示例：

```tsx
rounded-[18px] border border-[color:var(--cp-border-soft)] bg-[color:var(--cp-window-shell)] ...
```

Expected:

- 外层窗口看起来更接近工具浮窗，而不是展示卡片。
- 深色模式下边框存在感降低，但轮廓仍清楚。

- [ ] **Step 3: 同步收敛内部列表项圆角**

要求：

- 列表项圆角与外壳圆角属于同一语言
- 当前项和普通项都不能再显得“比窗口本体更圆”
- 当前项 ring 和阴影必须配合新圆角，不得残留旧尺寸

Expected:

- 外层窗口与内部条目不再出现“壳体偏方、内容偏圆”的割裂感。

- [ ] **Step 4: 核对真实窗口形状与 React 外层容器一致**

检查并联动以下事实：

- Tauri 真实窗口形状由后端窗口工具控制
- React 外层容器圆角负责视觉外观
- 两者的圆角尺度必须一致

Expected:

- 在桌面环境中，真实窗口四角与页面壳体视觉一致。
- 不会出现透明尖角、裁切错位或圆角穿帮。

### Task 4: 重做 Picker 高亮、顶栏与数字快捷键

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`
- Verify: `src/index.css`

- [ ] **Step 1: 把 Picker 主高亮从暖色切到冷色**

替换当前选中项相关类：

- `bg-cp-yellow/...`
- `ring-cp-yellow/...`
- `text-cp-yellow/...`

改为基于冷色主强调语义 token 的组合，并保留足够弱的 hover 态。

Expected:

- 当前选中项一眼可见，但不再偏黄。
- hover 与 selected 可稳定区分。

- [ ] **Step 2: 重做数字快捷键 `kbd`**

要求：

- 默认态和选中态都能清晰看到数字
- `kbd` 与列表背景分离，不融成一片
- 选中项下 `kbd` 文字不可再与背景撞色

实现时优先分离这两个层次：

```tsx
isSelected ? "kbd-selected" : "kbd-default"
```

即使最终仍写在 className 里，也要保证语义分开。

Expected:

- 默认态与选中态都能清晰读出数字。
- `kbd` 在深色主题下不会和高亮底色混成一片。

- [ ] **Step 3: 收敛列表元信息行**

调整以下元素层级：

- 类型标签
- 来源应用
- 时间
- 收藏星标

目标：

- 主文本仍是第一层
- 元信息弱化但可读

Expected:

- 类型标签、来源应用、时间、收藏星标明显弱于主文本。
- 即使选中时，元信息也不会和主文本争夺焦点。

- [ ] **Step 4: 单独调整顶栏层级与成功提示**

处理以下元素：

- 品牌点
- 标题 `FloatPaste`
- “资料库”次级入口
- 顶栏成功提示

Expected:

- 顶栏标题与次级入口层级清楚。
- 成功提示可读，但不会抢占顶栏主视觉中心。

- [ ] **Step 5: 运行构建并手工验证 Picker**

Run: `pnpm build`

Expected: 构建成功。

手工验证：

- 运行 `pnpm dev`
- 打开 Picker 预览页
- 确认圆角、选中态、数字快捷键、顶栏提示符合 spec

Expected:

- 手工检查项全部通过，无明显圆角穿帮或高亮撞色。

- [ ] **Step 6: 提交这一小步**

```bash
git add src/features/picker/PickerShell.tsx src/index.css
git commit -m "style: 收敛 Picker 外观与高亮层级"
```

## Chunk 3: Manager 控件、卡片与文字层级

### Task 5: 统一 Manager 输入控件与选择控件

**Files:**
- Modify: `src/features/manager/ManagerShell.tsx`
- Verify: `src/index.css`

- [ ] **Step 1: 搜索并归类 Manager 内的控件类型**

Run: `rg -n "type=\"radio\"|type=\"checkbox\"|textarea|<input|查看全部|保存设置|上一页|下一页" src/features/manager/ManagerShell.tsx`

Expected: 找到需要统一的单选框、复选框、输入框、textarea、次级按钮和分页按钮位置。

- [ ] **Step 2: 统一输入框与 textarea 基调**

调整以下区域：

- 搜索框
- 设置页数字输入框
- 全局快捷键输入框
- 排除应用 textarea
- 详情编辑 textarea

目标：

- 深色主题下不再像单独黑块
- focus 态统一使用冷色焦点环
- 边框只做轻分隔，不做重描线

Expected:

- 搜索框、设置输入框、详情编辑框共享同一套控件表面和焦点态。
- textarea 背景明显融入 Macchiato 表面体系。

- [ ] **Step 3: 统一 radio / checkbox 未选中态与选中态**

要求：

- 未选中态贴近当前表面，不突兀发灰
- 选中态统一用冷色主强调
- 禁用态与可操作态拉开

重点位置：

- 主题选择
- Picker 显示位置选择
- 只看收藏
- 开机自启 / 静默启动 / 恢复剪贴板 / 暂停监听

Expected:

- 未选中态不再突兀发灰。
- 选中态统一使用冷色主强调。
- 禁用态一眼可区分，但不脏不花。

### Task 6: 重做 Manager 次级按钮语义

**Files:**
- Modify: `src/features/manager/ManagerShell.tsx`

- [ ] **Step 1: 调整“查看全部”按钮**

Expected:

- 默认态看起来就是次级按钮，而不是文本链接。
- hover / active 反馈清楚，但不抢主按钮风头。

- [ ] **Step 2: 调整分页按钮**

Expected:

- 默认 / hover / disabled 三种状态易于区分。
- 深色主题下不再像厚重灰块。

- [ ] **Step 3: 调整左栏底部切换按钮**

Expected:

- “历史库 / 设置”切换关系清楚。
- 激活态与未激活态由统一按钮语义表达。

- [ ] **Step 4: 调整详情区非主操作按钮**

处理以下元素：

- 收藏切换按钮
- 非主复制/写入按钮
- 删除按钮之外的次级动作

目标：

- 一眼可识别为可点击操作
- 不再像普通文本或灰块
- 与主按钮、危险按钮形成稳定层级

Expected:

- 默认 / hover / active 下主次关系稳定。
- 危险按钮不会被误识别为普通次按钮。

### Task 7: 收口 Manager 卡片与文字层级

**Files:**
- Modify: `src/features/manager/ManagerShell.tsx`

- [ ] **Step 1: 梳理需要降噪或重排层级的区域**

手工列出以下区域：

- 左栏英雄卡片
- 收藏预览
- 中栏历史列表项
- 右栏详情标题与按钮组
- 详情信息卡
- 空状态文案

Expected: 明确哪些地方是“卡片层级问题”，哪些地方是“文字层级问题”。

- [ ] **Step 2: 收敛左栏与收藏预览**

调整目标：

- 左栏英雄卡片降噪，减少发光和体积感
- 收藏预览卡片回到统一表面语义
- 预览卡内标题、内容、时间拉开层级
- “查看全部”保持次按钮身份，不再回退成文字链接

Expected:

- 左栏品牌感保留，但不再压过中栏和右栏。
- 收藏预览标题、正文、时间一眼可分层。

- [ ] **Step 3: 重做中栏历史列表项**

要求：

- 主文本是第一视觉层
- 类型标签与收藏状态是第二层
- 来源应用、创建时间、最近使用时间是第三层
- 当前选中态改用冷色主强调色
- 列表项边框、背景、标签与文字属于同一控件系统

Expected:

- 列表在未选中和选中两种状态下都层级清楚。
- 冷色选中态与文字层级同时成立，没有相互打架。

- [ ] **Step 4: 收口右栏详情区**

重点处理：

- 顶部标题与小标签
- 收藏 / 写入剪贴板按钮组主次
- 信息卡字段标题
- 详情编辑区说明文案
- 成功提示与危险按钮的语义边界

Expected:

- 详情区标题、字段名、说明和操作形成稳定层级。
- 成功提示、危险按钮、次级按钮三种语义不会混淆。

- [ ] **Step 5: 调整设置页卡片分组与文字层级**

目标：

- 配置项卡片分组保留，但减弱描边
- 配置项标题明确
- 描述文案更像说明，而不是正文
- 帮助文案和状态错误提示层级稳定

Expected:

- 设置页卡片分组仍存在，但不再显得厚重。
- 标题、描述、帮助文案和错误提示层级稳定。

- [ ] **Step 6: 控制 `ManagerShell.tsx` 文件膨胀**

执行边界：

- 如果按钮组、设置项卡片、列表项 className 判断继续增长，优先抽出局部渲染函数或同文件内小组件
- 不做无关重构，但必须避免把新语义继续全部硬塞进单个超长 JSX 区块

Expected:

- `ManagerShell.tsx` 的新增复杂度被局部隔离。
- 不因为本轮主题收口让单文件更难维护。

- [ ] **Step 7: 运行构建并手工验证控件与层级一致性**

Run: `pnpm build`

Expected: 构建成功。

手工验证：

- 打开设置页
- 逐项检查 radio / checkbox / input / textarea / 次级按钮
- 检查左中右三栏和设置页的卡片、标题、说明、元信息、操作文案
- 确认深色主题下边框、焦点态与文字层级统一

Expected:

- Manager 的控件、卡片、文字层级与按钮语义全部通过手工检查。

- [ ] **Step 8: 提交这一小步**

```bash
git add src/features/manager/ManagerShell.tsx src/index.css
git commit -m "style: 收口 Manager 控件卡片与文字层级"
```

## Chunk 4: 最终验证与交付

### Task 8: 最终验证与交付整理

**Files:**
- Verify: `src/index.css`
- Verify: `src/shared/ui/Panel.tsx`
- Verify: `src/features/picker/PickerShell.tsx`
- Verify: `src/features/manager/ManagerShell.tsx`

- [ ] **Step 1: 运行最终构建验证**

Run: `pnpm build`

Expected: 终端输出 Vite 构建成功，且无 TypeScript 报错。

- [ ] **Step 2: 进行最终手工验收**

运行：

```bash
pnpm dev
```

如果桌面运行环境可用，再补充：

```bash
pnpm tauri dev
```

手工验收清单：

- Picker 外壳与内部条目圆角不再冲突
- Picker 选中项和 `1~9` 标记清晰
- 深色边框不再刺眼
- Radio / Checkbox 未选中态符合主题
- 编辑框背景与主题协调
- “查看全部”明显可点击
- 标题、说明、元信息、操作文案层级清晰

Expected:

- 视觉验收项全部通过。
- 如有剩余问题，能明确归类为后续小修，而不是主题系统性缺陷。

- [ ] **Step 3: 清理重复类与明显无效 token**

要求：

- 删除本次改造后已不再使用的重复样式片段
- 不做与本次主题无关的清理

Expected:

- 不留下明显重复的旧 class 组合或废弃 token。

- [ ] **Step 4: 整理最终变更并提交**

```bash
git add src/index.css src/shared/ui/Panel.tsx src/features/picker/PickerShell.tsx src/features/manager/ManagerShell.tsx
git commit -m "style: 完成 Catppuccin 主题收口"
```

- [ ] **Step 5: 输出交付说明**

说明内容至少包含：

- 用户可见变化
- 涉及文件
- 执行过的验证命令
- 仍存在的风险或未覆盖点

