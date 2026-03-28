# FloatPaste 视觉身份重构设计规范

**日期：** 2026-03-28
**状态：** 待审查
**范围：** 全面重构——品牌身份、模块重命名、Workbench 布局重构、收藏强化（含交互入口）、设置页优化、空状态/加载状态

---

## 背景与目标

FloatPaste 是一个 Windows 桌面剪贴板管理器（Tauri + React），包含 4 个独立窗口：速贴 (Picker)、搜索 (Workbench→Search)、设置 (Manager→Settings)、编辑 (Editor)。当前界面功能完整但缺乏视觉辨识度和个性。

**设计目标：**
- 建立可辨识的品牌视觉身份
- 提升信息效率（特别是 Workbench）
- 优化视觉层次和交互节奏
- 保留 Primer 蓝色主色调和现有语义化 CSS 变量体系
- 调性：冷静专业，效率+精致

**不改动：**
- Primer 色彩变量体系（`--pg-*`）
- 键盘导航交互语义
- Tauri window label（`picker`、`editor`、`manager`、`workbench` 保持不变，作为底层标识）
- 组件逻辑和状态管理（除命名变更外）
- Rust 后端（除命名变更外）

---

## 0. 模块重命名

### 0.1 命名规则

所有 4 个窗口的用户可见名称统一为 **2 个中文字**：速贴、搜索、设置、编辑。

代码层命名统一使用英文，遵循规则：
- `workbench` → `search`
- `manager` → `settings`
- `picker` 和 `editor` 保持不变

### 0.2 Window Title 变更

| 窗口 | 当前 title | 新 title |
|------|-----------|----------|
| Picker | `FloatPaste Picker` | `FloatPaste · 速贴` |
| Workbench | `FloatPaste 搜索` | `FloatPaste · 搜索` |
| Manager | `FloatPaste / 浮贴` | `FloatPaste · 设置` |
| Editor | `FloatPaste Editor` | `FloatPaste · 编辑` |

修改位置：`src-tauri/tauri.conf.json` 中各窗口的 `title` 字段。

同步更新 `src-tauri/tauri.conf.json` 中 capabilities 引用：`"workbench-capability"` → `"search-capability"`。
同步重命名 `src-tauri/capabilities/workbench.json` → `src-tauri/capabilities/search.json`，并更新其中的 `identifier` 和 `windows` 字段。

### 0.3 前端文件/目录重命名

| 当前路径 | 新路径 |
|---------|--------|
| `src/features/workbench/` | `src/features/search/` |
| `src/features/manager/` | `src/features/settings/` |
| `WorkbenchShell.tsx` | `SearchShell.tsx` |
| `ManagerShell.tsx` | `SettingsShell.tsx` |
| `workbench/index.ts` | `search/index.ts` |
| `workbench/keyboard.ts` | `search/keyboard.ts` |
| `workbench/queries.ts` | `search/queries.ts` |
| `workbench/state.ts` | `search/state.ts` |
| `workbench/store.ts` | `search/store.ts` |

`picker/` 和 `editor/` 目录保持不变。

### 0.4 前端代码引用重命名

