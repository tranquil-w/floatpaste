# Primer 主题迁移 + ManagerShell 设置窗口重设计 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 ManagerShell 从三栏剪贴板管理界面简化为纯设置窗口，并将整体色彩系统从 Catppuccin 迁移为 GitHub Primer 风格。

**Architecture:** 迁移共享 queries 到 `src/shared/queries/clipQueries.ts`，删除 `manager/store.ts`，重写 ManagerShell 为单列居中设置页面（max-width 680px），用 Primer 语义化 CSS 变量全量替换 Catppuccin 变量，更新 tailwind.config.ts 对应 token。

**Tech Stack:** React, TypeScript, Tailwind CSS, CSS Custom Properties, React Query

---

## Chunk 1: Foundation — Queries Migration + Theme System

### Task 1: Create `src/shared/queries/clipQueries.ts`

**Files:**
- Create: `src/shared/queries/clipQueries.ts`

从 `manager/queries.ts` 中迁出被 WorkbenchShell 和 EditorShell 共用的三个导出。

- [ ] **Step 1: Create the shared queries file**

```typescript
// src/shared/queries/clipQueries.ts
import { useMutation, useQuery } from "@tanstack/react-query";
import { getItemDetail, updateTextItem } from "../../bridge/commands";
import { queryClient } from "../../app/queryClient";

export function useItemDetailQuery(id: string | null) {
  return useQuery({
    queryKey: ["detail", id],
    queryFn: () => getItemDetail(id as string),
    enabled: Boolean(id),
  });
}

function invalidateClipQueries() {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: ["favorites"] }),
    queryClient.invalidateQueries({ queryKey: ["search"] }),
    queryClient.invalidateQueries({ queryKey: ["detail"] }),
    queryClient.invalidateQueries({ queryKey: ["picker-recent"] }),
    queryClient.invalidateQueries({ queryKey: ["workbench-recent"] }),
    queryClient.invalidateQueries({ queryKey: ["workbench-search"] }),
  ]);
}

export function useUpdateTextMutation() {
  return useMutation({
    mutationFn: ({ id, text }: { id: string; text: string }) => updateTextItem(id, text),
    onSuccess: async (detail) => {
      queryClient.setQueryData(["detail", detail.id], detail);
      await invalidateClipQueries();
    },
  });
}
```

- [ ] **Step 2: Verify file was created**

Run: `ls src/shared/queries/clipQueries.ts`

- [ ] **Step 3: Commit**

```bash
git add src/shared/queries/clipQueries.ts
git commit -m "refactor: extract shared clip queries to src/shared/queries/clipQueries.ts"
```

---

### Task 2: Update consumer imports

**Files:**
- Modify: `src/features/workbench/WorkbenchShell.tsx:21`
- Modify: `src/features/editor/EditorShell.tsx:7`

- [ ] **Step 1: Update WorkbenchShell import**

In `src/features/workbench/WorkbenchShell.tsx`, change line 21:
```diff
-import { useItemDetailQuery } from "../manager/queries";
+import { useItemDetailQuery } from "../../shared/queries/clipQueries";
```

- [ ] **Step 2: Update EditorShell imports**

In `src/features/editor/EditorShell.tsx`, change line 7:
```diff
-import { useItemDetailQuery, useUpdateTextMutation } from "../manager/queries";
+import { useItemDetailQuery, useUpdateTextMutation } from "../../shared/queries/clipQueries";
```

- [ ] **Step 3: Verify TypeScript compiles**

Run: `rtk tsc`

Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add src/features/workbench/WorkbenchShell.tsx src/features/editor/EditorShell.tsx
git commit -m "refactor: update WorkbenchShell and EditorShell imports to shared clipQueries"
```

---

### Task 3: Slim down `manager/queries.ts` (DEFERRED — merged into Task 7)

**Files:**
- Modify: `src/features/manager/queries.ts`

> **DEFERRED:** ManagerShell 当前仍导入 `useFavoritesQuery`、`useSearchQuery`、`useDeleteItemMutation` 等剪贴板操作 hooks。在 Task 7 重写 ManagerShell 后这些 exports 将不再有消费者。此任务的实际清理将作为 Task 7 的最后一步执行。跳过此任务。

- [ ] **Step 1: Skip — proceed to Task 4**

---

### Task 4: Delete `manager/store.ts`

**Files:**
- Delete: `src/features/manager/store.ts`

- [ ] **Step 1: Delete the store file**

Run: `rm src/features/manager/store.ts`

- [ ] **Step 2: Verify no remaining imports of store**

Run: `grep -r "useManagerStore\|manager/store" src/`

Expected: No matches

- [ ] **Step 3: Commit**

```bash
git add -u src/features/manager/store.ts
git commit -m "refactor: remove manager/store.ts (viewMode/selectedItemId/draftText no longer needed)"
```

---

### Task 5: Rewrite `index.css` — Catppuccin → Primer

**Files:**
- Modify: `src/index.css`

全量替换 CSS 变量。Primer 不使用 RGB 分量变量，所有色值直接使用 hex。

- [ ] **Step 1: Replace entire index.css content**

Replace the entire file with:

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

/* ── Primer Light Theme ── */
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

  color: var(--pg-fg-default);
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif;
  color-scheme: light;
  background-color: var(--pg-canvas-default);
}

/* ── Primer Dark Theme ── */
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

/* ── Semantic Mapping (both themes) ── */
:root,
html.dark {
  /* Foreground */
  --pg-fg-default: var(--pg-neutral-13);
  --pg-fg-muted: var(--pg-neutral-9);
  --pg-fg-subtle: var(--pg-neutral-8);
  --pg-fg-on-emphasis: #ffffff;

  /* Accent */
  --pg-accent-fg: var(--pg-blue-5);
  --pg-accent-emphasis: var(--pg-blue-5);
  --pg-accent-subtle: var(--pg-blue-0);
  --pg-accent-hover: var(--pg-blue-4);

  /* Canvas */
  --pg-canvas-default: var(--pg-neutral-0);
  --pg-canvas-subtle: var(--pg-neutral-1);
  --pg-canvas-inset: var(--pg-neutral-0);

  /* Border */
  --pg-border-default: var(--pg-neutral-6);
  --pg-border-muted: var(--pg-neutral-4);
  --pg-border-subtle: var(--pg-neutral-3);
  --pg-border-accent: var(--pg-blue-5);

  /* Status */
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

  /* Favorite (reuse yellow) */
  --pg-favorite: var(--pg-yellow-5);

  /* Shadow */
  --pg-shadow-sm: 0 1px 0 var(--pg-border-default);
  --pg-shadow-md: 0 3px 6px rgba(31, 35, 40, 0.04);
  --pg-shadow-lg: 0 8px 24px rgba(31, 35, 40, 0.12);
  --pg-shadow-xl: 0 12px 28px rgba(31, 35, 40, 0.12), 0 2px 4px rgba(31, 35, 40, 0.08);
}

/* ── Scrollbar ── */
::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: var(--pg-border-muted);
  border: 3px solid transparent;
  background-clip: content-box;
  border-radius: 10px;
}

::-webkit-scrollbar-thumb:hover {
  background: var(--pg-border-default);
  border: 3px solid transparent;
  background-clip: content-box;
}

html,
body,
#root {
  min-height: 100%;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  color: var(--pg-fg-default);
  background-color: var(--pg-canvas-default);
  transition:
    background-color 250ms ease-out,
    color 250ms ease-out;
}

/* Picker window transparency patch — must be last for highest priority */
html.window-picker,
html.window-picker body,
html.window-picker #root {
  background: transparent !important;
  background-color: transparent !important;
  background-image: none !important;
  transition: none !important;
  box-shadow: none !important;
}

body.theme-picker {
  background: transparent !important;
  background-color: transparent !important;
  transition: none !important;
}

body.theme-manager {
  background-color: var(--pg-canvas-default);
}

body.theme-workbench {
  background:
    radial-gradient(circle at top right, rgba(9, 105, 218, 0.06), transparent 45%),
    radial-gradient(circle at bottom left, rgba(31, 35, 40, 0.08), transparent 50%),
    var(--pg-canvas-default);
}

button,
input,
textarea,
select {
  font: inherit;
  color: inherit;
  background: transparent;
}

button {
  cursor: pointer;
  border: 1px solid transparent;
  outline: none;
}

/* Focus-visible styles for keyboard navigation */
button:focus-visible,
input:focus-visible,
textarea:focus-visible,
select:focus-visible,
[role="button"]:focus-visible {
  outline: 2px solid var(--pg-accent-fg);
  outline-offset: 2px;
}

input,
textarea {
  border: 1px solid transparent;
  outline: none;
}

/* Reduced motion support */
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}

/* Skip link for keyboard navigation */
.skip-link {
  position: absolute;
  top: -40px;
  left: 0;
  background: var(--pg-accent-emphasis);
  color: var(--pg-fg-on-emphasis);
  padding: 8px;
  text-decoration: none;
  z-index: 100;
  border-radius: 0 0 4px;
  font-weight: bold;
}

.skip-link:focus {
  top: 0;
}

::selection {
  color: var(--pg-fg-on-emphasis);
  background: var(--pg-accent-subtle);
}
```

