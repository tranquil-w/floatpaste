# FloatPaste 视觉身份重构 实施计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 重构 FloatPaste 的视觉身份——统一模块命名、建立品牌锚点、重构 Search 窗口布局、增强收藏交互、优化设置页和空状态。

**Architecture:** 分 4 个 Chunk 按依赖顺序执行。Chunk 1（重命名）是基础，Chunk 2-4 互相独立但依赖重命名完成。每个 Chunk 产出一个可独立构建验证的状态。

**Tech Stack:** Tauri (Rust) + React + TypeScript + Tailwind CSS + Primer 主题系统 + TanStack Query

**设计规范：** `docs/superpowers/specs/2026-03-28-visual-identity-redesign.md`

---

## Chunk 1: 模块重命名 (workbench→search, manager→settings)

重命名是所有后续工作的前提。先改 Rust 后端（编译验证），再改前端（构建验证）。

### Task 1: Rust 后端重命名

**Files:**
- Rename: `src-tauri/src/domain/workbench_session.rs` → `search_session.rs`
- Modify: `src-tauri/src/domain/mod.rs` — 模块声明
- Modify: `src-tauri/src/services/window_coordinator.rs` — 所有 `workbench` → `search`
- Modify: `src-tauri/src/services/paste_executor.rs` — 所有 `workbench` → `search`
- Modify: `src-tauri/src/services/shortcut_manager.rs` — 引用 + 中文日志
- Modify: `src-tauri/src/domain/settings.rs` — settings 字段名
- Modify: `src-tauri/src/services/settings_service.rs` — 引用
- Modify: `src-tauri/src/commands/windows.rs` — 命令函数名
- Modify: `src-tauri/src/domain/editor_session.rs` — `EditorSource::Workbench` → `Search`，`EditorReturnTarget::Workbench` → `Search`
- Modify: `src-tauri/src/domain/events.rs` — 事件常量
- Modify: `src-tauri/src/app_bootstrap.rs` — 引用
- Rename: `src-tauri/capabilities/workbench.json` → `search.json`
- Modify: `src-tauri/tauri.conf.json` — capabilities 引用 + window titles
- Modify: `src-tauri/src/lib.rs` — 模块/事件注册

- [ ] **Step 1: Rust 后端全局替换 workbench→search**

对以下文件执行 `workbench` → `search`（注意保留 Tauri window label `"workbench"` 不改）：
- `window_coordinator.rs` — 函数名、变量名、字符串字面量
- `paste_executor.rs` — 函数调用
- `shortcut_manager.rs` — 设置字段引用 + 中文日志 `"工作窗"` → `"搜索"`
- `settings.rs` — 字段名 `workbench_shortcut` → `search_shortcut`，`workbench_shortcut_enabled` → `search_shortcut_enabled`
- `settings_service.rs` — 字段引用
- `commands/windows.rs` — 函数名 `open_workbench` → `open_search`，`close_workbench` → `close_search`
- `editor_session.rs` — 枚举变体 `Workbench` → `Search`
- `events.rs` — 事件常量字符串
- `app_bootstrap.rs` — 引用更新
- `lib.rs` — 模块注册

- [ ] **Step 2: 重命名 workbench_session.rs 文件**

```bash
cd src-tauri && git mv src/domain/workbench_session.rs src/domain/search_session.rs
```

更新文件内所有 `WorkbenchSession` → `SearchSession`，`workbench_session` → `search_session`。
更新 `src/domain/mod.rs` 中的 `mod workbench_session` → `mod search_session`。

- [ ] **Step 3: 重命名 capabilities 文件**

```bash
cd src-tauri && git mv capabilities/workbench.json capabilities/search.json
```

更新 `capabilities/search.json` 内的 `identifier`（`"workbench-capability"` → `"search-capability"`）和 `windows`（`["workbench"]` 保持不变，因为 Tauri window label 不改）。

更新 `tauri.conf.json` 中的 `"workbench-capability"` → `"search-capability"`。

- [ ] **Step 4: 更新 window titles**

在 `tauri.conf.json` 中修改 4 个窗口的 `title`：
- `manager`: `"FloatPaste / 浮贴"` → `"FloatPaste · 设置"`
- `picker`: `"FloatPaste Picker"` → `"FloatPaste · 速贴"`
- `workbench`: `"FloatPaste 搜索"` → `"FloatPaste · 搜索"`
- `editor`: `"FloatPaste Editor"` → `"FloatPaste · 编辑"`

- [ ] **Step 5: Cargo build 验证**

```bash
cd src-tauri && cargo build 2>&1 | head -50
```

