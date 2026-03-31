# Picker 列表项实心卡片背景 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Picker 列表项拥有实心卡片背景，卡片间留有间隙透出容器背景色，形成清晰的三层视觉层次。

**Architecture:** 容器区域使用 `bg-pg-canvas-subtle`，列表项使用 `bg-pg-canvas-default`，利用 4px gap 透出容器色作为分隔线。改动集中在 `STYLES` 对象和一处 JSX div 的 className。

**Tech Stack:** React, Tailwind CSS, Primer 设计 token

**Spec:** `docs/superpowers/specs/2026-04-01-picker-card-background-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `src/features/picker/PickerShell.tsx:41-47` | `STYLES.itemButton` 函数 — 普通和收藏态的背景/边框 |
| Modify | `src/features/picker/PickerShell.tsx:492` | 列表容器外层 div — 添加容器背景 |

---

## Chunk 1: 样式改动

### Task 1: 修改列表项样式和容器背景

**Files:**
- Modify: `src/features/picker/PickerShell.tsx:41-47` (STYLES.itemButton)
- Modify: `src/features/picker/PickerShell.tsx:492` (列表容器外层)

- [ ] **Step 1: 修改 `STYLES.itemButton` 中普通态和收藏态的背景色**

将 `src/features/picker/PickerShell.tsx` 第 41-47 行的 `itemButton` 函数从：

```tsx
itemButton: (selected: boolean, favorited: boolean) => `group relative flex w-full flex-col gap-1 rounded-md px-2 py-1.5 text-left transition-colors border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pg-accent-fg focus-visible:ring-offset-2 ${
  selected
    ? "bg-pg-accent-subtle border-pg-accent-fg/30"
    : favorited
      ? "border-l-[3px] border-l-pg-accent-fg bg-transparent hover:bg-pg-canvas-subtle"
      : "bg-transparent border-transparent hover:bg-pg-canvas-subtle"
}`,
```

改为：

```tsx
itemButton: (selected: boolean, favorited: boolean) => `group relative flex w-full flex-col gap-1 rounded-md px-2 py-1.5 text-left transition-colors border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pg-accent-fg focus-visible:ring-offset-2 ${
  selected
    ? "bg-pg-accent-subtle border-pg-accent-fg/30"
    : favorited
      ? "border-pg-border-subtle border-l-[3px] border-l-pg-accent-fg bg-pg-canvas-default hover:bg-pg-canvas-subtle"
      : "bg-pg-canvas-default border-pg-border-subtle hover:bg-pg-canvas-subtle"
}`,
```

改动说明：
- **普通态**: `bg-transparent border-transparent` → `bg-pg-canvas-default border-pg-border-subtle`（实心背景 + 可见边框）
- **收藏态**: `bg-transparent` → `bg-pg-canvas-default`（实心背景），左边框保留 3px 强调色，其余三边 `border-pg-border-subtle`
- **选中态**: 不变

- [ ] **Step 2: 给列表容器外层添加背景色**

将 `src/features/picker/PickerShell.tsx` 第 492 行从：

```tsx
<div className="flex min-h-0 flex-1 flex-col px-1 py-1.5">
```

改为：

```tsx
<div className="flex min-h-0 flex-1 flex-col rounded-b-md bg-pg-canvas-subtle px-1 py-1.5">
```

改动说明：
- `bg-pg-canvas-subtle` — 容器背景色（比卡片深一级，亮色 `#E6EAEF`，暗色 `#262C36`）
- `rounded-b-md` — 底部圆角（与顶部蓝色渐变条的 `rounded-t-md` 对称）

- [ ] **Step 3: 构建验证**

Run: `rtk pnpm build`
Expected: 编译通过，无 TypeScript 或构建错误

- [ ] **Step 4: 视觉验证（手动）**

Run: `pnpm dev` 或 `pnpm tauri dev`
在浏览器/Tauri 中打开 Picker 面板，检查：
1. 每个列表项都有实心卡片背景
2. 卡片间 4px 间隙透出较深的容器背景色
3. 悬停时卡片背景变为 `canvas-subtle`
4. 选中态蓝色高亮正常
5. 收藏项左侧 3px 蓝色边框 + 实心背景
6. 亮色和暗色主题下都正确

- [ ] **Step 5: 提交**

```bash
git add src/features/picker/PickerShell.tsx
git commit -m "feat(picker): 列表项改为实心卡片背景，提升视觉区分度"
```
