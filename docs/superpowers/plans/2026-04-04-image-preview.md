# Picker 图片预览 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Picker 中的图片剪贴项在列表里显示稳定缩略图，并在 Tauri 原生 tooltip 窗口里显示无跳变的大图预览，同时保持文本项与文件项现有行为不回归。

**Architecture:** 先补齐 `ClipItemSummary` 的图片元数据，让列表层不再依赖详情查询；再通过 `src/bridge/` 新增的图片 URL 解析入口把 Tauri 相对路径安全转换成可渲染 URL，浏览器 mock 则统一返回内联占位图。Tooltip 侧沿用现有原生窗口，但把“待显示位置”升级为带 `requestId` 的上下文，并在 `public/tooltip.html` 中改成图片加载完成后再上报尺寸，彻底拦住迟到回调覆盖新请求的竞态。

**Tech Stack:** React 19、TypeScript、node:test、Tauri 2、Rust、SQLite

**Spec:** `docs/superpowers/specs/2026-04-02-image-preview-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `src/shared/types/clips.ts` | 给前端 `ClipItemSummary` 补齐图片元数据字段 |
| Modify | `src-tauri/src/domain/clip_item.rs` | 给 Rust `ClipItemSummary` 补齐同名字段并保持 serde 契约一致 |
| Modify | `src-tauri/src/repository/sqlite_repository.rs` | summary 查询补齐图片列、映射函数补齐字段、添加回归测试 |
| Modify | `src/bridge/mockBackend.ts` | mock summary 透出图片字段，并提供浏览器缩略图占位图 |
| Create | `src/bridge/imageUrl.ts` | 统一封装 `getImageUrl()`，负责运行时分支、Tauri 路径解析和 mock data URL |
| Modify | `src/bridge/commands.ts` | 暴露 `resolve_image_path` / requestId 版本 tooltip 命令桥接 |
| Modify | `src/features/picker/PickerShell.tsx` | 接入缩略图缓存、异步 tooltip URL 获取、图片项预览 UI |
| Create | `src/features/picker/tooltipHtml.ts` | 收拢 tooltip HTML 构造与文本/属性转义逻辑，避免继续把字符串拼接散落在组件里 |
| Modify | `src/features/picker/tooltipState.ts` | 保持 tooltip requestId 与定位状态的纯函数逻辑可测试 |
| Modify | `public/tooltip.html` | 实现图片加载后的延迟测量、超时回退、监听器清理和 requestId 校验 |
| Modify | `src-tauri/src/services/image_storage.rs` | 暴露可复用的安全路径解析入口，并补路径遍历防护测试 |
| Modify | `src-tauri/src/commands/clips.rs` | 新增 `resolve_image_path` Tauri 命令 |
| Modify | `src-tauri/src/commands/windows.rs` | `show_tooltip` / `tooltip_ready` 改为 requestId 协议 |
| Modify | `src-tauri/src/services/tooltip_window.rs` | 从单一 pending position 升级为 pending request context，并忽略过期 ready 回调 |
| Modify | `src-tauri/src/lib.rs` | 注册 `resolve_image_path` 和新的 tooltip 命令签名 |
| Create | `tests/mockBackendSummary.test.ts` | 锁定 mock summary 图片字段与浏览器占位图链路 |
| Create | `tests/imageUrl.test.ts` | 锁定 bridge 层 URL 解析与异常回退行为 |
| Modify | `tests/pickerTooltip.test.ts` | 锁定 tooltip HTML 转义、图片 fallback、requestId 失效保护 |

---

## Chunk 1: Summary 合约与安全路径解析

### Task 1: 先补前后端 summary 回归测试

**Files:**
- Create: `tests/mockBackendSummary.test.ts`
- Modify: `src-tauri/src/repository/sqlite_repository.rs`

- [ ] **Step 1: 先写前端 mock summary 的失败测试**

在 `tests/mockBackendSummary.test.ts` 增加两个断言：
- `demo-3` 的 summary 必须带出 `imagePath`、`imageWidth`、`imageHeight`、`imageFormat`、`fileSize`
- 非图片项的这些字段必须保持 `null`

- [ ] **Step 2: 先补 Rust summary 查询的失败测试**

在 `src-tauri/src/repository/sqlite_repository.rs` 现有测试模块里新增回归用例，约束 `list_recent` / 搜索结果里的图片 summary 必须包含图片元数据，而不是只能在 detail 查询里看到。

- [ ] **Step 3: 运行失败测试确认当前链路未打通**

Run: `rtk node --test tests/mockBackendSummary.test.ts`  
Expected: 前端 mock summary 断言失败，提示图片字段缺失

Run: `rtk .\scripts\win-cargo test summary_includes_image_metadata`  
Expected: Rust 测试失败，提示 summary 映射未包含图片列

- [ ] **Step 4: 最小实现 summary 合约**

实现内容：
- `src/shared/types/clips.ts` 给 `ClipItemSummary` 增加 5 个图片字段
- `src-tauri/src/domain/clip_item.rs` 给 Rust `ClipItemSummary` 增加同名字段
- `src-tauri/src/repository/sqlite_repository.rs` 的 `list_recent`、`list_favorites`、`search_recent`、`search_with_keyword` 的 `SELECT` 补齐 `image_path, image_width, image_height, image_format, file_size`
- `map_summary_row` 把这些字段写回 summary
- `src/bridge/mockBackend.ts` 的 `toSummary()` 把图片字段一并透出

- [ ] **Step 5: 重新运行测试确认合约已打通**

Run: `rtk node --test tests/mockBackendSummary.test.ts`  
Expected: PASS

Run: `rtk .\scripts\win-cargo test summary_includes_image_metadata`  
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add tests/mockBackendSummary.test.ts src/shared/types/clips.ts src/bridge/mockBackend.ts src-tauri/src/domain/clip_item.rs src-tauri/src/repository/sqlite_repository.rs
git commit -m "feat(picker): 补齐图片 summary 元数据"
```