Expected: 编译成功，无错误。如果有错误，逐一修复。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "refactor: 重命名 workbench→search, manager→settings (Rust 后端)"
```

---

### Task 2: 前端重命名

**Files:**
- Rename: `src/features/workbench/` → `src/features/search/`
- Rename: `src/features/manager/` → `src/features/settings/`
- Modify: `src/app/App.tsx` — 导入路径 + CSS 类名 + 返回值类型
- Modify: `src/index.css` — CSS 类名
- Modify: `src/bridge/events.ts` — 事件常量
- Modify: `src/bridge/commands.ts` — bridge 函数
- Modify: `src/bridge/window.ts` — 返回值类型
- Modify: `src/bridge/mockBackend.ts` — mock
- Modify: `src/shared/types/settings.ts` — settings 类型
- Modify: `src/shared/queries/clipQueries.ts` — 查询 key
- Modify: `src/features/picker/PickerShell.tsx` — 跨模块引用
- Modify: `src/features/editor/store.ts` — source 字段值
- Modify: `src/features/editor/EditorShell.tsx` — 条件判断

- [ ] **Step 1: 重命名前端目录**

```bash
cd src && git mv features/workbench features/search && git mv features/manager features/settings
```

- [ ] **Step 2: 重命名 Search 目录内的文件和导出**

```bash
cd src/features/search && git mv WorkbenchShell.tsx SearchShell.tsx
```

在所有重命名文件中更新内部引用：
- `WorkbenchShell` → `SearchShell`
- `useWorkbenchStore` → `useSearchStore`
- `useWorkbenchRecentQuery` → `useSearchRecentQuery`
- `useWorkbenchSearchQuery` → `useSearchSearchQuery`（考虑改名为 `useSearchQuery`）
- `getNextWorkbenchNavigationIndex` → `getNextSearchNavigationIndex`
- `getWorkbenchKeyboardAction` → `getSearchKeyboardAction`
- `index.ts` 中的导出名更新

在 `src/features/settings/` 中：
- `ManagerShell` → `SettingsShell`

- [ ] **Step 3: 更新 bridge 层**

`src/bridge/events.ts` — 全部 `WORKBENCH_` → `SEARCH_`，`WORKBENCH_SEARCH_EVENT` → `SEARCH_QUERY_EVENT`
`src/bridge/commands.ts` — `openWorkbench` → `openSearch`，`hideWorkbench` → `hideSearch`，`openEditorFromWorkbench` → `openEditorFromSearch`
`src/bridge/window.ts` — 返回值类型 `"picker" | "workbench" | "editor" | "manager"` → `"picker" | "search" | "editor" | "settings"`（注意：这里改的是 TypeScript 类型，但实际 Tauri window label 不变，所以比较条件仍用 `"workbench"` / `"manager"`）
`src/bridge/mockBackend.ts` — 同步更新所有 workbench/manager 引用

- [ ] **Step 4: 更新 App.tsx**

- 导入路径：`../features/search/SearchShell`，`../features/settings/SettingsShell`
- CSS 类名：`"window-workbench"` → `"window-search"`，`"theme-workbench"` → `"theme-search"`，`"window-manager"` → `"window-settings"`，`"theme-manager"` → `"theme-settings"`
- **重要**：window label 条件分支保持用字面量 `"workbench"` / `"manager"` / `"picker"` / `"editor"`（因为 `getCurrentWindowLabel()` 返回的是 Tauri 底层 label，不变）

- [ ] **Step 5: 更新共享类型和查询**

`src/shared/types/settings.ts` — `workbenchShortcut` → `searchShortcut`，`workbenchShortcutEnabled` → `searchShortcutEnabled`
`src/shared/queries/clipQueries.ts` — 如有 workbench 相关查询 key 则更新

- [ ] **Step 6: 更新 Picker 和 Editor 跨模块引用**

`src/features/picker/PickerShell.tsx` — `openEditorFromWorkbench` → `openEditorFromSearch`，所有 `WORKBENCH_*` 事件名
`src/features/editor/store.ts` — `EditorSession` 类型中 source 值如有 `"workbench"` → `"search"`
`src/features/editor/EditorShell.tsx` — `session.source === "picker"` 条件判断（source 值来自 Rust JSON 序列化，需确认新值）

- [ ] **Step 7: 更新 CSS 类名**

`src/index.css`:
- `body.theme-workbench` → `body.theme-search`
- `body.theme-manager` → `body.theme-settings`
- `html.window-workbench` → `html.window-search`
- `html.window-manager` → `html.window-settings`

- [ ] **Step 8: pnpm build 验证**

```bash
pnpm build 2>&1
```

Expected: 构建成功，无 TypeScript 错误。

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "refactor: 重命名 workbench→search, manager→settings (前端)"
```