- [ ] **Step 2: Verify no remaining `--cp-` references in CSS**

Run: `grep -n "cp-" src/index.css`

Expected: No matches

- [ ] **Step 3: Commit**

```bash
git add src/index.css
git commit -m "style: replace Catppuccin CSS variables with GitHub Primer theme system"
```

---

### Task 6: Rewrite `tailwind.config.ts`

**Files:**
- Modify: `tailwind.config.ts`

替换 `cp.*` 为 `pg.*`，删除遗留别名。

- [ ] **Step 1: Replace entire tailwind.config.ts**

Replace the entire file with:

```typescript
import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      transitionDuration: {
        '250': '250ms',
      },
      colors: {
        // Primer semantic tokens (via CSS variables)
        pg: {
          "fg-default": "var(--pg-fg-default)",
          "fg-muted": "var(--pg-fg-muted)",
          "fg-subtle": "var(--pg-fg-subtle)",
          "fg-on-emphasis": "var(--pg-fg-on-emphasis)",
          "accent-fg": "var(--pg-accent-fg)",
          "accent-emphasis": "var(--pg-accent-emphasis)",
          "accent-subtle": "var(--pg-accent-subtle)",
          "accent-hover": "var(--pg-accent-hover)",
          "canvas-default": "var(--pg-canvas-default)",
          "canvas-subtle": "var(--pg-canvas-subtle)",
          "canvas-inset": "var(--pg-canvas-inset)",
          "border-default": "var(--pg-border-default)",
          "border-muted": "var(--pg-border-muted)",
          "border-subtle": "var(--pg-border-subtle)",
          "border-accent": "var(--pg-border-accent)",
          "success-fg": "var(--pg-success-fg)",
          "success-emphasis": "var(--pg-success-emphasis)",
          "success-subtle": "var(--pg-success-subtle)",
          "danger-fg": "var(--pg-danger-fg)",
          "danger-emphasis": "var(--pg-danger-emphasis)",
          "danger-subtle": "var(--pg-danger-subtle)",
          "warning-fg": "var(--pg-warning-fg)",
          "warning-emphasis": "var(--pg-warning-emphasis)",
          "warning-subtle": "var(--pg-warning-subtle)",
          "done-fg": "var(--pg-done-fg)",
          "done-emphasis": "var(--pg-done-emphasis)",
          "done-subtle": "var(--pg-done-subtle)",
          favorite: "var(--pg-favorite)",
          // Direct primitive access (for custom opacity/needs)
          "neutral-0": "var(--pg-neutral-0)",
          "neutral-1": "var(--pg-neutral-1)",
          "neutral-2": "var(--pg-neutral-2)",
          "neutral-3": "var(--pg-neutral-3)",
          "neutral-4": "var(--pg-neutral-4)",
          "neutral-5": "var(--pg-neutral-5)",
          "neutral-6": "var(--pg-neutral-6)",
          "neutral-7": "var(--pg-neutral-7)",
          "neutral-8": "var(--pg-neutral-8)",
          "neutral-9": "var(--pg-neutral-9)",
          "neutral-10": "var(--pg-neutral-10)",
          "neutral-11": "var(--pg-neutral-11)",
          "neutral-12": "var(--pg-neutral-12)",
          "neutral-13": "var(--pg-neutral-13)",
        },
      },
      boxShadow: {
        'pg-sm': "var(--pg-shadow-sm)",
        'pg-md': "var(--pg-shadow-md)",
        'pg-lg': "var(--pg-shadow-lg)",
        'pg-xl': "var(--pg-shadow-xl)",
      },
      fontFamily: {
        display: ["Georgia", "serif"],
        body: ["'-apple-system', 'BlinkMacSystemFont', 'Segoe UI', 'Noto Sans', 'Helvetica', 'Arial', 'sans-serif'"],
      },
    },
  },
  plugins: [],
} satisfies Config;
```

