# Search 图片 Tooltip 与统一延时 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `Search` 仅在图片项悬停时显示 tooltip，并把 `Picker` 与 `Search` 的 tooltip 显示前延时统一加长。

**Architecture:** 复用现有 tooltip 窗口、HTML 生成和定位逻辑，把显示前延时提取成共享常量；`Picker` 只替换延时值，`Search` 增加与图片项绑定的 tooltip 触发/取消逻辑，并复用现有图片 URL 缓存。

**Tech Stack:** React, TypeScript, Tauri, 现有 tooltip bridge

**Spec:** `docs/superpowers/specs/2026-04-05-search-image-tooltip-delay-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Add | `src/shared/ui/tooltipConfig.ts` | 统一 tooltip 延时常量 |
| Modify | `src/features/picker/PickerShell.tsx` | 改为使用共享 tooltip 延时 |
| Modify | `src/features/search/SearchShell.tsx` | 仅为图片项接入 tooltip，复用图片 URL 缓存与定位逻辑 |

## Chunk 1: 统一延时常量

### Task 1: 提取共享 tooltip 延时配置

**Files:**
- Add: `src/shared/ui/tooltipConfig.ts`
- Modify: `src/features/picker/PickerShell.tsx`

- [ ] **Step 1: 新增共享 tooltip 延时常量**

新增一个轻量共享文件，导出：

```ts
export const TOOLTIP_SHOW_DELAY_MS = 400;
```

- [ ] **Step 2: Picker 改为引用共享延时**

把 `PickerShell.tsx` 中当前 tooltip `setTimeout(...)` 的魔法数字替换为共享常量。

## Chunk 2: Search 接入图片 tooltip

### Task 2: 只为图片项接入 tooltip 触发

**Files:**
- Modify: `src/features/search/SearchShell.tsx`

- [ ] **Step 1: 接入 tooltip 所需 ref 与取消逻辑**

在 `SearchShell.tsx` 中补齐：

- tooltip timer ref
- tooltip request id ref
- clear/cancel helper

要求：

- 窗口卸载时清理
- 鼠标离开图片项时立即隐藏

- [ ] **Step 2: 复用现有 tooltip HTML 和定位逻辑**

引入：

- `buildTooltipHtml`
- `resolveTooltipShowPosition`
- `showTooltip`
- `hideTooltip`

并复用 `getImageUrl(...)` / 当前图片 URL 缓存。

- [ ] **Step 3: 只给图片项绑定悬停显示**

要求：

- `item.type === "image"` 时才触发 tooltip
- 文本/文件项完全不触发
- 悬停显示前延时使用共享常量

## Chunk 3: 验证

### Task 3: 构建与交互验证

**Files:**
- Verify: `src/features/picker/PickerShell.tsx`
- Verify: `src/features/search/SearchShell.tsx`

- [ ] **Step 1: 执行前端构建验证**

Run: `./scripts/win-pnpm build`
Expected: 构建通过

- [ ] **Step 2: 手动验证 tooltip 交互**

重点检查：

1. `Picker` tooltip 是否比以前更晚出现
2. `Search` 只有图片项悬停才会出现 tooltip
3. 文本/文件项不会误触发 tooltip
4. 鼠标离开时 tooltip 立即关闭
