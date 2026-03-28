# Primer 主题迁移 + ManagerShell 设置窗口重设计

日期：2026-03-27

## 概述

两项并行变更：
1. 将 ManagerShell 从三栏剪贴板管理界面简化为纯设置窗口
2. 将整体色彩系统从 Catppuccin 迁移为 GitHub Primer 风格

---

## 一、ManagerShell → 设置窗口

### 1.1 移除范围

从 ManagerShell 中删除以下所有内容：

- 三栏布局（收藏侧栏 / 历史列表 / 详情面板）
- 搜索、分页、筛选功能
- 收藏预览
- 剪贴板详情查看与文本编辑
- 删除、收藏、粘贴操作
- 所有相关的 SVG 图标（闪电、星星、时钟等）
- STYLES 常量中 historyItem / favoriteItem / detailEditor / shortcutCard 等

### 1.2 保留范围

- SettingsPanel 的全部设置表单功能（原 729-1003 行）
- 设置相关的 Tauri 事件监听（CLIPS_CHANGED_EVENT 仅保留 settings 失效逻辑）
- useSettingsQuery / useUpdateSettingsMutation

### 1.3 依赖迁移

`manager/queries.ts` 中以下 hook 被 WorkbenchShell 和 EditorShell 引用，需要迁出：

| Hook | 消费方 |
|------|--------|
| `useItemDetailQuery` | WorkbenchShell, EditorShell |
| `useUpdateTextMutation` | EditorShell |
| `invalidateClipQueries` | 被 updateText/delete/favorite/paste 共用 |

**方案**：将这三个导出移到新建的 `src/shared/queries/clipQueries.ts`，然后 WorkbenchShell 和 EditorShell 改为从新路径 import。`manager/queries.ts` 仅保留 `useSettingsQuery` 和 `useUpdateSettingsMutation`。

### 1.4 Store 简化

`manager/store.ts` 当前有 `selectedItemId`、`draftText`、`viewMode`，全部移除后 store 不再需要存在。SettingsPanel 的表单状态由组件内 useState 管理（保持现状）。

**结论**：删除 `manager/store.ts`，删除 `manager/index.ts` 中对 store 的导出。

### 1.5 新 ManagerShell 结构

纯设置页面，单列居中布局：

```
┌─────────────────────────────────────────┐
│  FloatPaste  ·  设置                      │
│  偏好设置会自动保存。                        │
├─────────────────────────────────────────┤
│                                         │
│  ── 快捷键 ──────────────────────────    │
│  [全局快捷键输入框]                         │
│  [搜索窗口快捷键] [启用开关]               │
│                                         │
│  ── 通用 ────────────────────────────    │
│  [历史记录上限]                            │
│  [速贴窗口记录数]                          │
│                                         │
│  ── 外观 ────────────────────────────    │
│  (●) 跟随系统  ( ) 浅色  ( ) 深色        │
│  速贴窗口位置：                            │
│  (●) 鼠标位置 ( ) 上次位置 ( ) 光标位置    │
│                                         │
│  ── 行为 ────────────────────────────    │
│  [☐] 开机自启                             │
│  [☐] 开机时静默启动                        │
│  [☐] 回贴后恢复剪贴板                      │
│  [☐] 暂停监听                             │
│                                         │
│  ── 排除应用 ────────────────────────    │
│  [文本域]                                │
│                                         │
│  ┌─────────────────────┐                │
│  │     保存设置          │                │
│  └─────────────────────┘                │
│                                         │
└─────────────────────────────────────────┘
```

视觉风格：Primer 的表单风格——`max-width: 680px`，分组用 `<h2>` + 下划线分隔，输入框 1px 实色边框，聚焦时蓝色描边。

---

## 二、Primer 主题系统

### 2.1 设计原则

与 Catppuccin 的关键差异：
- **Catppuccin**：低饱和暖色调，强调温馨舒适
- **Primer**：中性冷色调，强调清晰高效、信息密度

CSS 变量命名从 `--cp-*` 改为 `--pg-*`（primer 全局），保持与现有变量结构相同的层次：