- [ ] **Step 2: Verify no remaining `cp` references**

Run: `grep -n "cp\|ink\|paper\|accentDeep\|moss\|primaryDark" tailwind.config.ts`

Expected: No matches (the word "accent" appears only in `accent-fg` etc., not `accentDeep`)

- [ ] **Step 3: Commit**

```bash
git add tailwind.config.ts
git commit -m "style: replace tailwind color tokens from Catppuccin to Primer system"
```

---

## Chunk 2: ManagerShell Rewrite

### Task 7: Rewrite `ManagerShell.tsx` as pure settings page

**Files:**
- Modify: `src/features/manager/ManagerShell.tsx`

将 1017 行的三栏管理界面重写为单列居中设置页面。保留 SettingsPanel 全部表单逻辑，但使用 Primer 风格重写样式。

移除所有依赖：Panel, StatusBadge, EmptyState, LoadingSpinner, useManagerStore, showPicker, CLIPS_CHANGED_EVENT, 剪贴板相关 queries/mutations。

- [ ] **Step 1: Rewrite ManagerShell.tsx**

Replace the entire file with:

```tsx
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  SETTINGS_CHANGED_EVENT,
  MANAGER_OPEN_SETTINGS_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { hideCurrentWindow } from "../../bridge/window";
import { queryClient } from "../../app/queryClient";
import type { PickerPositionMode, ThemeMode, UserSetting } from "../../shared/types/settings";
import { useSettingsQuery, useUpdateSettingsMutation } from "./queries";

const pickerPositionOptions: Array<{
  value: PickerPositionMode;
  label: string;
  description: string;
}> = [
  {
    value: "mouse",
    label: "鼠标位置",
    description: "默认推荐，速贴窗口会贴近当前鼠标所在位置弹出。",
  },
  {
    value: "lastPosition",
    label: "上次关闭时的位置",
    description: "保留上次拖动或关闭时的位置；首次使用会落在屏幕中心。",
  },
  {
    value: "caret",
    label: "光标所在位置",
    description: "优先跟随当前输入光标；如果系统拿不到光标位置，会退回鼠标位置。",
  },
];

const themeModeOptions: Array<{
  value: ThemeMode;
  label: string;
  description: string;
}> = [
  {
    value: "system",
    label: "跟随系统",
    description: "自动匹配 Windows 当前的浅色或深色外观。",
  },
  {
    value: "light",
    label: "浅色",
    description: "中性冷色调浅色主题，适合日常办公。",
  },
  {
    value: "dark",
    label: "深色",
    description: "中性深色主题，适合夜间使用。",
  },
];

const FORM_INPUT =
  "w-full rounded-md border border-[color:var(--pg-border-default)] bg-[color:var(--pg-canvas-inset)] px-4 py-2.5 text-sm outline-none transition-colors placeholder:text-[color:var(--pg-fg-subtle)] focus:border-[color:var(--pg-accent-fg)] focus:ring-1 focus:ring-[color:var(--pg-accent-fg)] focus-visible:outline-none";

const FORM_LABEL = "mb-1.5 block text-sm font-medium text-[color:var(--pg-fg-default)]";

const FORM_HINT = "mt-1.5 text-xs leading-relaxed text-[color:var(--pg-fg-subtle)]";

const SECTION_HEADING = "text-sm font-semibold text-[color:var(--pg-fg-default)] border-b border-[color:var(--pg-border-subtle)] pb-2";

export function ManagerShell() {
  const settings = useSettingsQuery();
  const updateSettingsMutation = useUpdateSettingsMutation();

  const { data } = settings;

  const [shortcut, setShortcut] = useState("Alt+Q");
  const [launchOnStartup, setLaunchOnStartup] = useState(false);
  const [silentOnStartup, setSilentOnStartup] = useState(false);
  const [historyLimit, setHistoryLimit] = useState(1000);
  const [pickerRecordLimit, setPickerRecordLimit] = useState(50);
  const [pickerPositionMode, setPickerPositionMode] = useState<PickerPositionMode>("mouse");
  const [restoreClipboardAfterPaste, setRestoreClipboardAfterPaste] = useState(true);
  const [pauseMonitoring, setPauseMonitoring] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>("system");
  const [excludedAppsText, setExcludedAppsText] = useState("");
  const [workbenchShortcut, setWorkbenchShortcut] = useState("Alt+S");
  const [workbenchShortcutEnabled, setWorkbenchShortcutEnabled] = useState(true);

  useEffect(() => {
    if (!data) return;
    setShortcut(data.shortcut);
    setLaunchOnStartup(data.launchOnStartup);
    setSilentOnStartup(data.silentOnStartup);
    setHistoryLimit(data.historyLimit);
    setPickerRecordLimit(data.pickerRecordLimit);
    setPickerPositionMode(data.pickerPositionMode);
    setRestoreClipboardAfterPaste(data.restoreClipboardAfterPaste);
    setPauseMonitoring(data.pauseMonitoring);
    setThemeMode(data.themeMode);
    setExcludedAppsText(data.excludedApps.join("\n"));
    setWorkbenchShortcut(data.workbenchShortcut);
    setWorkbenchShortcutEnabled(data.workbenchShortcutEnabled);
  }, [data]);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let offSettings: (() => void) | undefined;
    let offOpenSettings: (() => void) | undefined;

    void listen(SETTINGS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      offSettings = cleanup;
    });

    void listen(MANAGER_OPEN_SETTINGS_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      offOpenSettings = cleanup;
    });

    return () => {
      offSettings?.();
      offOpenSettings?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void hideCurrentWindow().catch(console.error);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const saveError = updateSettingsMutation.error
    ? getErrorMessage(updateSettingsMutation.error)
    : null;

  if (settings.isLoading && !data) {
    return (
      <div className="flex h-screen items-center justify-center text-sm text-[color:var(--pg-fg-subtle)]">
        正在加载设置...
      </div>
    );
  }

  return (
    <main className="flex min-h-screen flex-col">
      <div className="mx-auto w-full max-w-[680px] px-6 py-8">
        {/* Header */}
        <div className="mb-8">
          <h1 className="text-xl font-semibold text-[color:var(--pg-fg-default)]">
            FloatPaste
          </h1>
          <p className="mt-1 text-sm text-[color:var(--pg-fg-muted)]">
            偏好设置会自动保存。
          </p>
        </div>

        {saveError ? (
          <div className="mb-6 flex items-start justify-between gap-3 rounded-md border border-[color:var(--pg-danger-fg)] bg-[color:var(--pg-danger-subtle)] px-4 py-3 text-sm text-[color:var(--pg-danger-fg)]">
            <p>{saveError}</p>
            <button
              className="shrink-0 text-xs font-semibold uppercase tracking-wider transition-opacity hover:opacity-80"
              onClick={() => updateSettingsMutation.reset()}
              type="button"
            >
              关闭
            </button>
          </div>
        ) : null}

        {/* ── 快捷键 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>快捷键</h2>
          <div className="mt-4 space-y-4">
            <label className="block">
              <span className={FORM_LABEL}>全局快捷键</span>
              <input
                className={FORM_INPUT}
                onChange={(e) => setShortcut(e.target.value)}
                value={shortcut}
              />
            </label>

            <div className="block">
              <div className="mb-1.5 flex items-center justify-between">
                <span className={FORM_LABEL}>搜索窗口快捷键</span>
                <label className="flex cursor-pointer items-center gap-2">
                  <input
                    checked={workbenchShortcutEnabled}
                    className="h-4 w-4 rounded border-[color:var(--pg-border-default)] accent-[color:var(--pg-accent-fg)]"
                    onChange={(e) => setWorkbenchShortcutEnabled(e.target.checked)}
                    type="checkbox"
                  />
                  <span className="text-xs text-[color:var(--pg-fg-subtle)]">启用</span>
                </label>
              </div>
              <input
                className={FORM_INPUT}
                disabled={!workbenchShortcutEnabled}
                onChange={(e) => setWorkbenchShortcut(e.target.value)}
                placeholder="Alt+S"
                value={workbenchShortcut}
              />
              <p className={FORM_HINT}>全局快捷键，直接打开搜索窗口。</p>
            </div>
          </div>
        </section>

        {/* ── 通用 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>通用</h2>
          <div className="mt-4 space-y-4">
            <label className="block">
              <span className={FORM_LABEL}>历史记录上限</span>
              <input
                className={FORM_INPUT}
                min={100}
                onChange={(e) => setHistoryLimit(Number(e.target.value) || 1000)}
                step={100}
                type="number"
                value={historyLimit}
              />
            </label>

            <label className="block">
              <span className={FORM_LABEL}>速贴窗口记录数</span>
              <input
                className={FORM_INPUT}
                max={1000}
                min={9}
                onChange={(e) => setPickerRecordLimit(Number(e.target.value) || 50)}
                type="number"
                value={pickerRecordLimit}
              />
              <p className={FORM_HINT}>
                控制速贴面板一次可滚动浏览的记录数，数字快捷键仍只覆盖前 9 条。
              </p>
            </label>
          </div>
        </section>

        {/* ── 外观 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>外观</h2>
          <div className="mt-4 space-y-4">
            <fieldset className="border-0 p-0 m-0">
              <legend className={FORM_LABEL}>界面主题</legend>
              <div className="space-y-2">
                {themeModeOptions.map((option) => (
                  <label
                    className={`flex cursor-pointer items-start gap-3 rounded-md border px-4 py-3 transition-colors ${
                      themeMode === option.value
                        ? "border-[color:var(--pg-accent-fg)] bg-[color:var(--pg-accent-subtle)]"
                        : "border-[color:var(--pg-border-muted)] hover:border-[color:var(--pg-border-default)]"
                    }`}
                    key={option.value}
                  >
                    <input
                      checked={themeMode === option.value}
                      className="mt-0.5 h-4 w-4 accent-[color:var(--pg-accent-fg)]"
                      name="theme-mode"
                      onChange={() => setThemeMode(option.value)}
                      type="radio"
                    />
                    <span className="min-w-0">
                      <span className={`block text-sm font-medium ${themeMode === option.value ? "text-[color:var(--pg-fg-default)]" : "text-[color:var(--pg-fg-muted)]"}`}>
                        {option.label}
                      </span>
                      <span className="mt-0.5 block text-xs text-[color:var(--pg-fg-subtle)]">
                        {option.description}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>

            <fieldset className="border-0 p-0 m-0">
              <legend className={FORM_LABEL}>速贴窗口显示位置</legend>
              <div className="space-y-2">
                {pickerPositionOptions.map((option) => (
                  <label
                    className={`flex cursor-pointer items-start gap-3 rounded-md border px-4 py-3 transition-colors ${
                      pickerPositionMode === option.value
                        ? "border-[color:var(--pg-accent-fg)] bg-[color:var(--pg-accent-subtle)]"
                        : "border-[color:var(--pg-border-muted)] hover:border-[color:var(--pg-border-default)]"
                    }`}
                    key={option.value}
                  >
                    <input
                      checked={pickerPositionMode === option.value}
                      className="mt-0.5 h-4 w-4 accent-[color:var(--pg-accent-fg)]"
                      name="picker-position-mode"
                      onChange={() => setPickerPositionMode(option.value)}
                      type="radio"
                    />
                    <span className="min-w-0">
                      <span className={`block text-sm font-medium ${pickerPositionMode === option.value ? "text-[color:var(--pg-fg-default)]" : "text-[color:var(--pg-fg-muted)]"}`}>
                        {option.label}
                      </span>
                      <span className="mt-0.5 block text-xs text-[color:var(--pg-fg-subtle)]">
                        {option.description}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>
          </div>
        </section>

        {/* ── 行为 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>行为</h2>
          <div className="mt-4 space-y-2">
            <label className="flex cursor-pointer items-center gap-3 rounded-md border border-[color:var(--pg-border-muted)] px-4 py-3 transition-colors hover:border-[color:var(--pg-border-default)]">
              <input
                className="h-4 w-4 accent-[color:var(--pg-accent-fg)]"
                checked={launchOnStartup}
                onChange={(e) => {
                  const checked = e.target.checked;
                  setLaunchOnStartup(checked);
                  if (!checked) setSilentOnStartup(false);
                }}
                type="checkbox"
              />
              <span className="text-sm font-medium text-[color:var(--pg-fg-default)]">开机自启</span>
            </label>

            <label
              className={`flex items-center gap-3 rounded-md border px-4 py-3 transition-colors ${
                launchOnStartup
                  ? "cursor-pointer border-[color:var(--pg-border-muted)] hover:border-[color:var(--pg-border-default)]"
                  : "cursor-not-allowed border-[color:var(--pg-border-subtle)]"
              }`}
            >
              <input
                className="h-4 w-4 accent-[color:var(--pg-accent-fg)]"
                checked={silentOnStartup}
                disabled={!launchOnStartup}
                onChange={(e) => setSilentOnStartup(e.target.checked)}
                type="checkbox"
              />
              <span className={`text-sm font-medium ${launchOnStartup ? "text-[color:var(--pg-fg-default)]" : "text-[color:var(--pg-fg-subtle)]"}`}>
                开机时静默启动
              </span>
            </label>

            <label className="flex cursor-pointer items-center gap-3 rounded-md border border-[color:var(--pg-border-muted)] px-4 py-3 transition-colors hover:border-[color:var(--pg-border-default)]">
              <input
                className="h-4 w-4 accent-[color:var(--pg-accent-fg)]"
                checked={restoreClipboardAfterPaste}
                onChange={(e) => setRestoreClipboardAfterPaste(e.target.checked)}
                type="checkbox"
              />
              <span className="text-sm font-medium text-[color:var(--pg-fg-default)]">回贴后恢复剪贴板</span>
            </label>

            <label className="flex cursor-pointer items-center gap-3 rounded-md border border-[color:var(--pg-border-muted)] px-4 py-3 transition-colors hover:border-[color:var(--pg-border-default)]">
              <input
                className="h-4 w-4 accent-[color:var(--pg-accent-fg)]"
                checked={pauseMonitoring}
                onChange={(e) => setPauseMonitoring(e.target.checked)}
                type="checkbox"
              />
              <span className="text-sm font-medium text-[color:var(--pg-fg-default)]">暂停监听</span>
            </label>
          </div>
        </section>

        {/* ── 排除应用 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>排除应用</h2>
          <div className="mt-4">
            <label className="block">
              <textarea
                className={`${FORM_INPUT} min-h-[100px] leading-relaxed`}
                onChange={(e) => setExcludedAppsText(e.target.value)}
                placeholder={"每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"}
                value={excludedAppsText}
              />
            </label>
          </div>
        </section>

        {/* Save Button */}
        <div className="pt-2 pb-8">
          <button
            className="rounded-md bg-[color:var(--pg-accent-emphasis)] px-6 py-2.5 text-sm font-semibold text-[color:var(--pg-fg-on-emphasis)] transition-colors hover:bg-[color:var(--pg-accent-hover)] disabled:opacity-50"
            disabled={updateSettingsMutation.isPending}
            onClick={() => {
              updateSettingsMutation.reset();
              updateSettingsMutation.mutate({
                shortcut,
                launchOnStartup,
                silentOnStartup: launchOnStartup ? silentOnStartup : false,
                historyLimit,
                pickerRecordLimit,
                pickerPositionMode,
                themeMode,
                excludedApps: excludedAppsText
                  .split(/\r?\n/)
                  .map((v) => v.trim())
                  .filter(Boolean),
                restoreClipboardAfterPaste,
                pauseMonitoring,
                workbenchShortcut,
                workbenchShortcutEnabled,
              });
            }}
            type="button"
          >
            保存设置
          </button>
        </div>
      </div>
    </main>
  );
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  if (typeof error === "string" && error.trim()) {
    return error;
  }
  return "保存设置失败，请稍后重试。";
}
```