### Task 2: 新增安全的图片路径解析命令

**Files:**
- Modify: `src-tauri/src/services/image_storage.rs`
- Modify: `src-tauri/src/commands/clips.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 先写 `ImageStorage` 安全解析的失败测试**

在 `src-tauri/src/services/image_storage.rs` 测试模块新增断言：
- 合法相对路径能被解析到 `base_dir/images/...`
- `..`、根路径、盘符前缀路径都会被拒绝

- [ ] **Step 2: 运行失败测试**

Run: `rtk .\scripts\win-cargo test image_storage_rejects_invalid_paths`  
Expected: FAIL，提示缺少可复用的公开解析入口或校验行为不完整

- [ ] **Step 3: 实现最小路径解析链路**

实现内容：
- 把 `ImageStorage::resolve_image_path` 提升为 `pub(crate)`，或新增等价公开包装方法
- 在 `src-tauri/src/commands/clips.rs` 新增 `resolve_image_path(state, image_path)` 命令
- 命令内部复用 `state.image_storage` 做安全解析，并在返回前检查文件存在性
- 在 `src-tauri/src/lib.rs` 注册 `commands::clips::resolve_image_path`

- [ ] **Step 4: 重新运行测试**

Run: `rtk .\scripts\win-cargo test image_storage_rejects_invalid_paths`  
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/services/image_storage.rs src-tauri/src/commands/clips.rs src-tauri/src/lib.rs
git commit -m "feat(picker): 新增图片路径解析命令"
```

---

## Chunk 2: bridge URL 解析与列表缩略图

### Task 3: 建立可测试的 `getImageUrl()` bridge 封装

**Files:**
- Create: `src/bridge/imageUrl.ts`
- Create: `tests/imageUrl.test.ts`
- Modify: `src/bridge/commands.ts`
- Modify: `src/bridge/mockBackend.ts`

- [ ] **Step 1: 先写 bridge 层失败测试**

在 `tests/imageUrl.test.ts` 里通过依赖注入测试 `createImageUrlResolver()`：
- Tauri 分支会先调用 `resolve_image_path`，再把绝对路径转换成 webview URL
- `resolve_image_path` 失败时返回 `null`
- 浏览器 mock 分支对任意图片路径都返回固定 `MOCK_IMAGE_URL`
- `null` / 空路径直接返回 `null`

- [ ] **Step 2: 运行失败测试**

Run: `rtk node --test tests/imageUrl.test.ts`  
Expected: FAIL，提示 `getImageUrl` / `createImageUrlResolver` 不存在

- [ ] **Step 3: 实现最小 bridge**

