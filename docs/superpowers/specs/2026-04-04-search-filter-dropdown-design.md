# 搜索窗口筛选下拉设计

**日期：** 2026-04-04
**范围：** `Search` 窗口快速筛选

## 目标

在搜索窗口的搜索框右侧加入一个下拉框，允许用户快速切换为 `全部`、`收藏`、`文本`、`图片`、`文件` 五种筛选视图，并同时作用于“最近条目”和“关键词搜索”。

## 现状

- 搜索窗口当前只支持关键词输入与最近列表展示，没有显式筛选入口。
- 前后端已经支持 `favoritedOnly` 收藏过滤，但前端没有暴露入口。
- 剪贴类型已经统一为 `text`、`image`、`file`，适合直接加入查询过滤条件。

## 设计决策

### 1. 交互形式

- 在搜索框右侧增加一个原生 `select`。
- 默认值为 `全部`。
- 下拉框选项固定为：
  - `全部`
  - `收藏`
  - `文本`
  - `图片`
  - `文件`

### 2. 筛选语义

- `全部`：不附加筛选条件。
- `收藏`：设置 `filters.favoritedOnly = true`。
- `文本` / `图片` / `文件`：设置 `filters.clipType` 为对应类型。
- 当前版本不支持多选，不组合“收藏 + 类型”。

### 3. 数据流

- 搜索窗口本地维护当前筛选值。
- 无关键词时，最近列表查询也接受当前筛选值。
- 有关键词时，搜索查询把筛选值并入 `SearchQuery.filters`。
- Rust 仓储层统一在 `build_filters_clause_with_alias` 中追加类型条件，避免最近列表与全文搜索逻辑分叉。

### 4. 视觉与可用性

- 下拉框放在搜索头部，与现有搜索输入同行。
- 样式保持与当前窗口浅边框、低干扰基调一致。
- 切换筛选后保留当前关键词，并自动刷新列表。
- 若筛选结果为空，沿用现有空状态文案，不额外增加复杂说明。

## 影响文件

- `src/features/search/SearchShell.tsx`
- `src/features/search/queries.ts`
- `src/shared/types/clips.ts`
- `src/bridge/mockBackend.ts`
- `src-tauri/src/domain/clip_item.rs`
- `src-tauri/src/repository/sqlite_repository.rs`

## 验证策略

- Rust：新增按类型筛选的仓储测试，覆盖最近列表搜索与关键词搜索路径。
- 前端：执行 `./scripts/win-pnpm build`，确保类型、查询参数和界面编译通过。
- 若 Rust 改动落地，再执行 `./scripts/win-cargo test` 验证回归。