| 当前 | 新 | 说明 |
|------|-----|------|
| `WORKBENCH_SESSION_START_EVENT` | `SEARCH_SESSION_START_EVENT` | |
| `WORKBENCH_SESSION_END_EVENT` | `SEARCH_SESSION_END_EVENT` | |
| `WORKBENCH_NAVIGATE_EVENT` | `SEARCH_NAVIGATE_EVENT` | |
| `WORKBENCH_EDIT_ITEM_EVENT` | `SEARCH_EDIT_ITEM_EVENT` | |
| `WORKBENCH_PASTE_EVENT` | `SEARCH_PASTE_EVENT` | |
| `WORKBENCH_INPUT_SUSPEND_EVENT` | `SEARCH_INPUT_SUSPEND_EVENT` | |
| `WORKBENCH_INPUT_RESUME_EVENT` | `SEARCH_INPUT_RESUME_EVENT` | |
| `openWorkbench()` | `openSearch()` | bridge/commands.ts |
| `hideWorkbench()` | `hideSearch()` | bridge/commands.ts |
| `openEditorFromWorkbench()` | `openEditorFromSearch()` | bridge/commands.ts |
| `WorkbenchShell` | `SearchShell` | 组件导入 |
| `useWorkbenchStore` | `useSearchStore` | store 导入 |
| `useWorkbenchRecentQuery` | `useSearchRecentQuery` | queries 导入 |
| `useWorkbenchSearchQuery` | `useSearchSearchQuery` | queries 导入 |
| `getNextWorkbenchNavigationIndex` | `getNextSearchNavigationIndex` | state 导入 |
| `getWorkbenchKeyboardAction` | `getSearchKeyboardAction` | keyboard 导入 |
| `workbenchShortcut` | `searchShortcut` | settings 类型 |
| `workbenchShortcutEnabled` | `searchShortcutEnabled` | settings 类型 |
| `window-workbench` / `theme-workbench` | `window-search` / `theme-search` | CSS 类名 |
| `window-manager` / `theme-manager` | `window-settings` / `theme-settings` | CSS 类名 |
| `WORKBENCH_SEARCH_EVENT` | `SEARCH_QUERY_EVENT` | 避免重复 search 语义 |
| `MANAGER_OPEN_SETTINGS_EVENT` | 保持不变 | 内部事件，改名后语义更清晰"settings://open" |

### 0.5 后端（Rust）重命名

| 当前 | 新 | 说明 |
|------|-----|------|
| `workbench_session.rs` | `search_session.rs` | domain/ |
| `WorkbenchSession` | `SearchSession` | struct 名 |
| `workbench_shortcut` | `search_shortcut` | settings 字段 |
| `workbench_shortcut_enabled` | `search_shortcut_enabled` | settings 字段 |
| `WORKBENCH_SESSION_START` | `SEARCH_SESSION_START` | 事件名 |
| `WORKBENCH_SESSION_END` | `SEARCH_SESSION_END` | 事件名 |
| `open_workbench()` | `open_search()` | 命令函数 |
| `close_workbench()` | `close_search()` | 命令函数 |
| `window_coordinator.rs` 中所有 `workbench` 引用 | → `search` | |
| `paste_executor.rs` 中所有 `workbench` 引用 | → `search` | |
| `editor_session.rs` 中 `EditorSource::Workbench` | → `EditorSource::Search` | 枚举变体 |
| `editor_session.rs` 中 `EditorReturnTarget::Workbench` | → `EditorReturnTarget::Search` | 枚举变体 |
| `EditorShell.tsx` 中 `session.source === "picker"` 条件 | `"picker"` 保持不变 | source 值需同步更新 JSON 序列化 |
| `shortcut_manager.rs` 中中文日志 | "工作窗" → "搜索" | 日志文案 |

### 0.6 数据库迁移

Settings 使用 KV 存储（`key TEXT PRIMARY KEY, value TEXT NOT NULL`），`workbench_shortcut` 不是数据库列，而是 settings 表中的一行 KV 记录。因此字段名变更**不需要数据库迁移**。

需确保 Rust 反序列化兼容：
- 使用 `#[serde(alias = "workbench_shortcut")]` 兼容旧数据
- 或在 settings service 读取时做 key 映射

### 0.7 涉及文件总览

**前端（~16 文件）：**
- `src-tauri/tauri.conf.json` — window title + capabilities 引用
- `src-tauri/capabilities/workbench.json` → `search.json` — capabilities 配置
- `src/app/App.tsx` — 导入路径 + CSS 类名 + `getCurrentWindowLabel()` 返回值类型 + window label 条件分支
- `src/index.css` — CSS 类名
- `src/bridge/events.ts` — 事件常量名（含 `WORKBENCH_SEARCH_EVENT` → `SEARCH_QUERY_EVENT`）
- `src/bridge/commands.ts` — bridge 函数名
- `src/bridge/window.ts` — 返回值类型 + 导入路径
- `src/bridge/mockBackend.ts` — mock 实现
- `src/shared/types/settings.ts` — settings 类型
- `src/shared/queries/clipQueries.ts` — 查询 key
- `src/features/workbench/*` → `src/features/search/*` — 全面重命名
- `src/features/manager/*` → `src/features/settings/*` — 全面重命名
- `src/features/picker/PickerShell.tsx` — 跨模块引用
- `src/features/editor/store.ts` — `EditorSession` 类型中 source 字段值
- `src/features/editor/EditorShell.tsx` — 条件判断 display text