实现内容：
- 在 `src/bridge/imageUrl.ts` 新增 `MOCK_IMAGE_URL`
- 导出 `createImageUrlResolver()`，依赖注入 `isTauriRuntime`、`invoke`、`convertFileSrc`
- 默认导出 `getImageUrl()` 供业务直接调用
- `src/bridge/commands.ts` 暴露 `resolveImagePath()` 或把 `invoke("resolve_image_path")` 封进 `imageUrl.ts`
- `src/bridge/mockBackend.ts` 不再试图读取真实本地文件，浏览器模式统一走内联 SVG data URL

- [ ] **Step 4: 重新运行测试**

Run: `rtk node --test tests/imageUrl.test.ts`  
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/bridge/imageUrl.ts src/bridge/commands.ts src/bridge/mockBackend.ts tests/imageUrl.test.ts
git commit -m "feat(picker): 补齐图片 URL bridge 解析"
```

### Task 4: Picker 列表接入缩略图与 URL 缓存

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`

- [ ] **Step 1: 在 `PickerShell` 中加入缩略图 URL 缓存**

实现要求：
- 用组件级 `Map<string, string | null>` 缓存 itemId -> imageUrl
- 仅对 `item.type === "image"` 且存在 `imagePath` 的项按需请求
- 同一个 item 重渲染或重复 hover 时不重复解析路径

- [ ] **Step 2: 列表图片项渲染 32 × 32 缩略图**

实现要求：
- 图片项在 `contentPreview` 左侧显示 `<img>`
- 样式为 32 × 32、`object-fit: cover`
- 加载失败时回退为原有纯文本布局，不阻塞列表渲染
- 非图片项 UI 不变

- [ ] **Step 3: 运行前端回归测试与构建**

Run: `rtk node --test tests/mockBackendSummary.test.ts tests/imageUrl.test.ts tests/pickerTooltip.test.ts`  
Expected: PASS

Run: `rtk .\scripts\win-pnpm build`  
Expected: 前端构建通过，无新的 TypeScript 错误

- [ ] **Step 4: 提交**

```bash
git add src/features/picker/PickerShell.tsx
git commit -m "feat(picker): 列表接入图片缩略图"
```

---

## Chunk 3: Tooltip 异步测量与 requestId 防竞态

### Task 5: 把 tooltip HTML 与前端 requestId 流程拆清楚

**Files:**
- Create: `src/features/picker/tooltipHtml.ts`
- Modify: `src/features/picker/PickerShell.tsx`
- Modify: `src/features/picker/tooltipState.ts`
- Modify: `tests/pickerTooltip.test.ts`
- Modify: `src/bridge/commands.ts`

- [ ] **Step 1: 先写 tooltip HTML 与 requestId 的失败测试**

在 `tests/pickerTooltip.test.ts` 增加断言：
- `buildTooltipHtml()` 对文本内容继续做 HTML 转义
- 图片 tooltip 的 `src`、`data-request-id` 等属性走属性级转义
- 图片 URL 缺失或解析失败时回退为纯文本 tooltip
- requestId 不匹配时，异步返回的 tooltip 结果会被丢弃

- [ ] **Step 2: 运行失败测试**

Run: `rtk node --test tests/pickerTooltip.test.ts`  
Expected: FAIL，提示缺少图片 tooltip builder 或属性转义逻辑

- [ ] **Step 3: 最小实现前端 tooltip 流程**

实现内容：
- 从 `PickerShell.tsx` 提炼 `escapeHtml`、`escapeHtmlAttribute`、`buildTooltipHtml` 到 `tooltipHtml.ts`
- 文本 tooltip 保持原有即时展示逻辑，但 `showTooltip` 也携带 `requestId`
- 图片 tooltip 在 100ms hover 延迟后先调用 `getImageUrl()`，返回后再次比对 `tooltipRequestIdRef.current`
- 只有 requestId 仍匹配时才调用 `showTooltip(requestId, x, y, html, theme)`
- `handleItemMouseLeave` 继续失效当前 request，并隐藏 tooltip

- [ ] **Step 4: 扩展 bridge 命令签名**

实现要求：
- `src/bridge/commands.ts` 的 `showTooltip()` 改为接收 `requestId`
- 后续 `tooltip_ready` 也按同一 requestId 协议对齐

