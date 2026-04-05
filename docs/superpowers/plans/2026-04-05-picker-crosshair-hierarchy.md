# Picker 结构化准星层级 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Picker 在不展开内容、不恢复底部快捷键提示的前提下，把当前选中项重构为唯一明确的视觉中心。

**Architecture:** 改动集中在 `src/features/picker/PickerShell.tsx` 的 `STYLES` 和对应 JSX 结构，通过收紧头部、强化选中项、压低未选中项和重排元信息来建立“结构化准星”层级。实现不引入新组件、不改交互语义，也不扩大到其他窗口。

**Tech Stack:** React, TypeScript, Tailwind CSS, Primer design tokens

**Spec:** `docs/superpowers/specs/2026-04-05-picker-crosshair-hierarchy-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `src/features/picker/PickerShell.tsx` | 调整头部状态区、列表项样式、选中态层级、数字键徽标和元信息结构 |

## Chunk 1: Picker 视觉层级重构

### Task 1: 收敛顶部区域，让列表成为第一视觉落点

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`（`STYLES.header`、`STYLES.headerDot`、`lastMessage` 渲染区域）

- [ ] **Step 1: 调整头部容器样式，降低标题区存在感**

将 `src/features/picker/PickerShell.tsx` 中 `STYLES.header` 从偏“工具栏”的表现，改成更轻的状态条：

- 保留当前高度级别，但减少对比和装饰感
- 让 `FloatPaste` 标题更像工具标识，而不是内容主角
- 不新增额外按钮或二级信息

具体实现方向：

```tsx
header:
  "flex shrink-0 items-center justify-between border-b border-pg-border-subtle/80 bg-pg-canvas-default px-3 py-1.5"
```

可根据最终效果微调，但目标必须保持一致：头部退到辅助层。

- [ ] **Step 2: 收敛品牌锚点和状态反馈**

在同一文件中：

- 调整 `STYLES.headerDot`，让它更像识别点而不是高亮灯
- 将 `lastMessage` 从 `animate-pulse` 改成稳定的状态样式
- 保留消息可见性，但避免它在打开 Picker 时抢过列表注意力

当前：

```tsx
<span className="ml-2 animate-pulse text-[10px] font-medium text-pg-favorite">
  {lastMessage}
</span>
```

目标方向：

```tsx
<span className="ml-2 rounded-sm bg-pg-accent-subtle px-1.5 py-0.5 text-[10px] font-medium text-pg-accent-fg/85">
  {lastMessage}
</span>
```

如最终证明黄色状态更合适，也可保持状态色，但必须移除脉冲感。

- [ ] **Step 3: 运行前端构建，确认头部调整未破坏编译**

Run: `./scripts/win-pnpm build`  
Expected: 构建通过，无 TypeScript 或 Vite 错误


### Task 2: 把选中项重构为“锁定槽”

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`（`STYLES.itemButton`、`STYLES.itemContent`、列表项按钮结构）

- [ ] **Step 1: 提高选中项与普通项的层级差**

调整 `STYLES.itemButton`：

- 选中项边框更明确，但不做厚描边
- 选中项保留阴影，但让它更像聚焦层而不是浮起卡片
- 未选中项边框与背景更平，减少竞争

当前选中态：

```tsx
"bg-pg-accent-subtle border-pg-accent-fg/30 shadow-[0_2px_6px_rgba(var(--pg-shadow-color),0.25)]"
```

建议实现方向：

```tsx
"border-pg-accent-fg/40 bg-pg-accent-subtle shadow-[0_1px_0_rgba(var(--pg-shadow-color),0.14),inset_0_0_0_1px_rgba(var(--pg-blue-5-rgb),0.08)]"
```

普通态和收藏态也要同步变平，避免“每一项都像主角”。

- [ ] **Step 2: 提升正文优先级，降低未选中项正文竞争力**

调整 `STYLES.itemContent`：

- 选中项正文使用更稳的前景色和更清晰的字重
- 未选中项正文略退，仍保持可读
- 保持不展开内容，不新增摘要区

建议方向：

```tsx
itemContent: (selected: boolean, favorited: boolean) =>
  `${selected ? "text-pg-fg-default font-semibold" : "text-pg-fg-default/80"} line-clamp-4 text-[13px] leading-[1.55] tracking-tight ...`
