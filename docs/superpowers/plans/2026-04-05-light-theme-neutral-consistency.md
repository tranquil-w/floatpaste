# 浅色主题跨设备中性一致性修正 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将默认浅色主题从当前暖灰纸感基底收回到更中性的浅色基底，降低不同屏幕对暖感的放大效应，同时保持深色主题和主题切换机制不变。

**Architecture:** 改动只落在前端主题 token 层和与其直接相关的用户文案层。通过调整 `src/index.css` 的浅色 neutral、`--pg-accent-subtle`、`--pg-shadow-color`，统一影响 settings/search/picker/editor 四个窗口；再同步更新设置页里对浅色主题的描述，避免“界面已改中性，文案仍写暖调纸感”的认知冲突。

**Tech Stack:** React, TypeScript, Tailwind CSS, CSS custom properties, Tauri WebView

**Spec:** `docs/superpowers/specs/2026-04-05-light-theme-neutral-consistency-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `src/index.css:5-80` | 浅色 `:root` token 区：neutral、shadow-color、accent-subtle 注释与色值 |
| Modify | `src/features/settings/SettingsShell.tsx:57-71` | 主题模式选项文案，去掉“暖调纸质底色”描述 |

---

## Chunk 1: 浅色 token 校正与文案对齐

### Task 1: 将浅色基础 token 回调到更中性的 Primer 基线

**Files:**
- Modify: `src/index.css:5-80`

- [ ] **Step 1: 调整浅色 `:root` 的 neutral、shadow-color 与 accent-subtle**

将 `src/index.css` 中浅色 `:root` 这一段从当前暖灰纸感版本：

```css
/* ── Primer Light Theme ── */
:root {
  /* Neutral — warm gray (paper-like warmth) */
  --pg-neutral-0: #FFFEFA;
  --pg-neutral-1: #FAF8F4;
  --pg-neutral-2: #F3F0EB;
  --pg-neutral-3: #EBE7E1;
  --pg-neutral-4: #E4DFD8;
  --pg-neutral-5: #DFD9D1;
  --pg-neutral-6: #D5CFC6;
  --pg-neutral-7: #CCC5BB;
  --pg-neutral-8: #868078;
  --pg-neutral-9: #5D574F;
  --pg-neutral-10: #49433B;
  --pg-neutral-11: #3D3730;
  --pg-neutral-12: #2A241D;
  --pg-neutral-13: #241E17;

  /* Shadow color (rgb channels for rgba usage) */
  --pg-shadow-color: 36, 30, 23;

  /* Blue (accent) */
  --pg-blue-0: #ddf4ff;
  ...

  /* Light-theme accent-subtle: warm tone to harmonize with warm neutrals */
  --pg-accent-subtle: #f5eddc;
```

改为更中性的基线版本：

```css
/* ── Primer Light Theme ── */
:root {
  /* Neutral — near-Primer gray for cross-display consistency */
  --pg-neutral-0: #ffffff;
  --pg-neutral-1: #f6f8fa;
  --pg-neutral-2: #eff2f5;
  --pg-neutral-3: #e6eaef;
  --pg-neutral-4: #e0e6eb;
  --pg-neutral-5: #dae0e7;
  --pg-neutral-6: #d1d9e0;
  --pg-neutral-7: #c8d1da;
  --pg-neutral-8: #818b98;
  --pg-neutral-9: #59636e;
  --pg-neutral-10: #454c54;
  --pg-neutral-11: #393f46;
  --pg-neutral-12: #25292e;
  --pg-neutral-13: #1f2328;

  /* Shadow color (rgb channels for rgba usage) */
  --pg-shadow-color: 31, 35, 40;

  /* Blue (accent) */
  --pg-blue-0: #ddf4ff;
  ...

  /* Light-theme accent-subtle: low-intensity blue to keep feedback clear without warming the page */
  --pg-accent-subtle: var(--pg-blue-0);
```

改动要求：
- 只改浅色 `:root`，不要动 `html.dark` 分支
- 保持 `--pg-blue-*`、成功/警告/危险/完成语义色不变
- 保持 `--pg-canvas-*`、`--pg-border-*` 的语义映射不变，让组件自动跟随新的 neutral 基底
- 同步更新注释，明确目标是“cross-display consistency”，不要继续保留“paper-like warmth”表述

- [ ] **Step 2: 自检浅色 token 覆盖面，确认没有遗漏暖调入口**

Run: `rtk rg -n "paper-like warmth|warm tone|#f5eddc|36, 30, 23|#FFFEFA|#FAF8F4|#F3F0EB" src/index.css`
Expected: 无结果，说明旧的暖调注释和关键暖色值已全部移除

- [ ] **Step 3: 更新设置页浅色主题说明文案**

将 `src/features/settings/SettingsShell.tsx` 中 `themeModeOptions` 里的浅色描述从：

```tsx
{
  value: "light",
  label: "浅色",
  description: "暖调纸质底色，适合日常办公。",
},
```

改为：

```tsx
{
  value: "light",
  label: "浅色",
  description: "中性浅色基底，跨设备观感更稳定。",
},
```

要求：
- 只改浅色文案，不改 `system` / `dark` 的说明
- 文案目标是解释“更稳定”，不是再引导用户期待暖纸感

- [ ] **Step 4: 构建验证**

Run: `rtk .\scripts\win-pnpm build`
Expected: 构建通过；无 TypeScript 错误、无 Vite 打包错误

- [ ] **Step 5: 桌面端视觉验证（重点看浅色）**

Run: `rtk .\scripts\win-pnpm tauri dev`
Expected: 应用成功启动，四个窗口可正常打开，主题切换正常

手动验证清单：
1. 设置页浅色模式下，大面积背景从“纸黄感”回到中性浅灰白
2. Picker 列表页的默认卡片、hover、selected 背景不再整体发暖
3. Search 窗口的面板底色、输入框底色、下拉层阴影不再带棕感
4. Editor 窗口的大底、输入区域、弹窗面板保持清晰层次，但不显黄
5. 切回深色模式后，整体视觉与本次改动前保持一致
6. 记录至少一组“当前机器 vs 偏暖机器”的观察备注；如果无法使用第二台设备，至少在当前机器记录修改前后的主观对比

- [ ] **Step 6: 如手工验证仍偏冷，只允许做一次小范围回调**

如果当前机器上浅色主题已经明显偏冷，只允许在 `src/index.css` 的浅色 `:root` 中做一次小范围微调，并遵守以下边界：
- 只允许微调 `--pg-neutral-0` 到 `--pg-neutral-3`
- 每个色值最多向暖偏移一个很小步长，不得重新引入肉眼可感知的米黄
- `--pg-accent-subtle` 保持蓝系，不退回暖米色
- 调完后必须重新执行 Step 2、Step 4、Step 5

- [ ] **Step 7: 提交**

```bash
git add src/index.css src/features/settings/SettingsShell.tsx
git commit -m "style: 将默认浅色主题收回中性基底"
```
