# Search Filter Dropdown Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为搜索窗口增加“全部 / 收藏 / 文本 / 图片 / 文件”下拉筛选，并打通前后端过滤链路。

**Architecture:** 搜索窗口本地维护一个单值筛选状态，将其映射为现有查询结构中的 `filters`。前端查询层和 Rust 仓储层共享同一组过滤语义，避免最近列表与关键词搜索出现行为不一致。

**Tech Stack:** React 19、TypeScript、TanStack Query、Tauri、Rust、SQLite

---

## Chunk 1: 后端过滤能力

### Task 1: 先写失败中的 Rust 回归测试

**Files:**
- Modify: `src-tauri/src/repository/sqlite_repository.rs`

- [ ] **Step 1: 写一个按类型筛选最近列表的失败测试**

- [ ] **Step 2: 运行 `./scripts/win-cargo test search_filters_by_clip_type_for_recent_and_keyword_results`，确认失败**

- [ ] **Step 3: 写最小实现，让测试通过**

- [ ] **Step 4: 重新运行同一条测试，确认通过**

### Task 2: 扩展查询过滤模型

**Files:**
- Modify: `src-tauri/src/domain/clip_item.rs`
- Modify: `src-tauri/src/repository/sqlite_repository.rs`

- [ ] **Step 1: 在 `SearchFilters` 增加可选类型字段**

- [ ] **Step 2: 在 SQLite 过滤子句中拼接 `type = ?` 条件**

- [ ] **Step 3: 保持 `favoritedOnly` 语义不变**

- [ ] **Step 4: 运行相关测试，确认最近列表与关键词搜索都通过**

## Chunk 2: 前端筛选接线

### Task 3: 更新前端查询类型和 mock 行为

**Files:**
- Modify: `src/shared/types/clips.ts`
- Modify: `src/bridge/mockBackend.ts`
- Modify: `src/features/search/queries.ts`

- [ ] **Step 1: 定义前端筛选枚举和查询过滤字段**

- [ ] **Step 2: 让 mock 搜索和最近列表支持同样的筛选语义**

- [ ] **Step 3: 让搜索查询和最近查询都接收筛选值**

- [ ] **Step 4: 自检查询 key 是否包含筛选值，避免缓存串用**

### Task 4: 增加搜索头部下拉框

**Files:**
- Modify: `src/features/search/SearchShell.tsx`

- [ ] **Step 1: 新增筛选状态与选项映射**

- [ ] **Step 2: 在搜索头部加入 `select`**

- [ ] **Step 3: 将筛选状态传给最近查询和关键词查询**

- [ ] **Step 4: 保持现有选中项、空状态和操作按钮行为不回退**

## Chunk 3: 验证

### Task 5: 完整验证

**Files:**
- Modify: `docs/superpowers/specs/2026-04-04-search-filter-dropdown-design.md`
- Modify: `docs/superpowers/plans/2026-04-04-search-filter-dropdown.md`

- [ ] **Step 1: 运行 `./scripts/win-cargo test`**

- [ ] **Step 2: 运行 `./scripts/win-pnpm build`**

- [ ] **Step 3: 记录验证结果与任何剩余风险**