---

## Chunk 2: Search 窗口布局重构 + 收藏交互

### Task 3: SearchShell 单栏布局重构

**Files:**
- Rewrite: `src/features/search/SearchShell.tsx`

- [ ] **Step 1: 重构 SearchShell 布局**

将双栏布局（aside + section）改为单栏 + 内联展开：

1. 移除 header 区域（uppercase 标签 + 标题 + data-tauri-drag-region）
2. 顶部添加蓝色渐变条 `<div>`
3. 搜索框改为居中布局（`max-w-[460px] mx-auto py-3`），独立搜索区
4. 移除 `<aside>` 侧边栏容器，改为单栏列表
5. 选中项内联展开 `detailQuery.data` 内容（文本预览 + 操作按钮）
6. 操作按钮使用 `onMouseDown` + `preventDefault` + `onClick` 双绑定
7. 添加底部快捷键提示栏
8. 收藏项渲染 `isFavorited` 状态（左侧色条 + 星号）

保持不变：`useWorkbenchStore`（→ `useSearchStore`）的 state 管理、键盘事件处理逻辑、Tauri 事件监听、`inputSuspended` 逻辑。

- [ ] **Step 2: pnpm build 验证**

```bash
pnpm build 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: Search 窗口重构为单栏布局 + 内联展开 + 收藏渲染"
```

---

### Task 4: Search 收藏切换按钮

**Files:**
- Modify: `src/features/search/SearchShell.tsx`

- [ ] **Step 1: 添加收藏按钮到操作按钮栏**

在内联展开区域的操作按钮中新增"收藏"按钮：
- 按钮文案根据 `item.isFavorited` 切换："收藏" / "取消收藏"
- 调用 `setItemFavorited(id, !item.isFavorited)`（已有 bridge 函数）
- 使用 `onMouseDown` + `preventDefault` 保持搜索框聚焦
- 操作成功后乐观更新列表（`CLIPS_CHANGED_EVENT` 会自动刷新）

- [ ] **Step 2: pnpm build 验证**

```bash
pnpm build 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: Search 窗口添加收藏切换按钮"
```

---

## Chunk 3: Picker 视觉增强 + 收藏交互

### Task 5: Picker 视觉增强

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`

- [ ] **Step 1: 添加品牌身份锚点**

1. 在 `STYLES.container` 内部最顶部（header 之前）添加渐变条：
   ```tsx
   <div className="h-[3px] w-full bg-gradient-to-r from-pg-blue-5 to-pg-blue-4 shrink-0 rounded-t-md" />
   ```
2. `headerDot` 颜色 `bg-pg-neutral-7` → `bg-pg-accent-fg`
3. 标题字重 `font-bold` → `font-extrabold`

- [ ] **Step 2: 收藏视觉强化**

1. 收藏条目加左侧 3px 蓝色色条（未选中态）
2. 星号 `text-[10px]` → `text-[12px]`，移除 `opacity-80`
3. 选中态 kbd badge 反转（蓝底白字）
4. 收藏条目内容预览字重提升到 `font-medium`

- [ ] **Step 3: 添加空状态**

在列表区域添加 `items.length === 0` 且非加载中的空状态分支：
```tsx
{!recent.isLoading && items.length === 0 && (
  <div className="flex flex-col items-center justify-center flex-1 gap-1 py-8">
    <p className="text-sm text-pg-fg-muted">暂无剪贴板记录</p>
    <p className="text-xs text-pg-fg-subtle">复制内容后按 Alt+Q 打开此面板</p>
  </div>
)}
```

- [ ] **Step 4: 替换加载状态为 LoadingSpinner**

引入 `LoadingSpinner` 组件替换纯文字加载状态。

- [ ] **Step 5: pnpm build 验证**

```bash
pnpm build 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: Picker 品牌锚点 + 收藏视觉强化 + 空状态 + LoadingSpinner"
```

---

### Task 6: Picker 收藏快捷键

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`

- [ ] **Step 1: 添加 Ctrl+F 快捷键**

在浏览器模式的键盘事件处理中新增 `Ctrl+F` 处理：
- 调用 `setItemFavorited(itemsRef.current[selectedIndexRef.current].id, !item.isFavorited)`
- 设置 `lastMessage` 提示"已收藏"或"已取消收藏"
- 注意：Ctrl+F 在 Picker 中不会与系统/浏览器快捷键冲突（Picker 无搜索功能）