- [ ] **Step 5: 重新运行测试**

Run: `rtk node --test tests/pickerTooltip.test.ts`  
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add src/features/picker/tooltipHtml.ts src/features/picker/tooltipState.ts src/features/picker/PickerShell.tsx src/bridge/commands.ts tests/pickerTooltip.test.ts
git commit -m "feat(picker): 重构图片 tooltip 前端链路"
```

### Task 6: 原生 tooltip 窗口改成两阶段测量

**Files:**
- Modify: `public/tooltip.html`
- Modify: `src-tauri/src/commands/windows.rs`
- Modify: `src-tauri/src/services/tooltip_window.rs`

- [ ] **Step 1: 先补 Rust 侧 pending request 的失败测试**

在 `src-tauri/src/services/tooltip_window.rs` 的测试模块增加断言：
- `hide_tooltip` 会清空 pending request context
- 过期 requestId 的 `tooltip_ready` 不会消费新请求的位置

- [ ] **Step 2: 运行失败测试**

Run: `rtk .\scripts\win-cargo test tooltip_window`  
Expected: FAIL，提示当前 pending state 只有坐标、缺少 requestId 防护

- [ ] **Step 3: 实现 Rust 侧 requestId 协议**

实现内容：
- `TooltipWindow` 的 pending 状态从 `Option<(x, y)>` 升级为包含 `request_id, x, y` 的结构
- `show_tooltip()` 存入 request context，并把 `requestId` 传给 `window.showTooltip(...)`
- `on_tooltip_ready()` 仅在 requestId 匹配时才设置窗口尺寸/位置并显示
- 迟到的 ready 回调在 requestId 不匹配时静默丢弃，不打印噪音日志

- [ ] **Step 4: 实现 `public/tooltip.html` 的两阶段测量**

实现要求：
- `window.showTooltip(requestId, html, theme)` 设置新内容前，先清理上一请求的 `<img>` 监听器与超时器
- 图片 tooltip 监听 `<img>` 的 `load` / `error`，并设置 2 秒超时
- 图片成功加载后再上报 `tooltip_ready({ requestId, width, height })`
- 图片失败或超时后回退为纯文本 tooltip，再上报 `tooltip_ready`
- `window.hideTooltip()` 清理 DOM、监听器与超时器
- 调用 `invoke("tooltip_ready")` 时统一 `.catch(() => {})`，避免正常竞态下刷控制台错误

- [ ] **Step 5: 重新运行 Rust 测试**

Run: `rtk .\scripts\win-cargo test tooltip_window`  
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add public/tooltip.html src-tauri/src/commands/windows.rs src-tauri/src/services/tooltip_window.rs
git commit -m "feat(picker): 修复图片 tooltip 异步测量竞态"
```

---

## Chunk 4: 全量验证与交付

### Task 7: 完整验证图片预览链路

**Files:**
- Modify: `docs/superpowers/plans/2026-04-04-image-preview.md`

- [ ] **Step 1: 运行前端 node 测试**

Run: `rtk node --test tests/mockBackendSummary.test.ts tests/imageUrl.test.ts tests/pickerTooltip.test.ts tests/workbenchKeyboard.test.ts tests/workbenchState.test.ts tests/editorKeyboard.test.ts`  
Expected: PASS

- [ ] **Step 2: 运行 Rust 测试**

Run: `rtk .\scripts\win-cargo test`  
Expected: PASS

- [ ] **Step 3: 运行前端构建**

Run: `rtk .\scripts\win-pnpm build`  
Expected: PASS

- [ ] **Step 4: 运行桌面手动验证**

Run: `rtk .\scripts\win-pnpm tauri dev`  
手动检查：
- 图片项列表中能稳定显示 32 × 32 缩略图
- 浏览器 mock 模式下显示的是占位图，不依赖真实本地图片文件
- Tauri 模式下 hover 图片项时，tooltip 首次出现就是正确尺寸，没有“小到大”跳变
- 图片路径无效时，列表与 tooltip 都回退为纯文本，不刷错误日志
- 快速划过多张图片时，不会出现旧图片预览覆盖新 hover 的情况
- 文本项、文件项现有 tooltip 行为不回归

- [ ] **Step 5: 收尾提交**

```bash
git add .
git commit -m "feat(picker): 支持图片缩略图与悬浮大图预览"
```