- [ ] **Step 2: Clean up `manager/queries.ts` — remove dead clip exports**

Now that ManagerShell no longer imports them, remove all clip-related hooks from `manager/queries.ts`. Keep only `useSettingsQuery` and `useUpdateSettingsMutation`.

Replace the entire content of `src/features/manager/queries.ts` with:

```typescript
import { useMutation, useQuery } from "@tanstack/react-query";
import { queryClient } from "../../app/queryClient";
import { getSettings, updateSettings } from "../../bridge/commands";
import type { UserSetting } from "../../shared/types/settings";

export function useSettingsQuery() {
  return useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });
}

export function useUpdateSettingsMutation() {
  return useMutation({
    mutationFn: (payload: UserSetting) => updateSettings(payload),
    onSuccess: (nextValue) => {
      queryClient.setQueryData(["settings"], nextValue);
    },
  });
}
```

- [ ] **Step 3: Verify TypeScript compiles**

Run: `rtk tsc`

Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add src/features/manager/ManagerShell.tsx src/features/manager/queries.ts
git commit -m "feat: rewrite ManagerShell as pure settings page with Primer styling"
```

---

## Chunk 3: Theme Migration on Remaining Components

### Task 8: Update `PickerShell.tsx` styles

**Files:**
- Modify: `src/features/picker/PickerShell.tsx:26-48` (STYLES constant)
- Modify: `src/features/picker/PickerShell.tsx` (inline `--cp-*` references in JSX)

**CSS Variable Mapping for PickerShell STYLES:**

| Old Catppuccin | New Primer |
|---|---|
| `--cp-border-weak` | `--pg-border-muted` |
| `--cp-window-shell` | `--pg-canvas-default` |
| `--cp-mantle` / `--cp-mantle-rgb` | `--pg-canvas-subtle` (no RGB needed) |
| `--cp-surface0` / `--cp-surface0-rgb` | `--pg-neutral-3` |
| `--cp-surface1` / `--cp-surface1-rgb` | `--pg-neutral-6` |
| `--cp-text-primary` | `--pg-fg-default` |
| `--cp-text-secondary` | `--pg-fg-muted` |
| `--cp-text-muted` | `--pg-fg-subtle` |
| `--cp-accent-primary-rgb` | `--pg-accent-fg` (use opacity modifier) |
| `--cp-favorite` | `--pg-favorite` |

- [ ] **Step 1: Replace STYLES constant (lines 26-48)**

Replace with:

```typescript
const STYLES = {
  container:
    "flex h-screen w-screen flex-col overflow-hidden rounded-md border border-[color:var(--pg-border-muted)] bg-[color:var(--pg-canvas-default)]",
  header:
    "flex shrink-0 items-center justify-between border-b border-[color:var(--pg-border-subtle)] bg-[color:var(--pg-canvas-subtle)] px-2.5 py-1.5",
  headerDot: "h-2.5 w-2.5 rounded-full bg-[color:var(--pg-neutral-7)]",
  headerButton:
    "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[11px] font-semibold text-[color:var(--pg-fg-muted)] transition-colors hover:bg-[color:var(--pg-accent-subtle)] hover:text-[color:var(--pg-fg-default)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[color:var(--pg-accent-fg)] focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-45",
  itemButton: (selected: boolean) => `group relative flex w-full flex-col gap-1 rounded-md px-2 py-1.5 text-left transition-colors border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[color:var(--pg-accent-fg)] focus-visible:ring-offset-2 ${
    selected
      ? "bg-[color:var(--pg-accent-subtle)] border-[color:var(--pg-accent-fg)]/30"
      : "bg-transparent border-transparent hover:bg-[color:var(--pg-canvas-subtle)]"
  }`,
  itemContent: (selected: boolean) =>
    `${selected ? "text-[color:var(--pg-fg-default)]" : "text-[color:var(--pg-fg-default)]/90"} line-clamp-5 text-[13px] font-medium leading-[1.6] tracking-tight break-words [overflow-wrap:anywhere] whitespace-pre-wrap transition-colors`,
  kbdBadge: (selected: boolean) => `flex h-[16px] min-w-[16px] px-1 items-center justify-center rounded-[3px] font-mono text-[9px] font-bold transition-colors ${
    selected
      ? "bg-[color:var(--pg-neutral-7)] text-[color:var(--pg-fg-default)]"
      : "bg-[color:var(--pg-neutral-3)] text-[color:var(--pg-fg-muted)] group-hover:bg-[color:var(--pg-neutral-6)] group-hover:text-[color:var(--pg-fg-default)]"
  }`,
  typeBadge:
    "shrink-0 rounded-[2px] bg-[color:var(--pg-neutral-3)] px-1.5 py-0.5 font-medium text-[color:var(--pg-fg-muted)]",
};
```

- [ ] **Step 2: Replace remaining `--cp-*` references in JSX**

Search-and-replace throughout the file:

| Line | Old | New |
|---|---|---|
| 331 | `text-[color:var(--cp-text-primary)]` | `text-[color:var(--pg-fg-default)]` |
| 335 | `text-[color:var(--cp-favorite)]` | `text-[color:var(--pg-favorite)]` |
| 388 | `text-[rgba(var(--cp-accent-primary-rgb),0.8)]` | `text-[color:var(--pg-accent-fg)]/80` |
| 389 | `text-[color:var(--cp-text-muted)]` | `text-[color:var(--pg-fg-subtle)]` |
| 402 | `text-[color:var(--cp-favorite)]` | `text-[color:var(--pg-favorite)]` |

- [ ] **Step 3: Verify**

Run: `grep -n "cp-" src/features/picker/PickerShell.tsx`

Expected: No matches

- [ ] **Step 4: Commit**

```bash
git add src/features/picker/PickerShell.tsx
git commit -m "style: migrate PickerShell STYLES from Catppuccin to Primer variables"
```

---

### Task 9: Update `WorkbenchShell.tsx` styles

**Files:**
- Modify: `src/features/workbench/WorkbenchShell.tsx:27-51` (STYLES constant)
- Modify: `src/features/workbench/WorkbenchShell.tsx` (inline `--cp-*` references in JSX)

**CSS Variable Mapping:**

| Old Catppuccin | New Primer |
|---|---|
| `--cp-window-shell` / `--cp-base` / `--cp-base-rgb` | `--pg-canvas-default` |
| `--cp-mantle` / `--cp-mantle-rgb` | `--pg-canvas-subtle` |
| `--cp-crust` / `--cp-crust-rgb` | `--pg-neutral-1` |
| `--cp-surface0` / `--cp-surface0-rgb` | `--pg-neutral-3` |
| `--cp-surface1` / `--cp-surface1-rgb` | `--pg-neutral-6` |
| `--cp-surface2` / `--cp-surface2-rgb` | `--pg-neutral-7` |
| `--cp-border-weak` | `--pg-border-muted` |
| `--cp-text-primary` | `--pg-fg-default` |
| `--cp-text-secondary` | `--pg-fg-muted` |
| `--cp-text-muted` | `--pg-fg-subtle` |
| `--cp-peach` / `--cp-peach-rgb` | `--pg-accent-fg` / `--pg-accent-subtle` |
| `--cp-accent-primary` | `--pg-accent-fg` |
| `--cp-danger` | `--pg-danger-fg` |
| `--cp-red` / `--cp-red-rgb` | `--pg-danger-fg` / `--pg-danger-subtle` |

- [ ] **Step 1: Replace STYLES constant (lines 27-51)**

Replace with:

```typescript
const STYLES = {
  shell:
    "flex h-screen w-screen flex-col overflow-hidden bg-[color:var(--pg-canvas-default)] text-[color:var(--pg-fg-default)]",
  header:
    "flex shrink-0 items-center gap-4 border-b border-[color:var(--pg-border-subtle)] bg-[color:var(--pg-canvas-subtle)] px-4 py-3",
  searchInput:
    "flex-1 rounded-lg border border-[color:var(--pg-border-default)] bg-[color:var(--pg-canvas-inset)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[color:var(--pg-fg-subtle)] focus:border-[color:var(--pg-accent-fg)] focus:ring-1 focus:ring-[color:var(--pg-accent-fg)]",
  closeButton:
    "rounded-md border border-[color:var(--pg-border-default)] px-3 py-2 text-sm transition-colors hover:bg-[color:var(--pg-canvas-subtle)]",
  sidebar:
    "w-[360px] shrink-0 border-r border-[color:var(--pg-border-muted)] bg-[color:var(--pg-canvas-subtle)]",
  listItem: (selected: boolean) =>
    `w-full border-b border-[color:var(--pg-border-muted)] px-4 py-3 text-left transition-colors hover:bg-[color:var(--pg-accent-subtle)] ${
      selected
        ? "bg-[color:var(--pg-accent-subtle)]"
        : ""
    }`,
  detailPanel: "min-w-0 flex-1 px-5 py-4 bg-[color:var(--pg-canvas-default)]",
  detailCard:
    "rounded-lg border border-[color:var(--pg-border-default)] bg-[color:var(--pg-canvas-subtle)] p-4",
  textPreview:
    "min-h-0 flex-1 rounded-lg border border-[color:var(--pg-border-default)] bg-[color:var(--pg-canvas-subtle)] p-4",
  nonTextNotice:
    "rounded-lg border border-[color:var(--pg-border-default)] bg-[color:var(--pg-canvas-subtle)] p-4 text-sm leading-6 text-[color:var(--pg-fg-muted)]",
};
```

- [ ] **Step 2: Replace remaining `--cp-*` references in JSX**

Global search-replace in WorkbenchShell.tsx:

| Pattern | Replacement |
|---|---|
| `text-[color:var(--cp-text-primary)]` | `text-[color:var(--pg-fg-default)]` |
| `text-[color:var(--cp-text-secondary)]` | `text-[color:var(--pg-fg-muted)]` |
| `text-[color:var(--cp-text-muted)]` | `text-[color:var(--pg-fg-subtle)]` |
| `text-cp-base` | `text-[color:var(--pg-fg-on-emphasis)]` |
| `bg-[color:var(--cp-accent-primary)]` | `bg-[color:var(--pg-accent-emphasis)]` |
| `border-[rgba(var(--cp-red-rgb),0.18)]` | `border-[color:var(--pg-danger-fg)]/20` |
| `bg-[rgba(var(--cp-red-rgb),0.08)]` | `bg-[color:var(--pg-danger-subtle)]` |
| `text-[color:var(--cp-danger)]` | `text-[color:var(--pg-danger-fg)]` |

- [ ] **Step 3: Verify**

Run: `grep -n "cp-" src/features/workbench/WorkbenchShell.tsx`

Expected: No matches

- [ ] **Step 4: Commit**

```bash
git add src/features/workbench/WorkbenchShell.tsx
git commit -m "style: migrate WorkbenchShell STYLES from Catppuccin to Primer variables"
```

---

### Task 10: Update `EditorShell.tsx` styles

**Files:**
- Modify: `src/features/editor/EditorShell.tsx` (all inline `--cp-*` references)

- [ ] **Step 1: Replace all `--cp-*` references**

Global search-replace in EditorShell.tsx:

| Old | New |
|---|---|
| `bg-[color:var(--cp-window-shell)]` | `bg-[color:var(--pg-canvas-default)]` |
| `border-[color:var(--cp-border-weak)]` | `border-[color:var(--pg-border-muted)]` |
| `text-[color:var(--cp-text-muted)]` | `text-[color:var(--pg-fg-subtle)]` |
| `text-[color:var(--cp-text-primary)]` | `text-[color:var(--pg-fg-default)]` |
| `text-[color:var(--cp-text-secondary)]` | `text-[color:var(--pg-fg-muted)]` |
| `text-cp-base` | `text-[color:var(--pg-fg-on-emphasis)]` |
| `hover:bg-[rgba(var(--cp-surface1-rgb),0.12)]` | `hover:bg-[color:var(--pg-canvas-subtle)]` |
| `bg-cp-mantle` | `bg-[color:var(--pg-canvas-subtle)]` |
| `focus:border-[rgba(var(--cp-peach-rgb),0.35)]` | `focus:border-[color:var(--pg-accent-fg)]` |
| `focus:bg-[color:var(--cp-window-shell)]` | `focus:bg-[color:var(--pg-canvas-inset)]` |
| `focus:shadow-[rgba(var(--cp-peach-rgb),0.08)]` | (remove this shadow, or use `focus:shadow-pg-sm`) |
| `dark:bg-[rgba(var(--cp-surface0-rgb),0.2)]` | (remove dark: overrides, Primer handles via CSS vars) |
| `dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]` | (remove dark: overrides) |
| `bg-[rgba(var(--cp-surface1-rgb),0.08)]` | `bg-[color:var(--pg-canvas-subtle)]` |
| `border-[rgba(var(--cp-surface1-rgb),0.3)]` | `border-[color:var(--pg-border-default)]` |
| `bg-[rgba(var(--cp-green-rgb),0.16)]` | `border-[color:var(--pg-success-fg)]/20` |
| `bg-[rgba(var(--cp-green-rgb),0.08)]` | `bg-[color:var(--pg-success-subtle)]` |
| `text-[color:var(--cp-success)]` | `text-[color:var(--pg-success-fg)]` |
| `border-[rgba(var(--cp-red-rgb),0.16)]` | `border-[color:var(--pg-danger-fg)]/20` |
| `bg-[rgba(var(--cp-red-rgb),0.08)]` | `bg-[color:var(--pg-danger-subtle)]` |
| `text-[color:var(--cp-danger)]` | `text-[color:var(--pg-danger-fg)]` |
| `bg-[color:var(--cp-accent-primary)]` | `bg-[color:var(--pg-accent-emphasis)]` |

- [ ] **Step 2: Verify**

Run: `grep -n "cp-" src/features/editor/EditorShell.tsx`

Expected: No matches

- [ ] **Step 3: Commit**

```bash
git add src/features/editor/EditorShell.tsx
git commit -m "style: migrate EditorShell from Catppuccin to Primer variables"
```

---

### Task 11: Update `Panel.tsx`

**Files:**
- Modify: `src/shared/ui/Panel.tsx`

- [ ] **Step 1: Replace Catppuccin references**

Replace the className in Panel.tsx:

```diff
-className={`rounded-lg border border-cp-surface1/30 bg-cp-mantle/70 p-5 sm:p-6 shadow-xs backdrop-blur-md transition-all duration-250 hover:border-cp-surface1/50 hover:bg-cp-mantle/85 ${className}`}
+className={`rounded-lg border border-[color:var(--pg-border-muted)] bg-[color:var(--pg-canvas-subtle)] p-5 sm:p-6 shadow-pg-sm transition-all duration-250 hover:border-[color:var(--pg-border-default)] ${className}`}
```

- [ ] **Step 2: Commit**

```bash
git add src/shared/ui/Panel.tsx
git commit -m "style: migrate Panel component from Catppuccin to Primer variables"
```

---

### Task 12: Update `StatusBadge.tsx`

**Files:**
- Modify: `src/shared/ui/StatusBadge.tsx`

Replace RGB-based rgba() calls with Primer semantic subtle colors.

- [ ] **Step 1: Replace toneClassMap**

```diff
 const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