```
基础色板  →  语义映射  →  Tailwind token
```

### 2.2 基础色板（从 Primer primitives 直接取值）

#### Light

```css
:root {
  /* Neutral */
  --pg-neutral-0: #ffffff;
  --pg-neutral-1: #F6F8FA;
  --pg-neutral-2: #EFF2F5;
  --pg-neutral-3: #E6EAEF;
  --pg-neutral-4: #E0E6EB;
  --pg-neutral-5: #DAE0E7;
  --pg-neutral-6: #D1D9E0;
  --pg-neutral-7: #C8D1DA;
  --pg-neutral-8: #818B98;
  --pg-neutral-9: #59636E;
  --pg-neutral-10: #454C54;
  --pg-neutral-11: #393F46;
  --pg-neutral-12: #25292E;
  --pg-neutral-13: #1f2328;

  /* Blue (accent) */
  --pg-blue-0: #ddf4ff;
  --pg-blue-1: #b6e3ff;
  --pg-blue-2: #80ccff;
  --pg-blue-3: #54aeff;
  --pg-blue-4: #218bff;
  --pg-blue-5: #0969da;
  --pg-blue-6: #0550ae;
  --pg-blue-7: #033d8b;
  --pg-blue-8: #0a3069;
  --pg-blue-9: #002155;

  /* Green */
  --pg-green-0: #dafbe1;
  --pg-green-3: #4ac26b;
  --pg-green-4: #2da44e;
  --pg-green-5: #1a7f37;
  --pg-green-7: #044f1e;

  /* Yellow */
  --pg-yellow-0: #fff8c5;
  --pg-yellow-3: #d4a72c;
  --pg-yellow-4: #bf8700;
  --pg-yellow-5: #9a6700;
  --pg-yellow-7: #633c01;

  /* Orange */
  --pg-orange-0: #fff1e5;
  --pg-orange-3: #fb8f44;
  --pg-orange-4: #e16f24;
  --pg-orange-5: #bc4c00;
  --pg-orange-7: #762c00;

  /* Red */
  --pg-red-0: #ffebe9;
  --pg-red-3: #ff8182;
  --pg-red-4: #fa4549;
  --pg-red-5: #cf222e;
  --pg-red-7: #82071e;

  /* Purple */
  --pg-purple-0: #fbefff;
  --pg-purple-3: #c297ff;
  --pg-purple-4: #a475f9;
  --pg-purple-5: #8250df;
  --pg-purple-7: #512a97;
}
```

#### Dark

```css
html.dark {
  --pg-neutral-0: #010409;
  --pg-neutral-1: #0D1117;
  --pg-neutral-2: #151B23;
  --pg-neutral-3: #212830;
  --pg-neutral-4: #262C36;
  --pg-neutral-5: #2A313C;
  --pg-neutral-6: #2F3742;
  --pg-neutral-7: #3D444D;
  --pg-neutral-8: #656C76;
  --pg-neutral-9: #9198A1;
  --pg-neutral-10: #B7BDC8;
  --pg-neutral-11: #D1D7E0;
  --pg-neutral-12: #F0F6FC;
  --pg-neutral-13: #ffffff;

  --pg-blue-0: #cae8ff;
  --pg-blue-1: #a5d6ff;
  --pg-blue-2: #79c0ff;
  --pg-blue-3: #58a6ff;
  --pg-blue-4: #388bfd;
  --pg-blue-5: #1f6feb;
  --pg-blue-6: #1158c7;
  --pg-blue-7: #0d419d;
  --pg-blue-8: #0c2d6b;
  --pg-blue-9: #051d4d;

  --pg-green-0: #aff5b4;
  --pg-green-3: #3fb950;
  --pg-green-4: #2ea043;
  --pg-green-5: #238636;
  --pg-green-7: #0f5323;

  --pg-yellow-0: #f8e3a1;
  --pg-yellow-3: #d29922;
  --pg-yellow-4: #bb8009;
  --pg-yellow-5: #9e6a03;
  --pg-yellow-7: #693e00;

  --pg-orange-0: #ffdfb6;
  --pg-orange-3: #f0883e;
  --pg-orange-4: #db6d28;
  --pg-orange-5: #bd561d;
  --pg-orange-7: #762d0a;

  --pg-red-0: #ffdcd7;
  --pg-red-3: #ff7b72;
  --pg-red-4: #f85149;
  --pg-red-5: #da3633;
  --pg-red-7: #8e1519;

  --pg-purple-0: #eddeff;
  --pg-purple-3: #BE8FFF;
  --pg-purple-4: #AB7DF8;
  --pg-purple-5: #8957e5;
  --pg-purple-7: #553098;
}
```

