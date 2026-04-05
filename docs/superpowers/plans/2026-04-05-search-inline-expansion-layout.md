# Search 内联展开与自适应窗口布局 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Search 从“结果列表 + 底部摘要区”改成“结果列表内联展开”，并把窗口收敛为更窄、禁高向手动调整、可随结果数量自动变化的搜索处理窗。

**Architecture:** 以 `src/features/search/SearchShell.tsx` 为主战场，重排列表项结构，把详情与主操作迁入当前选中项；同时补充 `src/bridge/window.ts` 的窗口尺寸控制封装，让前端可以根据结果数量回写 Search 窗口尺寸。Rust 侧在 `src-tauri/src/services/window_coordinator.rs` 收紧 Search 的默认宽度、最小尺寸与可调整策略，保证首次打开和异常恢复时都落在新的窗口模型内。

**Tech Stack:** React, TypeScript, TanStack Query, Tailwind CSS, Tauri 2, Rust

**Spec:** `docs/superpowers/specs/2026-04-05-search-inline-expansion-layout-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `src/features/search/SearchShell.tsx` | 移除底部摘要区，改成当前项内联展开；计算内容高度并驱动窗口自适应 |
| Modify | `src/bridge/window.ts` | 增加读取/设置当前窗口尺寸与限制的轻量封装 |
| Modify | `src-tauri/src/services/window_coordinator.rs` | 调整 Search 默认宽度、最小尺寸、可调整策略与异常恢复尺寸 |

## Chunk 1: 重构 Search 列表结构

### Task 1: 移除底部固定摘要区，改为当前项内联展开

**Files:**
- Modify: `src/features/search/SearchShell.tsx`（`STYLES`、结果列表 JSX、底部 `<section>` 与 `<footer>`）

- [ ] **Step 1: 调整 SearchShell 的样式常量，建立“普通项 + 当前项展开槽”两层结构**

在 `src/features/search/SearchShell.tsx` 中重写以下样式职责：

- `STYLES.listItem` 从单层按钮行改成可容纳“头部摘要 + 展开区”的外层容器
- 为当前项新增展开区、操作区、元信息行、状态提示样式
- 收窄整体视觉体量，避免当前项展开后变成详情页

目标约束：

- 当前项更明确，但其他项保持正常高度
- 展开区只承接摘要、必要上下文和主操作
- 不恢复底部固定摘要区，也不保留底部快捷键提示

- [ ] **Step 2: 把 detailQuery 的渲染迁入当前选中项内部**

在列表 `items.map(...)` 中：

- 让每个结果项保持可点击切换
- 当前项内部渲染 `detailQuery` 的加载、文本摘要、非文本提示、空态
- `粘贴`、`编辑`、`收藏/取消收藏` 操作按钮放在当前项展开区

实现要求：

- 文本条目展示比列表预览更完整的内容，但不要无限展开全文
- 非文本条目保留“可快速粘贴 / 定位”的说明
- 当前项切换后，展开区跟随切换，且仍支持双击直接粘贴

- [ ] **Step 3: 删除旧的底部摘要区与快捷键 footer**

移除 `SearchShell.tsx` 末尾的：

- 底部 `<section>` 摘要与操作区
- 底部 `<footer>` 快捷键提示

删除后确认：

- 页面结构只剩顶部搜索区、结果统计条、结果列表
- 所有主操作都能在内联展开区完成

- [ ] **Step 4: 运行前端构建，确认列表结构调整未破坏编译**

Run: `./scripts/win-pnpm build`
Expected: 构建通过，无 TypeScript 或 Vite 错误


### Task 2: 收紧 Search 的窗口拉伸入口，只保留宽度方向

**Files:**
- Modify: `src/features/search/SearchShell.tsx`（`SEARCH_RESIZE_HANDLES`）

- [ ] **Step 1: 移除所有高度相关 resize handles**

在 `SEARCH_RESIZE_HANDLES` 中删除以下方向：

- `North`
- `South`
- `NorthWest`
- `NorthEast`
- `SouthWest`
- `SouthEast`

只保留：

- `West`
- `East`

要求：

- 视觉上仍允许用户横向微调宽度
- 不再出现上下或角落的高度调整手柄

- [ ] **Step 2: 手动检查 Search 顶部拖拽与横向拉伸是否互不冲突**

验证点：

- 顶部渐变条和标题区域仍可拖动窗口
- 左右边缘仍可触发 resize
- 误触列表区域时不会触发窗口 resize

## Chunk 2: 让窗口高度跟随列表内容自动变化

### Task 3: 补充窗口尺寸桥接，让前端可以控制 Search 高度

**Files:**
- Modify: `src/bridge/window.ts`
- Modify: `src/features/search/SearchShell.tsx`

- [ ] **Step 1: 在 `src/bridge/window.ts` 增加当前窗口尺寸控制封装**

新增面向 Search 可复用的轻量方法，例如：

- 读取当前窗口 scale factor 或当前尺寸
- 设置当前窗口尺寸
- 设置最小尺寸
- 设置最大尺寸

要求：

- 继续通过 `getCurrentWebviewWindow()` 实现
- 保持函数粒度小，不把 Search 逻辑耦合进 bridge 层
- 浏览器预览模式下安全 no-op

- [ ] **Step 2: 在 SearchShell 中建立“内容高度 -> 窗口高度”的同步逻辑**

在 `SearchShell.tsx` 中增加与窗口尺寸同步相关的 ref / effect：

- 为搜索头、统计条、错误条、列表容器建立测量点
- 按结果数量和当前项展开状态估算或测量目标内容高度
- 设置窗口高度时遵循最小高度与最大高度

推荐规则：

- 结果为空或很少时，窗口明显收短
- 结果增加时逐步增长
- 超过最大高度后固定在上限
- 滚动只发生在结果列表内部

- [ ] **Step 3: 确保自动高度不会干扰键盘导航和滚动定位**

重点处理：

- 当前项切换导致展开区变化时，窗口高度更新不能造成明显闪跳
- 达到最大高度后，只更新列表内部滚动，不继续放大窗口
- `scrollIntoView({ block: "nearest" })` 仍然可用

- [ ] **Step 4: 在浏览器预览模式下提供安全退化**

要求：

- 非 Tauri 环境不调用窗口尺寸 API
- 浏览器预览仍然使用页面内滚动查看结构效果
- 相关 effect 在非 Tauri 环境中应尽早返回

## Chunk 3: 收紧 Rust 侧默认窗口模型

### Task 4: 调整 Search 的默认宽度、最小尺寸与恢复尺寸

**Files:**
- Modify: `src-tauri/src/services/window_coordinator.rs`

- [ ] **Step 1: 收窄 Search 默认宽度和最小宽度**

调整以下常量：

- `SEARCH_WINDOW_DEFAULT_WIDTH`
- `SEARCH_WINDOW_MIN_WIDTH`

目标：

- 默认宽度明显小于当前 `900`
- 最小宽度允许适度收窄，但不能压坏搜索框和展开操作区

可接受方向：

- 默认宽度落在 `720 ~ 820`
- 最小宽度落在 `560 ~ 640`

最终数值以实际布局效果为准，但必须体现“更像处理窗而不是资料库页”。

- [ ] **Step 2: 更新 Search 的默认高度边界，为自动高度留出空间**

调整：

- `SEARCH_WINDOW_DEFAULT_HEIGHT`
- `SEARCH_WINDOW_MIN_HEIGHT`

要求：

- 默认高度不再假设固定的大型面板
- 最小高度能够容纳搜索框、统计条与少量结果
- 与前端自动高度逻辑保持一致，不发生首次打开后立即强制跳变过大的情况

- [ ] **Step 3: 关闭 Search 的原生可调整高度策略**

在 `ensure_search_window(...)` 中重新评估：

- 是否保留 `.resizable(true)` 以支持左右拉宽
- 若 Tauri 不支持单独锁轴，则通过前端 handle 限制 + 合理的 min/max size 共同实现

同时确保：

- `restore_search_window_geometry(...)` 使用新的默认尺寸
- 异常位置/占位尺寸恢复后，窗口落在新宽度模型内

## Chunk 4: 验证与回归检查

### Task 5: 执行构建验证并覆盖关键场景

**Files:**
- Verify: `src/features/search/SearchShell.tsx`
- Verify: `src/bridge/window.ts`
- Verify: `src-tauri/src/services/window_coordinator.rs`

- [ ] **Step 1: 执行前端构建验证**

Run: `./scripts/win-pnpm build`
Expected: 构建通过

- [ ] **Step 2: 如 Rust 窗口代码有改动，执行 Rust 测试**

Run: `./scripts/win-cargo test`
Expected: 测试通过，至少不引入编译错误

- [ ] **Step 3: 手动验证 Search 的关键场景**

重点检查：

1. 空结果时窗口是否更短
2. 1-2 条结果时窗口是否跟随收缩
3. 多条结果时窗口是否逐步变高
4. 达到最大高度后是否转为内部滚动
5. 当前项是否在列表内联展开并承接摘要与操作
6. 非文本项是否仍可快速判断并执行粘贴
7. 上下键、Enter、Ctrl+Enter、收藏切换是否保持正常

- [ ] **Step 4: 提交**

```bash
git add docs/superpowers/plans/2026-04-05-search-inline-expansion-layout.md src/features/search/SearchShell.tsx src/bridge/window.ts src-tauri/src/services/window_coordinator.rs
git commit -m "feat(search): 改为内联展开并收紧窗口模型"
```

## 执行说明

- 当前仓库没有独立前端测试框架，因此以 `./scripts/win-pnpm build` 和手动交互验证为主
- 自动高度逻辑优先采用“前端测量 + Tauri 当前窗口 API”方案，不额外引入新的 Rust 命令，除非实现中证明前端桥接能力不足
- 若 Tauri 原生窗口无法真正做到“仅允许横向调整”，本次以“只保留左右 resize handles + 不暴露上下/角落拉伸入口”为可接受实现，前提是用户已经不能通过产品表面直接调高窗口