---

## 1. 品牌身份锚点

### 1.1 蓝色渐变顶条

所有 4 个窗口顶部添加 3px 蓝色渐变条，使用现有 `--pg-blue-5` 到 `--pg-blue-4` 变量。

**实现方式：** 在各 Shell 组件的根元素顶部直接添加一个 `<div>` 元素作为渐变条，而非 CSS 伪元素。原因：Picker 窗口使用 `overflow-hidden` + `rounded-md` 圆角容器 + 透明 body 背景，CSS 伪元素可能被裁剪或与 header 层叠冲突。显式 `<div>` 更直观且不受布局影响。

```tsx
<div className="h-[3px] w-full bg-gradient-to-r from-pg-blue-5 to-pg-blue-4 shrink-0" />
```

**Picker 特殊处理：** 渐变条放在 `STYLES.container` 内部最顶部（header 之前）。Picker 的 container 有 `overflow-hidden rounded-md`，渐变条会被正确裁剪到圆角内。

```
渐变方向：左 → 右
起始色：var(--pg-blue-5)
结束色：var(--pg-blue-4)
高度：3px
```

**深色主题兼容：** 深色主题的 `--pg-blue-5` (#1f6feb) 和 `--pg-blue-4` (#388bfd) 在深色背景上对比度充足，无需额外处理。

**涉及文件：**
- `src/index.css` — 添加窗口级别的渐变条样式类
- `src/features/picker/PickerShell.tsx` — 在 container 顶部应用
- `src/features/workbench/WorkbenchShell.tsx` — 在 shell 顶部应用
- `src/features/manager/ManagerShell.tsx` — 在 main 容器顶部应用
- `src/features/editor/EditorShell.tsx` — 在根容器顶部应用

### 1.2 Picker Header 品牌升级

- `headerDot` 颜色从 `bg-pg-neutral-7`（灰色）改为 `bg-pg-accent-fg`（蓝色）
- 标题字重从 `font-bold` 改为 `font-extrabold`，`tracking-tight` 保持

### 1.3 移除装饰性背景

- 删除 Workbench 的 `radial-gradient` 装饰背景（`index.css:257-262` 的 `body.theme-workbench`）
- 改为纯 `var(--pg-canvas-default)`

---

## 2. Workbench 布局重构

### 2.1 从双栏改为单栏 + 内联展开

**当前：** 左侧固定宽度列表 (360px) + 右侧详情面板

**新布局：**
```
┌─────────────────────────────────┐
│ ▌蓝色渐变条                      │
├─────────────────────────────────┤
│        [ 搜索剪贴板记录... ]      │  ← 居中搜索框，max-width: 460px
├─────────────────────────────────┤
│ 1  API 接口文档 v2.3...    文本  │  ← 列表项
│    VS Code · 14:32              │
│    ─────────────────────         │
│    详情预览文本...               │  ← 内联展开区域（仅选中项）
│    [粘贴] [编辑]                 │  ← 操作按钮
├─────────────────────────────────┤
│ 2  SELECT * FROM users... 文本  │
│    DBeaver · 14:28              │
├─────────────────────────────────┤
│ 3▌design-system-tokens.json 文件│  ← 收藏项左侧色条
│    Figma · 14:15  ★             │
├─────────────────────────────────┤
│ Enter 粘贴  Ctrl+Enter 编辑  Esc │  ← 底部快捷键提示
└─────────────────────────────────┘
```

### 2.2 Header 简化

**移除：**
- `data-tauri-drag-region` 的标题区域（包含 uppercase 小标签 + "搜索窗口"标题）
- 独立的搜索输入框

**新增：**
- 顶部：蓝色渐变条
- 搜索区：居中搜索框，padding 增大，border-radius 改为 `rounded-lg`，添加 focus shadow ring
- 无独立 header 区域

### 2.3 列表项结构

**基础列表项：**
- 内容预览：`text-[13px] font-medium`，1-2 行截断
- 元信息行：kbd 快捷键 | 类型 badge | 来源 | 时间
- 收藏项：左侧 3px 蓝色色条 + 放大星号

**选中项（内联展开）：**
- 背景：`bg-pg-accent-subtle` + border
- 内容预览字重提升
- 元信息行颜色变为蓝色调
- 展开"详情预览"区域：显示完整文本或图片缩略图
- 展开区域底部：操作按钮（粘贴 / 编辑）

**非文本类型：**
- 展开区域显示类型提示文案，不显示"编辑"按钮

### 2.4 底部快捷键提示栏

固定在窗口底部：
```
Enter 粘贴  ·  Ctrl+Enter 编辑  ·  Esc 关闭
```

- 使用 `kbd` 样式的键盘标记
- 颜色：`text-pg-fg-subtle`
- padding：`py-2 px-4`
- 布局：`flex justify-center`

### 2.5 移除右侧详情面板

删除以下内容：
- `<aside>` 侧边栏容器
- `detailPanel` 区域
- `detailCard` 中的"来源"和"更新时间"小卡片
- `textPreview` 和 `nonTextNotice` 的独立面板

详情信息内联到选中列表项中。

### 2.6 内联展开的实现细节

**`detailQuery` 渲染迁移：** 当前 `useItemDetailQuery(selectedItemId)` 获取详情数据。重构后，详情数据仍通过此 query 获取，但渲染到选中列表项内部：

```tsx
{/* 选中项的详情预览 */}
{detailQuery.data && isSelected && (
  <div className="mt-2 border-t border-pg-accent-fg/12 pt-2">
    {detailQuery.data.type === "text" ? (
      <pre className="text-xs leading-relaxed text-pg-fg-muted line-clamp-5">
        {detailQuery.data.fullText || detailQuery.data.contentPreview}
      </pre>
    ) : (
      <p className="text-xs text-pg-fg-subtle">
        当前条目不是文本类型，搜索窗口只负责搜索与定位
      </p>
    )}
    <div className="mt-2 flex gap-2">
      <button className="...primary">粘贴</button>
      {detailQuery.data.type === "text" && <button>编辑</button>}
    </div>
  </div>
)}
```

**滚动行为：** 内联展开后列表项高度变化。`scrollIntoView({ block: "nearest" })` 仍然适用，无需额外调整。

**搜索框焦点管理：** `inputSuspended` 逻辑保持不变。搜索框在 header 移除后位于独立搜索区，`searchInputRef` 的 focus/blur 行为不受影响。

**不添加展开动画：** 保持即时显示/隐藏，不使用 `max-height` 或 `transition` 动画，以保持工具的即时感。

### 2.7 Workbench 收藏渲染（新增逻辑）

当前 Workbench 列表项**未渲染** `isFavorited` 状态。需新增：

```tsx
{/* 在列表项的元信息行中 */}
{item.isFavorited && (
  <span className="text-[12px] text-pg-favorite">★</span>
)}

{/* 收藏项的左侧色条样式 */}
const listItemStyle = (selected: boolean, favorited: boolean) =>
  `...${favorited && !selected ? "border-l-[3px] border-l-pg-accent-fg" : ""}${selected ? "bg-pg-accent-subtle border-pg-accent-fg/30" : "..."}`;
```

### 2.8 涉及文件

- `src/features/workbench/WorkbenchShell.tsx` — 全面重构布局 + 收藏渲染
- `src/features/workbench/state.ts` — 保持不变（导航逻辑基于列表索引）
- `src/features/workbench/keyboard.ts` — 保持不变
- `src/features/workbench/store.ts` — 保持不变

---

## 3. 收藏功能视觉强化

### 3.1 收藏条目样式

**Picker 和 Workbench 统一处理：**

**未选中 + 收藏：**
- 左侧 3px 蓝色色条（`border-left: 3px solid var(--pg-accent-fg)`）
- 内容预览字重提升到 `font-medium`
- 星号从 `text-[10px]` 放大到 `text-[12px]`，颜色 `text-pg-favorite`
- 星号始终可见（移除 `opacity-80`）

**选中 + 收藏：**
- 蓝色色条合并进选中态（选中态已有 `bg-pg-accent-subtle` 背景，色条视觉上融入）
- 星号保持 `text-[12px]`

**选中 + 非收藏：**
- 保持当前选中态
- 无左侧色条

### 3.2 Kbd Badge 选中态

选中项的 kbd badge 从中性色反转为品牌色：
- 背景：`bg-pg-accent-fg`（蓝底）
- 文字：`text-pg-fg-on-emphasis`（白字）

### 3.3 收藏切换交互（新增）

当前收藏功能只能"显示"（`isFavorited` 字段），没有用户切换入口。两个窗口使用不同的交互方式：

**Picker（纯键盘工具）：**
- `Ctrl+F` 切换当前选中项的收藏状态（Picker 窗口无搜索功能，`Ctrl+F` 不会与其他内置操作冲突）
- 删除右上角的"编辑"按钮（所有操作通过快捷键完成）
- `Ctrl+Enter` 仍保留用于打开编辑器
- 切换后通过现有 `lastMessage` 机制显示提示

**Search（搜索工具）：**
- 选中项内联展开区域的操作按钮栏新增"收藏"按钮（与"粘贴""编辑"并列）
- 按钮文案根据状态切换："收藏" / "取消收藏"
- 搜索框始终保持聚焦（点击"收藏"等按钮后搜索框不失去焦点）
- 实现方式：操作按钮使用 `onMouseDown` + `preventDefault`（而非 `onClick`），避免触发 blur
- 同时保留 `onClick` 以支持键盘可访问性（Tab + Enter）

**星号保持纯展示：** 星号不可点击，仅为视觉指示。未收藏条目不显示星号，收藏条目显示实心 `★`。

**后端支持（复用现有基础设施）：**
- 后端已有 `set_item_favorited(id: String, value: bool)` 命令（`clips.rs`）
- Bridge 已有 `setItemFavorited(id, value)` 函数（`commands.ts`）
- 前端调用 `setItemFavorited(id, !item.isFavorited)` 即可切换
- 切换后会自动发射 `CLIPS_CHANGED_EVENT`，Picker 已监听此事件并自动刷新列表
- **不需要新增任何 Rust 命令或数据库操作**

### 3.4 涉及文件

- `src/features/picker/PickerShell.tsx` — STYLES.kbdBadge 和 itemButton 样式调整
- `src/features/workbench/WorkbenchShell.tsx` — 新布局中的列表项样式

---

## 4. 设置页面优化

### 4.1 自动保存

**移除：** 底部"保存设置"按钮及其容器

**新增 debounce 自动保存：**
- 监听所有表单 state 变化
- 使用 `useEffect` + `setTimeout`/`clearTimeout` 模式实现 debounce（800ms 延迟），不引入外部依赖
- 保存成功/失败通过内联 toast 反馈（非 alert）
- 保存时 header 区域显示微妙的状态指示（如"已保存"文字闪现）

**实现方案：**
```typescript
// 在 ManagerShell 中 — 无外部依赖的 debounce hook
const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");

useEffect(() => {
  // 初始化阶段不触发保存（data 从 API 加载后同步到 state）
  if (!data) return;

  const timer = setTimeout(() => {
    setSaveStatus("saving");
    updateSettingsMutation.mutate(settings, {
      onSuccess: () => setSaveStatus("saved"),
      onError: () => setSaveStatus("error"),
    });
  }, 800);

  return () => clearTimeout(timer);
}, [settings]);

// 组件卸载时 flush
useEffect(() => {
  return () => {
    // pending timer 会在 cleanup 中被 clearTimeout 清除
    // 如果需要 flush 最后一次修改，可以在卸载前同步保存
  };
}, []);
```

**竞态条件处理：**
- `updateSettingsMutation` 的 `isPending` 为 true 时，新 debounce 触发会覆盖上一次（React Query 的 mutation 会自动处理并发）
- 组件卸载时 `clearTimeout` 确保不会泄漏
- 初始化同步（`useEffect` 将 `data` 同步到 state）阶段不触发 debounce，通过 `isInitializingRef` flag 控制

**保存错误处理：** 保留当前的错误提示条（`saveError`），但改为 debounce 触发失败后显示。

### 4.2 Section 间距节奏

当前所有 section 统一 `mb-8`。改为交替节奏：
- 快捷键：`mb-10`（高频设置，更多呼吸空间）
- 通用：`mb-6`
- 外观：`mb-10`
- 行为：`mb-6`
- 排除应用：`mb-10`

### 4.3 Section 标题层次

- 字号：`text-sm` → `text-base`
- 字重：保持 `font-semibold`
- 间距：底部 border 下方间距增加（`mt-4` → `mt-5`）

### 4.4 涉及文件

- `src/features/manager/ManagerShell.tsx` — 自动保存逻辑 + 布局调整
- `src/features/manager/queries.ts` — 可能需要调整 mutation 行为

---

## 5. 空状态和加载状态

### 5.1 加载状态

所有窗口的加载状态从纯文字改为 `<LoadingSpinner />` 组件（已存在但从未使用）。

**Picker：** 首次加载时显示 `<LoadingSpinner size="sm" text="正在加载记录..." />`
**Workbench：** `<LoadingSpinner size="sm" text="加载中..." />`
**Editor：** `<LoadingSpinner size="sm" text="正在加载条目内容..." />`
**Manager：** `<LoadingSpinner size="sm" text="正在加载设置..." />`

### 5.2 空状态文案

**Picker 空状态（新增逻辑分支）：**
```
暂无剪贴板记录
复制内容后按 Alt+Q 打开此面板
```
- 注意：当前 Picker 代码**没有空状态处理**，`items` 为空时列表区域直接渲染空白。需在 `items.length === 0` 且非加载中时新增空状态渲染
- 主文案：`text-pg-fg-muted`（提升可读性）
- 提示文案：`text-pg-fg-subtle`，`text-xs`
- 布局：垂直居中，`flex items-center justify-center h-full`

```tsx
{!recent.isLoading && items.length === 0 && (
  <div className="flex flex-col items-center justify-center flex-1 gap-1 py-8">
    <p className="text-sm text-pg-fg-muted">暂无剪贴板记录</p>
    <p className="text-xs text-pg-fg-subtle">复制内容后按 Alt+Q 打开此面板</p>
  </div>
)}
```

**Workbench 搜索无结果：**
```
未找到匹配记录
尝试调整搜索关键词
```

**Workbench 无记录：**
```
暂无剪贴板记录
复制内容后使用 Alt+S 打开此窗口
```

**Editor 空状态（等待会话）：**
```
等待编辑会话启动
在速贴面板或搜索窗口中选中文本条目后按 Ctrl+Enter 进入编辑
```

### 5.3 涉及文件

- `src/features/picker/PickerShell.tsx` — 空状态和加载状态
- `src/features/workbench/WorkbenchShell.tsx` — 空状态和加载状态
- `src/features/editor/EditorShell.tsx` — 空状态和加载状态
- `src/features/manager/ManagerShell.tsx` — 加载状态

---

## 6. 其他细节清理

### 6.1 移除 Workbench 详情卡片中的 uppercase tracking

"来源"和"更新时间"标签不需要仪表板感排版。
（此条目随 Workbench 布局重构一起移除，因为整个详情面板被删除。）

### 6.2 Header 冗余标签清理

- Workbench 的 `"搜索与定位"` uppercase 小标签 + `"搜索窗口"` 标题 → 随新布局一起移除
- Editor 的 `"等待编辑会话"` uppercase 小标签 + `"独立编辑窗口"` 标题 → 简化为单行标题

**Editor header 简化后的布局：**
```tsx
<header className="flex shrink-0 items-center justify-between px-5 py-3">
  <h1 className="text-lg font-semibold text-pg-fg-default">
    编辑器
  </h1>
  <button className="..." onClick={requestClose}>关闭</button>
</header>
```

保留元素：关闭按钮、标题（简化为"编辑器"）
移除元素：来源标签（`getSourceLabel` 返回的 uppercase 小标签）、副标题（"独立编辑窗口"）

---

## 修改文件清单

| 文件 | 改动类型 | 说明 |
|------|----------|------|
| **重命名（~30 文件）** | | |
| `src/features/workbench/*` → `src/features/search/*` | 重命名 | 目录 + 所有文件 |
| `src/features/manager/*` → `src/features/settings/*` | 重命名 | 目录 + 所有文件 |
| `src-tauri/src/domain/workbench_session.rs` → `search_session.rs` | 重命名 | |
| `src-tauri/tauri.conf.json` | 修改 | window title 更新 |
| `src/app/App.tsx` | 修改 | 导入路径 + CSS 类名 |
| `src/index.css` | 修改 | CSS 类名（workbench→search, manager→settings） |
| `src/bridge/events.ts` | 修改 | 事件常量重命名 |
| `src/bridge/commands.ts` | 修改 | bridge 函数重命名 |
| `src/bridge/window.ts` | 修改 | 导入路径 |
| `src/bridge/mockBackend.ts` | 修改 | mock 实现 |
| `src/shared/types/settings.ts` | 修改 | settings 字段重命名 |
| `src/shared/queries/clipQueries.ts` | 修改 | 查询 key 更新 |
| `src/features/picker/PickerShell.tsx` | 修改 | 跨模块引用更新 |
| `src/features/editor/*` | 修改 | 跨模块引用更新 |
| `src-tauri/src/services/window_coordinator.rs` | 修改 | 大量 workbench→search |
| `src-tauri/src/services/shortcut_manager.rs` | 修改 | 快捷键引用 |
| `src-tauri/src/domain/settings.rs` | 修改 | settings 字段 |
| `src-tauri/src/commands/windows.rs` | 修改 | 命令函数名 |
| **视觉重构** | | |
| `src/index.css` | 修改 | 添加渐变条样式；移除 Workbench radial-gradient |
| `src/features/picker/PickerShell.tsx` | 修改 | 渐变条 + headerDot 蓝色 + 收藏色条 + kbd 反转 + 收藏切换 + 空状态 + LoadingSpinner |
| `src/features/search/SearchShell.tsx` | 重构 | 全面重构为单栏布局 + 内联展开 + 搜索框居中 + 快捷键栏 + 收藏切换 |
| `src/features/settings/SettingsShell.tsx` | 修改 | 自动保存 + 间距节奏 + 标题层次 + LoadingSpinner |
| `src/features/editor/EditorShell.tsx` | 修改 | 渐变条 + header 简化 + LoadingSpinner + 空状态引导文案 |
| **新增收藏交互** | | |
| `src/features/picker/PickerShell.tsx` | 修改 | Ctrl+F 快捷键 + 删除编辑按钮 |
| `src/features/search/SearchShell.tsx` | 修改 | 收藏按钮 + onMouseDown 焦点管理 |
| `src/features/picker/PickerShell.tsx` | 修改 | 调用 setItemFavorited + 监听 CLIPS_CHANGED_EVENT（已有基础设施，无需新增后端命令） |

---

## 验收标准

1. 所有 4 个窗口顶部有蓝色渐变条（浅色和深色主题均正确显示）
2. Search 窗口为单栏布局，选中项内联展开详情
3. 收藏条目在 Picker 和 Search 中有左侧蓝色色条 + 放大星号
4. Picker 通过 `Ctrl+F` 切换收藏，已删除右上角编辑按钮
5. Search 选中项内联展开区域有"收藏"按钮，搜索框保持聚焦
6. 设置页面自动保存，无手动保存按钮
7. 所有窗口的加载状态使用 LoadingSpinner 组件
8. 所有窗口的空状态有引导文案
9. 代码中不再有 `workbench`、`Workbench`、`WORKBENCH_` 命名（CSS 类、事件名、函数名等）
10. 代码中不再有 `ManagerShell`、`Manager` 在 UI 层的引用（Rust domain 可保留）
11. Window title 统一为 `FloatPaste · 速贴/搜索/设置/编辑`
12. `pnpm build` 无 TypeScript 错误
13. `cargo build` 无编译错误
14. 键盘导航功能不受影响（Picker、Search、Editor 的快捷键全部正常）