> 注：为减少 CSS 体积，深色主题只列出与浅色不同的色值。各色阶 0/3/4/5/7 是最常用的语义锚点，完整 0-9 级按需添加。

### 2.3 语义化映射

```css
:root, html.dark {
  /* ── 前景 ── */
  --pg-fg-default: var(--pg-neutral-13);   /* 浅: #1f2328  深: #ffffff */
  --pg-fg-muted: var(--pg-neutral-9);      /* 浅: #59636E  深: #9198A1 */
  --pg-fg-subtle: var(--pg-neutral-8);     /* 浅: #818B98  深: #656C76 */
  --pg-fg-on-emphasis: #ffffff;            /* 按钮上的文字色 */

  /* ── 强调 ── */
  --pg-accent-fg: var(--pg-blue-5);        /* 链接、焦点环 */
  --pg-accent-emphasis: var(--pg-blue-5);  /* 主按钮背景 */
  --pg-accent-subtle: var(--pg-blue-0);    /* 选中项/活跃项背景 */
  --pg-accent-hover: var(--pg-blue-4);     /* hover 态 */

  /* ── 画布 ── */
  --pg-canvas-default: var(--pg-neutral-0); /* 浅: #ffffff  深: #010409 */
  --pg-canvas-subtle: var(--pg-neutral-1);  /* 浅: #F6F8FA  深: #0D1117 */
  --pg-canvas-inset: var(--pg-neutral-0);   /* 内嵌区域（浅/深同 neutral-0） */

  /* ── 边框 ── */
  --pg-border-default: var(--pg-neutral-6); /* 浅: #D1D9E0  深: #3D444D */
  --pg-border-muted: var(--pg-neutral-4);   /* 浅: #E0E6EB  深: #262C36 */
  --pg-border-subtle: var(--pg-neutral-3);  /* 分隔线 */
  --pg-border-accent: var(--pg-blue-5);     /* 焦点/活跃边框 */

  /* ── 状态 ── */
  --pg-success-fg: var(--pg-green-5);
  --pg-success-emphasis: var(--pg-green-4);
  --pg-success-subtle: var(--pg-green-0);

  --pg-danger-fg: var(--pg-red-5);
  --pg-danger-emphasis: var(--pg-red-5);
  --pg-danger-subtle: var(--pg-red-0);

  --pg-warning-fg: var(--pg-yellow-5);
  --pg-warning-emphasis: var(--pg-yellow-4);
  --pg-warning-subtle: var(--pg-yellow-0);

  --pg-done-fg: var(--pg-purple-5);
  --pg-done-emphasis: var(--pg-purple-5);
  --pg-done-subtle: var(--pg-purple-0);

  /* ── 收藏（复用 yellow-5） ── */
  --pg-favorite: var(--pg-yellow-5);

  /* ── 阴影 ── */
  --pg-shadow-sm: 0 1px 0 var(--pg-border-default);
  --pg-shadow-md: 0 3px 6px rgba(31, 35, 40, 0.04);
  --pg-shadow-lg: 0 8px 24px rgba(31, 35, 40, 0.12);
  --pg-shadow-xl: 0 12px 28px rgba(31, 35, 40, 0.12), 0 2px 4px rgba(31, 35, 40, 0.08);
}
```

### 2.4 字体

```css
:root {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif;
}
```

替代 Catppuccin 使用的 `Georgia` display 字体和 `'Segoe UI'` body 字体。统一为 Primer 的系统字体栈，不再区分 display/body。