- [ ] **Step 2: 删除右上角编辑按钮**

移除 PickerShell header 中的"编辑"按钮（`PickerShell.tsx` 中 `<button ... disabled={!canEditSelected} onClick={handleOpenEditor}` 部分）。
`Ctrl+Enter` 打开编辑器的快捷键仍然保留。

- [ ] **Step 3: pnpm build 验证**

```bash
pnpm build 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: Picker Ctrl+F 收藏切换 + 移除编辑按钮"
```

---

## Chunk 4: 设置页 + Editor + CSS 清理

### Task 7: Settings 自动保存 + 视觉优化

**Files:**
- Modify: `src/features/settings/SettingsShell.tsx`

- [ ] **Step 1: 实现自动保存**

1. 移除底部"保存设置"按钮及其容器 `<div className="pt-2 pb-8">`
2. 添加 `saveStatus` state（`"idle" | "saving" | "saved" | "error"`）
3. 用 `useEffect` + `setTimeout` 实现 800ms debounce 自动保存
4. 使用 `isInitializingRef` flag 避免初始化阶段触发
5. header 区域显示保存状态（"已保存" 文字闪现）
6. 保留错误提示条

- [ ] **Step 2: 间距节奏和标题层次**

1. Section 间距交替：快捷键 `mb-10`，通用 `mb-6`，外观 `mb-10`，行为 `mb-6`，排除应用 `mb-10`
2. Section 标题 `text-sm` → `text-base`，`mt-4` → `mt-5`

- [ ] **Step 3: 添加渐变条 + LoadingSpinner**

1. `<main>` 顶部添加渐变条
2. 加载状态替换为 `<LoadingSpinner size="sm" />`

- [ ] **Step 4: pnpm build 验证**

```bash
pnpm build 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: 设置页自动保存 + 间距节奏 + 品牌锚点"
```

---

### Task 8: Editor 视觉优化 + CSS 清理

**Files:**
- Modify: `src/features/editor/EditorShell.tsx`
- Modify: `src/index.css`

- [ ] **Step 1: Editor header 简化**

1. 移除来源标签（`getSourceLabel` 的 uppercase 小标签）和副标题（"独立编辑窗口"）
2. 简化为单行标题"编辑器"
3. 添加渐变条到 header 之前

- [ ] **Step 2: Editor 空状态和加载状态**

1. 加载状态替换为 `<LoadingSpinner size="sm" />`
2. "等待编辑会话启动"空状态补充引导文案："在速贴面板或搜索窗口中选中文本条目后按 Ctrl+Enter 进入编辑"
3. "未找到对应条目"和"当前条目不支持文本编辑"状态文字颜色从 `text-pg-fg-subtle` → `text-pg-fg-muted`

- [ ] **Step 3: CSS 清理**

1. 移除 `body.theme-workbench` 的 `radial-gradient` 装饰背景 → 纯 `var(--pg-canvas-default)`
2. 确认所有 `window-workbench` / `theme-workbench` CSS 类已在 Chunk 1 中重命名

- [ ] **Step 4: pnpm build 验证**

```bash
pnpm build 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: Editor header 简化 + 空状态优化 + CSS 清理"
```

---

## Chunk 5: 最终验证

### Task 9: 全量构建验证

- [ ] **Step 1: 前端构建**

```bash
pnpm build 2>&1
```

Expected: 0 errors

- [ ] **Step 2: Rust 编译**

```bash
cd src-tauri && cargo build 2>&1 | tail -5
```

Expected: `Finished` 无错误

- [ ] **Step 3: 搜索残留引用**

```bash
grep -r "workbench\|Workbench\|WORKBENCH_\|managerShortcut\|ManagerShell" src/ --include="*.ts" --include="*.tsx" -l
grep -r "workbench\|Workbench" src-tauri/src/ --include="*.rs" -l | grep -v "window_label\|\"workbench\""
```

Expected: 无残留（Tauri window label `"workbench"` 的硬编码字符串除外）

- [ ] **Step 4: 启动 dev 模式手动验证**

```bash
pnpm dev
```

验证项：
- [ ] 4 个窗口顶部蓝色渐变条可见
- [ ] Search 窗口为单栏布局，选中项内联展开
- [ ] Picker Ctrl+F 切换收藏，无编辑按钮
- [ ] Search 收藏按钮可点击，搜索框不失去焦点
- [ ] 设置页面修改后自动保存
- [ ] LoadingSpinner 在各窗口正确显示
- [ ] 键盘快捷键（Alt+Q, Alt+S, Ctrl+Enter, Esc, 方向键, 数字键）全部正常
