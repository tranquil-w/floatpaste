# 标签功能设计

**日期：** 2026-08-14
**范围：** 剪贴记录标签（Tags）的数据库、命令、搜索集成与三个窗口的交互

## 目标

为剪贴记录引入用户自定义标签，解决"收藏"只有一维状态、无法表达"项目 / 用途 / 主题"等多维度归属的问题：

1. 在编辑窗口为任意条目（文本 / 图片 / 文件）添加、移除标签。
2. 在搜索窗口按标签筛选条目，且关键词搜索能命中标签名。
3. 在设置窗口统一管理标签（重命名、合并、删除）。
4. 在速贴面板展示选中条目的标签（纯展示）。

## 现状

- 条目只有 `is_favorited` 一个布尔维度，搜索筛选也只有 `favoritedOnly`、`clipType`、`sourceApp` 三种条件。
- 搜索窗口的筛选下拉为固定 5 项（全部 / 收藏 / 文本 / 图片 / 文件），选项类型 `SearchQuickFilter` 定义于 `src/shared/types/clips.ts`，选中态是 `SearchShell` 内的组件局部 state。
- 编辑窗口目前只负责文本编辑，是唯一"查看条目详情"的窗口，适合承担标签编辑入口。
- 全文搜索基于 FTS5（`clip_items_fts`），CJK 走逐字空格化索引（schema v4）；仓储层 `build_filters_clause_with_alias` 统一拼接筛选条件。
- FTS 行在文本更新、条目删除时会被整行重写或移除（`update_text` / `delete_item`），新增 `tags` 列后这些写入路径必须同步维护（见 §4 写入路径清单）。
- 事件：`CLIPS_CHANGED_EVENT` 的监听方为搜索与速贴窗口；设置窗口只监听 `SETTINGS_CHANGED_EVENT`，标签列表刷新需要新增事件。
- 数据库连接已启用 `PRAGMA foreign_keys = ON`（`0001_init.sql`，单连接架构），外键级联删除可用。

## 设计决策

### 1. 数据模型：标签名即主键，两张新表

```sql
CREATE TABLE IF NOT EXISTS tags (
  name TEXT PRIMARY KEY COLLATE NOCASE,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS clip_item_tags (
  item_id TEXT NOT NULL REFERENCES clip_items(id) ON DELETE CASCADE,
  tag_name TEXT NOT NULL COLLATE NOCASE REFERENCES tags(name) ON DELETE CASCADE,
  PRIMARY KEY (item_id, tag_name)
);

CREATE INDEX IF NOT EXISTS idx_clip_item_tags_tag ON clip_item_tags(tag_name);
```

- **以标签名为主键**，不引入自增 id，省去 id↔name 双向映射。
- **两表的标签列都声明 `COLLATE NOCASE`**：`tags.name` 保证大小写不敏感唯一（`Work` 与 `work` 是同一标签，存储形态取首次创建时的写法）；`clip_item_tags.tag_name` 与父列保持一致，使关联表主键唯一性与外键比较都按 NOCASE 进行，避免同一标签的大小写变体在关联表里出现双行、聚合时输出重复芯片。
- 关联表主键 `(item_id, tag_name)` 天然去重；外键级联删除保证删条目 / 删标签时无孤儿关联。
- **不做 `deleted_at` 软删**：标签被删即彻底删除（重命名 / 合并兜底误删），避免标签表复杂化。
- 条目本身是软删除（`deleted_at`），其关联行保留无碍——所有查询路径都强制 `deleted_at IS NULL`，`list_tags` 计数同样只统计未删除条目。若未来新增硬清理逻辑，需同步清理关联表（外键级联可兜底）。

**重命名 / 合并统一走"先插后删"四步，不用 UPDATE 改主键。** 外键即时检查下，直接 `UPDATE tags SET name = ?` 会因子表仍引用旧名而失败；先改子表又会因父表尚无新名而失败，两条顺序都走不通：