```

说明：
- 若收藏项需要保留轻微强化，可在未选中态只叠加到 `font-medium`
- 但不能让收藏项在未选中时超过当前选中项

- [ ] **Step 3: 调整列表项内部结构，确保正文先于所有辅助信息被读到**

检查并微调 JSX 结构：

- 图片缩略图仍保留，但不能压过正文
- 文本与图片的水平节奏要更稳
- 主文案块与元信息块之间留出更明确的上下层次

如有必要，可将正文容器从简单 `span` 提升为：

```tsx
<div className="min-w-0 flex-1">
  <span className={...}>{item.contentPreview}</span>
</div>
```

前提是不要顺手演变成展开式详情布局。

- [ ] **Step 4: 运行前端构建，确认选中态改造未破坏编译**

Run: `./scripts/win-pnpm build`  
Expected: 构建通过，无 TypeScript 或 Vite 错误


### Task 3: 强化数字键徽标与元信息的“确认层”角色

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`（`STYLES.kbdBadge`、`STYLES.typeBadge`、列表项元信息区域）

- [ ] **Step 1: 将数字键徽标提升为命中标记**

调整 `STYLES.kbdBadge`：

- 选中态更像“当前命中编号”
- 非选中态降低存在感
- 保持体积克制，不做夸张高亮块

建议方向：

```tsx
kbdBadge: (selected: boolean) => `inline-flex h-[18px] min-w-[18px] ... ${
  selected
    ? "bg-pg-accent-fg text-pg-fg-on-emphasis shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)]"
    : "bg-pg-canvas-subtle text-pg-fg-subtle"
}`
```

- [ ] **Step 2: 压低类型 badge 和元信息行的竞争力**

调整 `STYLES.typeBadge` 和元信息行 className：

- 选中项元信息应更清楚，但仍然只是确认层
- 非选中项元信息更淡、更紧凑
- 时间、来源、收藏星标都要服务正文，而不是与正文并列抢注意力

当前元信息行在 JSX 中是：

```tsx
className={`flex w-full items-center gap-2 text-[10px] leading-none transition-colors ${
  isSelected ? "text-pg-accent-fg/80" : "text-pg-fg-subtle"
}`}
```

建议改为更明确的两级确认层，例如：

```tsx
className={`flex w-full items-center gap-2 text-[10px] leading-none ${
  isSelected ? "text-pg-fg-muted" : "text-pg-fg-subtle/90"
}`}
```

注意：
- 这里不要把选中项元信息做成整排蓝色，否则会和正文形成并列主层
- 收藏星标可保留状态色，但尺寸和亮度要服从整排信息

- [ ] **Step 3: 手动验证“1 秒内能锁定当前项”**

Run: `./scripts/win-pnpm dev`  
在浏览器预览中检查：

1. 打开 Picker 后，注意力是否先落在当前选中项
2. 上下切换时，数字键徽标是否帮忙确认当前项
3. 未选中项是否退后但仍可快速扫读
4. 收藏项在未选中时是否仍清楚但不抢主位

- [ ] **Step 4: 执行最终构建验证**

Run: `./scripts/win-pnpm build`  
Expected: 构建通过

- [ ] **Step 5: 提交**

```bash
git add src/features/picker/PickerShell.tsx
git commit -m "feat(picker): 强化选中项层级，改为结构化准星"
```

## 执行说明

- 本计划不要求修改 `src/index.css`，除非在实现中证明 `PickerShell.tsx` 内无法完成层级目标
- 本计划不包含自动化前端测试新增，因为当前仓库没有独立前端测试框架
- 视觉验收必须围绕一个问题判断：`打开 Picker 后，用户是否先看到当前会贴哪条`