-  running: "bg-[rgba(var(--cp-green-rgb),0.15)] text-[color:var(--cp-green)] ring-1 ring-inset ring-[rgba(var(--cp-green-rgb),0.3)]",
-  paused: "bg-[rgba(var(--cp-yellow-rgb),0.15)] text-[color:var(--cp-favorite)] ring-1 ring-inset ring-[rgba(var(--cp-yellow-rgb),0.3)]",
-  muted: "bg-[rgba(var(--cp-surface0-rgb),0.6)] text-[color:var(--cp-text-muted)] ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.4)]",
+  running: "bg-[color:var(--pg-success-subtle)] text-[color:var(--pg-success-fg)] ring-1 ring-inset ring-[color:var(--pg-success-fg)]/20",
+  paused: "bg-[color:var(--pg-warning-subtle)] text-[color:var(--pg-warning-fg)] ring-1 ring-inset ring-[color:var(--pg-warning-fg)]/20",
+  muted: "bg-[color:var(--pg-neutral-3)] text-[color:var(--pg-fg-subtle)] ring-1 ring-inset ring-[color:var(--pg-neutral-6)]/40",
 };
```

- [ ] **Step 2: Commit**

```bash
git add src/shared/ui/StatusBadge.tsx
git commit -m "style: migrate StatusBadge from Catppuccin to Primer semantic colors"
```

---

### Task 13: Update `LoadingSpinner.tsx`

**Files:**
- Modify: `src/shared/ui/LoadingSpinner.tsx`

- [ ] **Step 1: Replace color references**

```diff
-className={`${sizeClasses[size]} animate-spin rounded-full border-[color:var(--cp-surface1)] border-t-[color:var(--cp-accent-primary)]`}
+className={`${sizeClasses[size]} animate-spin rounded-full border-[color:var(--pg-neutral-6)] border-t-[color:var(--pg-accent-fg)]`}
```

```diff
-<span className="text-sm font-medium text-[color:var(--cp-text-secondary)]">
+<span className="text-sm font-medium text-[color:var(--pg-fg-muted)]">
```

- [ ] **Step 2: Commit**

```bash
git add src/shared/ui/LoadingSpinner.tsx
git commit -m "style: migrate LoadingSpinner from Catppuccin to Primer colors"
```

---

### Task 14: Delete `EmptyState.tsx`

**Files:**
- Delete: `src/shared/components/EmptyState.tsx`

EmptyState was only used by ManagerShell (for history list and favorites). After rewriting ManagerShell as a pure settings page, this component is no longer needed.

- [ ] **Step 1: Verify no remaining imports**

Run: `grep -rn "EmptyState" src/`

Expected: No matches (ManagerShell was rewritten without it)

- [ ] **Step 2: Delete the file**

Run: `rm src/shared/components/EmptyState.tsx`

- [ ] **Step 3: Remove empty components directory if needed**

Run: `ls src/shared/components/`

If empty, also `rmdir src/shared/components/`

- [ ] **Step 4: Commit**

```bash
git add -u src/shared/components/
git commit -m "refactor: remove EmptyState component (no longer used after ManagerShell rewrite)"
```

---

### Task 15: Final Verification

- [ ] **Step 1: Full TypeScript check**

Run: `rtk tsc`

Expected: No errors

- [ ] **Step 2: Check for any remaining `--cp-` or `cp-` references**

Run: `grep -rn "cp-\|--cp-" src/ tailwind.config.ts`

Expected: No matches in source files

- [ ] **Step 3: Check for any remaining legacy Tailwind aliases**

Run: `grep -rn "text-ink\b\|bg-cp\|border-cp\|text-cp" src/`

Expected: No matches

- [ ] **Step 4: Build verification**

Run: `rtk vite build`

Expected: Build succeeds

- [ ] **Step 5: Visual verification**

Open each window (manager, picker, workbench, editor) and verify:
- [ ] Manager window shows pure settings page, max-width 680px, centered
- [ ] Light theme uses Primer neutral cool tones
- [ ] Dark theme uses Primer dark palette
- [ ] Picker window maintains transparency
- [ ] All form inputs have 1px border, blue focus ring on focus
- [ ] Radio buttons and checkboxes use accent color
- [ ] Status badges render with correct semantic colors
- [ ] Toggle between light/dark themes transitions smoothly

---

## Appendix: CSS Variable Migration Reference

### Global mapping (used across all files)

| Catppuccin | Primer |
|---|---|
| `--cp-text-primary` | `--pg-fg-default` |
| `--cp-text-secondary` | `--pg-fg-muted` |
| `--cp-text-muted` | `--pg-fg-subtle` |
| `--cp-text` (primitive) | `--pg-fg-default` |
| `--cp-base` | `--pg-canvas-default` |
| `--cp-mantle` | `--pg-canvas-subtle` |
| `--cp-crust` | `--pg-neutral-1` |
| `--cp-window-shell` | `--pg-canvas-default` |
| `--cp-panel-surface` | `--pg-canvas-subtle` |
| `--cp-control-surface` | `--pg-canvas-subtle` |
| `--cp-card-surface` | `--pg-canvas-default` |
| `--cp-border-weak` | `--pg-border-muted` |
| `--cp-border-medium` | `--pg-border-default` |
| `--cp-border-strong` | `--pg-border-subtle` |
| `--cp-accent-primary` | `--pg-accent-fg` |
| `--cp-accent-primary-strong` | `--pg-accent-emphasis` |
| `--cp-favorite` | `--pg-favorite` |
| `--cp-success` | `--pg-success-fg` |
| `--cp-danger` | `--pg-danger-fg` |
| `--cp-warning` | `--pg-warning-fg` |
| `text-cp-base` (button text) | `text-[color:var(--pg-fg-on-emphasis)]` |

### Key pattern changes

| Old Catppuccin Pattern | New Primer Pattern |
|---|---|
| `rgba(var(--cp-surface1-rgb), 0.15)` | `color-mix(in srgb, var(--pg-neutral-6) 15%, transparent)` or direct subtle color |
| `bg-cp-mantle/70` | `bg-[color:var(--pg-canvas-subtle)]` |
| `bg-[rgba(var(--cp-surface0-rgb),0.2)]` | `bg-[color:var(--pg-neutral-3)]` |
| `dark:bg-[rgba(var(--cp-surface0-rgb),0.2)]` | Remove dark: override (CSS vars handle it) |
| `backdrop-blur-md` | Remove (Primer prefers solid backgrounds) |