```sql
-- 事务内执行；?new 不存在时为纯重命名，已存在时自然演变为合并
INSERT OR IGNORE INTO tags(name, created_at) VALUES(?new, ?now);
INSERT OR IGNORE INTO clip_item_tags(item_id, tag_name)
  SELECT item_id, ?new FROM clip_item_tags WHERE tag_name = ?old;
DELETE FROM clip_item_tags WHERE tag_name = ?old;
DELETE FROM tags WHERE name = ?old;
```

- 第一步 `INSERT OR IGNORE` 吸收"新名已存在"的合并场景：关联取并集（第二步 OR IGNORE），旧标签连同其关联被删除。
- **禁止用 `UPDATE OR REPLACE` 改 `tags.name`**：合并时 REPLACE 会先删除同名冲突行并触发级联，把新名下已有的全部关联一并删掉，"合并"退化为"丢失"。
- 重命名 / 删除前先收集受影响条目 id，用于批量重建 FTS 行（见 §4），级联或删行之后关联已查不到。

### 2. 标签归一化规则

- 录入时 `trim`、折叠连续空白（`\s+` → 空格），空串或全空白直接拒绝。
- 服务端写入前按 NOCASE 去重（前端芯片集合可能混入大小写变体），并把每个名字对齐到 `tags` 表的既有规范写法；未登录新标签由 `PRIMARY KEY COLLATE NOCASE` 兜底返回"标签已存在"。
- 长度上限 32 字符（超出报错），单条目标签数上限 20 个（防卡片渲染失控）。
- 标签名不参与 CJK 空格化存储，但索引到 FTS 时按 `space_cjk_text` 处理（与现有索引策略对称）。

### 3. 命令层 API（Rust）

| 命令 | 签名 | 语义 |
|------|------|------|
| `list_tags` | `() -> Vec<TagInfo>` | 全部标签及使用次数（`name`、`itemCount`、`createdAt`），按使用次数降序、同数按名称升序；计数只统计未删除条目 |
| `set_item_tags` | `(id, tag_names: Vec<String>) -> ClipItemDetail` | **全量替换**条目标签；幂等、便于前端"芯片集合"整体提交 |
| `rename_tag` | `(old_name, new_name) -> ()` | 重命名；新名已存在时**合并**（关联并集，删旧标签），由 §1 四步流程统一表达 |
| `delete_tag` | `(name) -> ()` | 删除标签及其全部关联（先收集受影响条目 id，`DELETE FROM tags` 依赖级联清关联） |

- 业务规则（归一化、去重、数量上限、合并逻辑、FTS 同步）收敛到 `TagService`，命令层只做参数透传与事件广播——与 `ClipService` 现有分工一致。
- **事件广播矩阵**：`set_item_tags`、`rename_tag`、`delete_tag` 三个写命令都同时广播 `CLIPS_CHANGED_EVENT` 与 `TAGS_CHANGED_EVENT`（前者刷新搜索 / 速贴的条目列表，后者刷新设置窗口的标签列表）。现有 payload 惯例是单个条目 id，但重命名 / 删除的影响是批量的，前端监听方不得按 payload id 精确失效，需整体失效重查。
- 广播时机与 `set_item_favorited` 一致：无论是否发生变化都广播，由前端缓存失效兜底。

### 4. 搜索集成

**筛选条件**：`SearchFilters` 新增 `tagNames?: string[]`，**AND 语义**——选中多个标签时条目必须同时拥有全部标签（多数用户对"多标签筛选"的直觉）。`tagNames` 为 `None` 或空数组时不追加任何子句（空数组等价于未筛选）。实现上在 `build_filters_clause_with_alias` 中对每个标签追加一条 `EXISTS` 子查询：

```sql
EXISTS (
  SELECT 1 FROM clip_item_tags t
  WHERE t.item_id = ci.id AND t.tag_name = ? COLLATE NOCASE
)
```

`list_recent`、`list_favorites`、`search_recent` 目前不带表别名（`FROM clip_items`），本次统一改为 `FROM clip_items ci`（`search_with_keyword` 已是 `ci`，对应 COUNT 查询同步），保证子查询里的 `ci.id` 在四条查询中都有效。