### 2.5 tailwind.config.ts 变更

- 删除 `cp.*` 整个颜色对象
- 新增 `pg.*` 颜色对象，映射所有语义变量
- 删除遗留别名 `ink/paper/accent/accentDeep/moss/primaryDark`
- 字体 `display` 改为 `'Georgia', serif`（如仍需保留，否则删除 display font family）
- 阴影改为 Primer 的 shadow token

---

## 三、组件级影响

### 3.1 所有窗口 Shell

| 文件 | 变更 |
|------|------|
| `index.css` | 全量替换 Catppuccin CSS 变量为 Primer |
| `tailwind.config.ts` | 全量替换颜色 token |
| `App.tsx` | body 主题 class 不变，内部逻辑不变 |
| `ManagerShell.tsx` | 重写为纯设置页面 |
| `PickerShell.tsx` | STYLES 常量中 `--cp-*` → `--pg-*` |
| `WorkbenchShell.tsx` | STYLES + inline styles 中 `--cp-*` → `--pg-*` |
| `EditorShell.tsx` | inline styles 中 `--cp-*` → `--pg-*` |

### 3.2 共享组件

| 文件 | 变更 |
|------|------|
| `Panel.tsx` | `bg-cp-mantle/70` → Primer 等价色 |
| `StatusBadge.tsx` | `--cp-*-rgb` → `--pg-*` |
| `LoadingSpinner.tsx` | accent 色引用更新 |
| `EmptyState.tsx` | 颜色变量更新 |

### 3.3 queries 迁移

| 操作 | 文件 |
|------|------|
| 新建 | `src/shared/queries/clipQueries.ts` — 迁入 `useItemDetailQuery`、`useUpdateTextMutation`、`invalidateClipQueries` |
| 修改 | `src/features/manager/queries.ts` — 仅保留 `useSettingsQuery`、`useUpdateSettingsMutation` |
| 修改 | `src/features/workbench/WorkbenchShell.tsx` — import 路径改为 `../../shared/queries/clipQueries` |
| 修改 | `src/features/editor/EditorShell.tsx` — import 路径改为 `../../shared/queries/clipQueries` |

### 3.4 删除文件

| 文件 | 原因 |
|------|------|
| `src/features/manager/store.ts` | 不再有 selectedItemId/draftText/viewMode 状态 |
| `src/shared/components/EmptyState.tsx` | 仅 ManagerShell 使用，设置页面不需要空状态 |

---

## 四、实施步骤

1. **新建 `src/shared/queries/clipQueries.ts`**，从 manager/queries.ts 迁出共享 hook
2. **更新 WorkbenchShell 和 EditorShell** 的 import 路径
3. **精简 `manager/queries.ts`**，只保留 settings 相关
4. **删除 `manager/store.ts`**
5. **重写 `index.css`**：Catppuccin → Primer 全量替换
6. **重写 `tailwind.config.ts`**：cp → pg 全量替换
7. **重写 `ManagerShell.tsx`**：三栏 → 单列设置页面
8. **更新 PickerShell / WorkbenchShell / EditorShell** 中的 `--cp-*` → `--pg-*`
9. **更新共享组件** Panel / StatusBadge / LoadingSpinner / EmptyState
10. **视觉验证**：逐窗口检查浅色/深色主题表现

---

## 五、风险与注意事项

- **全局搜索替换的风险**：`--cp-` 替换为 `--pg-` 是机械操作但涉及 6 个文件数百处，需逐文件验证
- **index.css 中的 `html.window-picker` 透明补丁**和 `body.theme-workbench` 渐变背景需要用 Primer 变量重写
- **rgb 分量变量**：Catppuccin 有 `--cp-peach-rgb` 等，Primer 语义层不再需要此模式（Primer 的 subtle 色已内置半透明），可考虑移除所有 `*-rgb` 变量，简化系统
- **Panel 组件的 `backdrop-blur` 毛玻璃效果**与 Primer 风格不太一致，Primer 更偏好纯色背景 + 实色边框