**关键词命中标签名**：`clip_items_fts` 新增 `tags` 列（空格化后的标签名拼接），迁移时全量重建索引。搜索框直接输入标签名即可命中，且走 `bm25` 参与相关性排序，无需单独的"标签前缀搜索"入口。索引侧 `space_cjk_text` 与查询侧 `build_fts_query` 的逐词空格化保持对称，沿用现状。`tags` 列与正文列在 `bm25` 中默认等权，如需突出标签命中可传列权重（`bm25(clip_items_fts, ...)`），本期不做；注意 `RelevanceDesc` 的 ORDER BY 已含一个 `search_text LIKE` 加权 CASE，FTS 列数变化后需核对占位符顺序。

**FTS `tags` 列的写入路径（共五处，缺一会出现幽灵命中或标签丢失）**：

| 路径 | 行为 |
|------|------|
| `save_text_item` / `save_image_item` / `save_file_item` | 新条目无标签，`tags` 列写空串（与其他列的非 NULL 惯例一致） |
| `update_text` | 重写 FTS 行时**回填该条目当前标签**——否则已打标签的条目编辑保存一次，标签即不再命中搜索 |
| `set_item_tags` | 重写该条目 FTS 行，`tags` 列取聚合后的新标签集合 |
| `rename_tag` / `delete_tag` | 对受影响条目**批量重写 FTS 行**（沿用 `DELETE FROM clip_items_fts WHERE item_id = ?` + 重插整行的模式）——否则搜旧标签名继续命中（幽灵命中），搜新名不命中 |
| schema v5 迁移 | 重建 FTS 时从 `clip_item_tags` 回填（见 §7） |

**快速筛选入口**：`SearchQuickFilter` 新增 `"tag"` 选项。选中后，搜索头部下方出现一行标签芯片（数据来自 `list_tags`），点击芯片切换选中态，多选 AND 组合；再次点击"标签"筛选项收起芯片行。未选中任何芯片时等价于"全部"。筛选状态与现有 `activeFilter` 同构（`SearchShell` 组件局部 state）；查询键内嵌完整 query 对象，`tagNames` 变化自然触发重新查询。

**列表展示**：`ClipItemSummary` 新增 `tags: string[]`，仓储层用聚合子查询带入现有查询（见 §6），搜索与速贴的卡片可直接渲染芯片，无需逐条回查。

### 5. 各窗口交互

#### 编辑窗口（`EditorShell`）—— 标签编辑主入口

- 文本条目：标签区放在文本编辑区上方；图片 / 文件条目：放在"此条目无法编辑"卡片内（标签对任意类型生效）。
- 交互：输入框输入 + Enter 添加；输入时下拉建议已有标签（前缀匹配，忽略大小写，数据来自 `list_tags`）；芯片右上角 × 移除；整行芯片换行展示。
- **即时提交**：添加 / 移除立即调用 `set_item_tags`（全量替换），不并入文本的 dirty 状态——标签变更与文本保存互不干扰，避免"没点保存标签就丢了"的困惑。提交期间芯片按乐观更新展示，**失败时回滚为提交前状态**，并在现有错误横幅展示错误信息。
- 提交后 `CLIPS_CHANGED_EVENT` 触发列表刷新。

#### 搜索窗口（`SearchShell`）—— 筛选与展示

- 卡片在 `inlineMetaRow`（来源、时间行）下方展示标签芯片，最多 3 个，超出显示 `+N`；chip 使用弱化样式（背景 `pg-canvas-subtle`），不抢选中态视觉。
- 筛选下拉新增"标签"项；激活时头部下方出现芯片行（全部标签，超出横向滚动）。
- 键盘操作保持现状不新增按键；芯片行用 Tab / 方向键可达，回车切换选中。筛选下拉自身的键盘模型（`filterKeyboard`）随 `FILTER_OPTIONS` 新增项自然继承。

#### 设置窗口 —— 标签管理

- 新增 `"tags"` section（"标签"，描述"管理剪贴记录标签"），位于"排除应用"之后。
- 列表展示：标签名 + 使用次数 + 操作（重命名 / 删除）；组件内监听 `TAGS_CHANGED_EVENT` 刷新（设置窗口目前只监听 `SETTINGS_CHANGED_EVENT`，此为新增监听）。
- 重命名：行内编辑；新名与其他标签冲突时提示"已存在，将合并"（前端用 `list_tags` 数据按忽略大小写比对），确认后合并。
- 删除：二次确认；删除后相关条目的芯片同步消失（`CLIPS_CHANGED_EVENT` 驱动搜索 / 速贴窗口刷新）。

#### 速贴面板（`PickerShell`）—— 只读展示

- 选中卡片（selectedIndex 对应项）在内容下方展示至多 2 个标签芯片；不提供编辑与筛选入口，保持速贴"快"的定位。

### 6. 列表查询的标签聚合

`list_recent`、`list_favorites`、`search_recent`、`search_with_keyword` 四条列表查询统一追加一个聚合子查询列（置于 SELECT 列表末尾）：

```sql
(SELECT GROUP_CONCAT(tag_name, char(31)) FROM
   (SELECT tag_name FROM clip_item_tags WHERE item_id = ci.id
    ORDER BY tag_name COLLATE NOCASE)) AS tag_names
```

- **分隔符必须用 `char(31)`**：SQLite 字符串字面量没有 `\x` 转义，`'\x1f'` 只是四个字面字符，不是单位分隔符。
- 内层 `ORDER BY` 使 `GROUP_CONCAT` 输出顺序确定，卡片芯片不因查询而跳动。
- 无标签时 `GROUP_CONCAT` 返回 NULL，Rust 映射为空数组；按 `char(31)` 拆分后即 `tags: string[]`。
- `map_summary_row` 按列序读取，`tag_names` 追加在 SELECT 末尾（现有末列为索引 17 的 `substr(full_text, ...)`，新列为 18），需同步扩展。
- 详情查询（`get_item_detail`）同样带回标签，保证编辑窗口会话内数据一致。

`list_tags` 计数排除软删除条目：

```sql
SELECT t.name, t.created_at, COUNT(ci.id) AS item_count
FROM tags t
LEFT JOIN clip_item_tags cit ON cit.tag_name = t.name
LEFT JOIN clip_items ci ON ci.id = cit.item_id AND ci.deleted_at IS NULL
GROUP BY t.name
ORDER BY item_count DESC, t.name COLLATE NOCASE ASC
```

### 7. 迁移（schema v5）

当前 `CURRENT_SCHEMA_VERSION` 为 4，新版本为 **5**：

1. 建 `tags`、`clip_item_tags` 两张表——语句必须带 `IF NOT EXISTS`：全新库路径会先执行 `0001_init.sql`（已含新表）再跑增量迁移，非幂等建表会让首次启动直接失败。
2. `DROP TABLE clip_items_fts` 后按新结构（含 `tags` 列）重建，从 `clip_items` 关联 `clip_item_tags` 回填标签列；所有列（含 `tags`）写入前照旧 `space_cjk_text`；FTS 列序与 `0001_init.sql` 一致（`item_id, full_text, search_text, source_app, tags`）。
3. `PRAGMA user_version = 5`。

- 存量库会连续执行 v4（CJK 空格化重建）与 v5（加 `tags` 列重建）两次 FTS 全量重建，桌面数据量级可接受；若实现时 v4 尚未随任何版本发布，也可把 FTS 结构变更并入 v4 一次完成（需同步调整 v4 测试断言），默认按独立 v5 实施。
- `clip_item_tags` 为空表时重建 FTS 无压力；存量数据在首次录入标签后由 `set_item_tags` 同步更新对应行的 FTS `tags` 列。

### 8. 命令层与前端对接

- `src/bridge/commands.ts` 新增 `listTags`、`setItemTags`、`renameTag`、`deleteTag`，Tauri 分支调 `invoke`，浏览器分支走 `mockBackend`（mock 数据为 demo 条目补一组标签，`rankItems` 同步支持 `tagNames` AND 过滤）。
- `src/shared/types/clips.ts` 新增 `TagInfo`、`SearchFilters.tagNames`；`ClipItemSummary` / `ClipItemDetail` 新增 `tags: string[]`。
- 事件：`src/bridge/events.ts` 与 `src-tauri/src/domain/events.rs` 同步新增 `TAGS_CHANGED_EVENT = "tags://changed"`。
- 4 个新命令需注册进 `src-tauri/src/lib.rs` 的 `generate_handler!` 列表。

## 影响文件

**Rust**
- `src-tauri/migrations/0001_init.sql`（新表定义 + FTS 含 `tags` 列，供全新库）
- `src-tauri/src/repository/schema.rs`（v5 迁移 + FTS 重建）
- `src-tauri/src/repository/sqlite_repository.rs`（`set_item_tags` / `rename_tag` / `delete_tag` / `list_tags`、列表查询聚合与 `ci` 别名、`update_text` 回填 tags、`build_filters_clause_with_alias` 扩展）
- `src-tauri/src/domain/clip_item.rs`（`TagInfo`、`SearchFilters.tagNames`、summary/detail 加 `tags`）
- `src-tauri/src/domain/events.rs`（`TAGS_CHANGED_EVENT`）
- `src-tauri/src/services/tag_service.rs`（新增，业务规则与 FTS 同步）
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/commands/clips.rs`（4 个新命令与事件广播）
- `src-tauri/src/lib.rs`（`generate_handler!` 注册新命令）

**前端**
- `src/shared/types/clips.ts`
- `src/bridge/commands.ts`、`src/bridge/events.ts`、`src/bridge/mockBackend.ts`
- `src/features/editor/EditorShell.tsx`（标签编辑区 + 建议下拉，新增 `src/features/editor/tagEditor.tsx` 组件与 `src/features/editor/tags.ts` 辅助）
- `src/features/search/SearchShell.tsx`、`src/features/search/state.ts`、`src/features/search/queries.ts`（`tag` 筛选 + 芯片行 + 卡片芯片）
- `src/features/picker/PickerShell.tsx`（选中卡片芯片展示）
- `src/features/settings/settingsSections.ts` + 新增 `src/features/settings/sections/TagsSection.tsx`（标签管理，含 `TAGS_CHANGED_EVENT` 监听）

## 验证策略

- Rust：`./scripts/win-cargo test`，仓储测试覆盖——
  - `set_item_tags` 全量替换 / 幂等 / 空数组清空 / 重复与大小写变体输入去重；
  - 重命名、重命名合并（大小写变体合并到 NOCASE 唯一）；重命名后旧标签名不再命中搜索、新标签名可命中（FTS 同步）；
  - `update_text` 保存文本后既有标签仍命中搜索；
  - 删除标签后关联消失、`list_tags` 计数正确且不含软删除条目；
  - 按标签筛选（单标签 / 多标签 AND / 空数组无筛选）、关键词命中标签名的 FTS 路径（含 CJK 标签）；
  - 迁移双路径：存量库（v3 → v4 → v5 连续升级）与全新库（`0001_init.sql` 后跑增量迁移不报错、`user_version = 5`）。
- 前端：`./scripts/win-pnpm build` 验证类型与编译；浏览器 mock 模式手测编辑窗口加标签、搜索筛选、设置重命名。
- 手动回归：编辑窗口 Ctrl+S 保存文本不受标签即时提交影响；速贴面板选中态渲染不破版。

## 后续可扩展（本期不做）

- 自动打标签（按来源应用 / 关键词规则）。
- 标签颜色与图标、标签云统计。
- 速贴面板按标签快速过滤（长按 / 右键）。
- 标签去重合并建议（`列表相近标签`）。
