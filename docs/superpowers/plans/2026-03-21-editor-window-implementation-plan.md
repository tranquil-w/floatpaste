# 独立编辑窗口实现计划

> **给代理执行者：** 必须优先使用 `superpowers:subagent-driven-development`（若可用）或 `superpowers:executing-plans` 来执行本计划。所有步骤均使用 `- [ ]` 复选框跟踪。

**目标：** 新增独立 `Editor` 窗口承接当前项纯编辑，彻底解除 `Picker` 与资料库窗口的直接耦合，并让 `Enter`/方向键回归文本编辑语义。

**架构：** 保留现有 `picker`、`workbench`、`manager` 多窗口架构，新增 `editor` 窗口和对应会话事件。`Picker` 与 `Workbench` 都只把“当前项编辑”交给 `Editor`，窗口切换与快捷键释放/恢复统一收敛到 Rust `WindowCoordinator` 与 `ShortcutManager`，前端只处理会话事件、纯编辑 UI 和未保存确认。

**技术栈：** React 19、TypeScript、Zustand、TanStack Query、Tauri 2、Rust、`node:test`（现有纯状态测试）、`pnpm build`、`cargo test`

---

## 文件结构与责任划分

### 计划内新增文件

- `src/features/editor/EditorShell.tsx`
  负责独立编辑窗口 UI、会话监听、文本加载、保存、关闭确认。
- `src/features/editor/store.ts`
  负责编辑窗口会话、脏状态、来源窗口元信息和关闭确认状态。
- `src/features/editor/index.ts`
  统一导出 `EditorShell`。
- `src-tauri/src/domain/editor_session.rs`
  定义编辑窗口会话结构、来源枚举、返回目标枚举。

### 计划内主要修改文件

- `src/app/App.tsx`
  根据新窗口标签渲染 `EditorShell`，补充 `window-editor`/`theme-editor` 类名。
- `src/bridge/commands.ts`
  删除 `Picker -> Workbench/Manager` 的直连命令，新增 `open_editor_from_picker`、`open_editor_from_workbench`、`hide_editor`。
- `src/bridge/events.ts`
  删除不再使用的 `Picker -> Workbench` 事件，新增 `EDITOR_SESSION_START_EVENT`、`EDITOR_SESSION_END_EVENT`。
- `src/bridge/mockBackend.ts`
  如本地预览需要，补齐编辑窗口相关 mock 行为和示例数据。
- `src/features/picker/PickerShell.tsx`
  删除打开资料库窗口的 UI 与快捷键入口，改为只保留“编辑当前项”入口。
- `src/features/workbench/WorkbenchShell.tsx`
  移除嵌入式编辑面板，收敛为搜索/定位窗口，并改为从当前项进入 `Editor`。
- `src/features/workbench/store.ts`
  删除与嵌入式文本编辑强绑定的状态，保留搜索、选中、来源上下文。
- `src/features/workbench/state.ts`
  只保留列表导航相关纯函数；若当前缓存草稿逻辑已不再需要则一并删除。
- `src-tauri/tauri.conf.json`
  预创建 `editor` 窗口并补充 capability。
- `src-tauri/src/commands/windows.rs`
  删除 `open_manager_from_picker`、`open_workbench_from_picker_edit`、`open_workbench_from_picker_search` 的前端入口暴露，新增编辑窗口开关命令。
- `src-tauri/src/commands/mod.rs`
  注册新增/移除的窗口命令。
- `src-tauri/src/domain/events.rs`
  删除旧的 `PICKER_OPEN_WORKBENCH_*` 常量，新增 `EDITOR_SESSION_*` 常量。
- `src-tauri/src/domain/mod.rs`
  导出新增 `editor_session` 模块。
- `src-tauri/src/services/window_coordinator.rs`
  新增 `editor` 窗口创建、打开、关闭、来源恢复；删除 `Picker -> Workbench` 的直接流转。
- `src-tauri/src/services/shortcut_manager.rs`
  删除 `Picker` 中打开资料库窗口的会话快捷键，新增编辑窗口快捷键边界，确保 `Editor` 不注册会抢占文本输入的会话键。
- `src-tauri/src/app_bootstrap.rs`
  若现有状态容器按窗口维护激活标记，需要补充 editor 激活状态与快捷键注册状态。
- `src-tauri/src/domain/workbench_session.rs`
  去掉仅用于 `Picker -> Workbench` 返回链路的字段，保留 `Workbench` 自身来源上下文。
- `tests/workbenchState.test.ts`
  若 `workbench/state.ts` 纯函数被删改，需要同步更新或删除不再成立的测试。

### 计划内测试文件

- `src-tauri/src/services/shortcut_manager.rs`
  补充会话快捷键集合与清理策略的单元测试。
- `src-tauri/src/services/window_coordinator.rs`
  如当前结构允许，补充来源恢复与命令路由的纯逻辑测试；若难以直接测 UI 窗口对象，则至少把可纯化的恢复决策抽成可测函数。

### 范围提醒

- 不新增前端测试框架；前端验证以 `pnpm build` + 手工场景回归为主。
- 不在本轮内改 `ManagerShell` 的资料库 UI，只清理它与 `Picker` 的直接关系。

## Chunk 1: 后端窗口模型与跨窗口关系拆解

### Task 1: 新增 editor 窗口配置与会话模型

**文件：**
- Create: `src-tauri/src/domain/editor_session.rs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/src/domain/events.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/app_bootstrap.rs`

- [ ] **Step 1: 写一个失败的 Rust 单元测试，锁定 editor 会话来源与返回目标的序列化约束**

在 `src-tauri/src/domain/editor_session.rs` 中先写测试，至少覆盖：

```rust
#[test]
fn editor_session_source_and_return_target_should_roundtrip() {
    let session = EditorSession {
        item_id: "clip-1".to_string(),
        source: EditorSource::Picker,
        return_to: EditorReturnTarget::Picker,
    };

    let json = serde_json::to_string(&session).unwrap();
    assert!(json.contains("\"source\":\"picker\""));
    assert!(json.contains("\"returnTo\":\"picker\""));
}
```

- [ ] **Step 2: 运行测试确认当前失败**

运行：`cargo test editor_session_source_and_return_target_should_roundtrip`

预期：失败，提示 `editor_session` 模块或类型尚不存在。

- [ ] **Step 3: 以最小实现补齐 editor 会话模型与事件常量**

需要完成：

- 新建 `EditorSession`、`EditorSource`、`EditorReturnTarget`
- 新增 `EDITOR_SESSION_START_EVENT`
- 新增 `EDITOR_SESSION_END_EVENT`
- 在 `tauri.conf.json` 中预创建 `editor` 窗口并补 capability
- 在 `AppState` 中补 editor 激活/快捷键状态（若当前状态容器按窗口区分）

- [ ] **Step 4: 重新运行单测确认通过**

运行：`cargo test editor_session_source_and_return_target_should_roundtrip`

预期：PASS。

- [ ] **Step 5: 提交本任务**

运行：

```bash
git add src-tauri/tauri.conf.json src-tauri/src/domain/editor_session.rs src-tauri/src/domain/events.rs src-tauri/src/domain/mod.rs src-tauri/src/app_bootstrap.rs
git commit -m "feat: 新增编辑窗口会话模型"
```

### Task 2: 删除 Picker 直连资料库窗口的 Rust 命令链

**文件：**
- Modify: `src-tauri/src/commands/windows.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/services/window_coordinator.rs`
- Modify: `src-tauri/src/domain/workbench_session.rs`

- [ ] **Step 1: 先写失败测试，锁定不再保留 Picker -> Workbench 的直接返回链**

若 `window_coordinator.rs` 里没有可测纯函数，先抽一个最小纯函数，例如：

```rust
fn should_restore_picker_after_workbench_close(session: &WorkbenchSession) -> bool {
    !matches!(session.source, WorkbenchSource::GlobalShortcut)
}
```

然后写测试，目标是把逻辑改为“关闭 workbench 不再恢复 picker”：

```rust
#[test]
fn workbench_close_should_not_restore_picker_flow() {
    let session = WorkbenchSession::for_picker_search(None);
    assert!(!should_restore_picker_after_workbench_close(&session));
}
```

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test workbench_close_should_not_restore_picker_flow`

预期：失败，因为旧结构仍把 `from_picker` 视为需要恢复 Picker。

- [ ] **Step 3: 最小实现清理旧命令链**

需要完成：

- 删除 `open_manager_from_picker`
- 删除 `open_workbench_from_picker_edit`
- 删除 `open_workbench_from_picker_search`
- 删除/重构 `WorkbenchSession` 中仅服务 `Picker -> Workbench -> Picker` 的字段
- 删除 `WindowCoordinator` 中 `hide_picker_and_open_manager`、`open_workbench_from_picker_*` 的直接流转依赖

- [ ] **Step 4: 运行定向测试与全量 Rust 测试**

运行：

```bash
cargo test workbench_close_should_not_restore_picker_flow
cargo test
```

预期：PASS。

- [ ] **Step 5: 提交本任务**

运行：

```bash
git add src-tauri/src/commands/windows.rs src-tauri/src/commands/mod.rs src-tauri/src/services/window_coordinator.rs src-tauri/src/domain/workbench_session.rs
git commit -m "refactor: 断开 Picker 与资料库窗口直连"
```

### Task 3: 新增 editor 打开/关闭命令与快捷键释放恢复逻辑

**文件：**
- Modify: `src-tauri/src/commands/windows.rs`
- Modify: `src-tauri/src/services/window_coordinator.rs`
- Modify: `src-tauri/src/services/shortcut_manager.rs`
- Modify: `src-tauri/src/domain/events.rs`
- Test: `src-tauri/src/services/shortcut_manager.rs`

- [ ] **Step 1: 先写失败测试，锁定 editor 打开时不应保留会抢占文本输入的会话键**

在 `shortcut_manager.rs` 中补测试，目标至少覆盖：

```rust
#[test]
fn picker_session_shortcuts_should_not_contain_workbench_jump_keys_after_editor_split() {
    assert!(!is_picker_session_shortcut("ctrl+f"));
}
```

以及：

```rust
#[test]
fn editor_window_should_not_register_navigation_shortcuts() {
    assert!(!is_editor_session_shortcut("arrowup"));
    assert!(!is_editor_session_shortcut("enter"));
}
```

如果不打算为 editor 定义会话快捷键集合，则第二个测试改为验证“不存在 editor 会话快捷键注册函数”对应的纯逻辑辅助函数。

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test picker_session_shortcuts_should_not_contain_workbench_jump_keys_after_editor_split`

预期：失败，因为当前 `Picker` 仍包含 `Ctrl+F` 且仍绑定跳资料库窗口。

- [ ] **Step 3: 以最小实现新增 editor 命令并收紧快捷键边界**

需要完成：

- 新增 `open_editor_from_picker(item_id)`
- 新增 `open_editor_from_workbench(item_id)`
- 新增 `hide_editor()`
- `Picker` 打开 `Editor` 前释放 Picker 会话快捷键
- `Workbench` 打开 `Editor` 前释放 Workbench 会话快捷键
- `Editor` 本身不注册 `Up / Down / Enter` 这类会抢占文本输入的会话快捷键
- 关闭 `Editor` 时依据 `return_to` 恢复来源窗口与对应会话快捷键

- [ ] **Step 4: 运行 Rust 测试确认通过**

运行：`cargo test`

预期：PASS。

- [ ] **Step 5: 提交本任务**

运行：

```bash
git add src-tauri/src/commands/windows.rs src-tauri/src/services/window_coordinator.rs src-tauri/src/services/shortcut_manager.rs src-tauri/src/domain/events.rs
git commit -m "feat: 新增编辑窗口命令与快捷键边界"
```

## Chunk 2: 前端 EditorShell 与窗口路由接入

### Task 4: 接入 editor 窗口标签与桥接命令

**文件：**
- Modify: `src/app/App.tsx`
- Modify: `src/bridge/commands.ts`
- Modify: `src/bridge/events.ts`
- Modify: `src/bridge/mockBackend.ts`
- Create: `src/features/editor/index.ts`

- [ ] **Step 1: 写一个失败的类型检查目标，确保前端能识别 editor 窗口标签**

先在 `App.tsx` 引入但不实现 `EditorShell`：

```tsx
if (windowLabel === "editor") {
  return <EditorShell />;
}
```

- [ ] **Step 2: 运行前端构建确认失败**

运行：`pnpm build`

预期：失败，提示 `EditorShell`、`window-editor` 或新桥接命令尚不存在。

- [ ] **Step 3: 最小实现前端桥接层**

需要完成：

- `App.tsx` 新增 `editor` 标签分支
- 增加 `window-editor` / `theme-editor` 类名
- `commands.ts` 新增 editor 相关命令
- `events.ts` 新增 editor 相关事件
- mock 后端对 editor 命令给出最小兼容返回

- [ ] **Step 4: 重新运行构建确认通过**

运行：`pnpm build`

预期：PASS。

- [ ] **Step 5: 提交本任务**

运行：

```bash
git add src/app/App.tsx src/bridge/commands.ts src/bridge/events.ts src/bridge/mockBackend.ts src/features/editor/index.ts
git commit -m "feat: 接入编辑窗口前端桥接"
```

### Task 5: 实现 EditorShell、会话状态与未保存确认

**文件：**
- Create: `src/features/editor/EditorShell.tsx`
- Create: `src/features/editor/store.ts`
- Modify: `src/features/manager/queries.ts`
- Modify: `src/shared/types/clips.ts`（仅当 editor 需要补类型）

- [ ] **Step 1: 先写最小失败目标，锁定编辑窗口必须具备会话、保存和关闭确认状态**

先定义 store 接口但不实现，例如：

```ts
type EditorSession = {
  itemId: string;
  source: "picker" | "workbench";
  returnTo: "picker" | "workbench";
};
```

并让 `EditorShell` 依赖这些字段渲染，制造构建失败。

- [ ] **Step 2: 运行前端构建确认失败**

运行：`pnpm build`

预期：失败，提示 editor store 或 session 字段不完整。

- [ ] **Step 3: 实现最小可用 EditorShell**

至少完成以下行为：

- 监听 `EDITOR_SESSION_START_EVENT` / `EDITOR_SESSION_END_EVENT`
- 通过 `getItemDetail` 加载当前文本条目
- `textarea` 自动聚焦
- `Enter` 与方向键完全交给文本输入
- `Ctrl+S` 触发保存
- 关闭按钮与 `Esc` 走统一 `requestClose()`
- 脏状态下弹 `保存并关闭 / 放弃修改 / 取消`
- 保存成功后刷新相关 query cache

- [ ] **Step 4: 运行构建确认通过**

运行：`pnpm build`

预期：PASS。

- [ ] **Step 5: 提交本任务**

运行：

```bash
git add src/features/editor/EditorShell.tsx src/features/editor/store.ts src/features/manager/queries.ts src/shared/types/clips.ts
git commit -m "feat: 实现独立编辑窗口"
```

## Chunk 3: Picker / Workbench 收口与验证

### Task 6: 收口 Picker，只保留轻量选择与当前项编辑

**文件：**
- Modify: `src/features/picker/PickerShell.tsx`
- Modify: `src/bridge/commands.ts`
- Modify: `src/bridge/events.ts`
- Modify: `src-tauri/src/services/shortcut_manager.rs`
- Modify: `src-tauri/src/domain/events.rs`

- [ ] **Step 1: 先写失败测试，锁定 Picker 不再暴露资料库跳转快捷键**

在 `shortcut_manager.rs` 中新增或扩展测试：

```rust
#[test]
fn picker_session_shortcuts_should_only_keep_selection_and_edit_actions() {
    assert!(!is_picker_session_shortcut("ctrl+f"));
}
```

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test picker_session_shortcuts_should_only_keep_selection_and_edit_actions`

预期：失败，因为当前 `PICKER_SESSION_SHORTCUTS` 仍包含 `Ctrl+F`。

- [ ] **Step 3: 最小实现 Picker 收口**

需要完成：

- 移除 `openManagerFromPicker`
- 移除 `openWorkbenchFromPickerSearch`
- `Ctrl+E` 或现有“编辑”按钮只进入 `Editor`
- 删除不再使用的 `PICKER_OPEN_WORKBENCH_*` 事件
- 保留 `Picker` 的列表选择、回贴和关闭能力

- [ ] **Step 4: 运行 Rust + 前端验证**

运行：

```bash
cargo test
pnpm build
```

预期：PASS。

- [ ] **Step 5: 提交本任务**

运行：

```bash
git add src/features/picker/PickerShell.tsx src/bridge/commands.ts src/bridge/events.ts src-tauri/src/services/shortcut_manager.rs src-tauri/src/domain/events.rs
git commit -m "refactor: 收口 Picker 交互职责"
```

### Task 7: 收口 Workbench，只保留搜索定位并进入 Editor

**文件：**
- Modify: `src/features/workbench/WorkbenchShell.tsx`
- Modify: `src/features/workbench/store.ts`
- Modify: `src/features/workbench/state.ts`
- Modify: `tests/workbenchState.test.ts`
- Modify: `src/bridge/commands.ts`

- [ ] **Step 1: 先写失败的状态测试，锁定 workbench 只保留导航状态**

如果 `tests/workbenchState.test.ts` 仍覆盖嵌入式编辑缓存逻辑，先把期望改成新的状态边界，例如保留：

```ts
test("快速连续向下导航时，仍基于当前选中项计算下一个索引", () => {
  const items = [createSummary("a"), createSummary("b"), createSummary("c")];
  assert.equal(getNextWorkbenchNavigationIndex(items, "a", "down"), 1);
});
```

并删除/改写已不再成立的 `getCachedTextStateForSelection` 测试。

- [ ] **Step 2: 运行测试确认失败**

运行：`node --test tests/workbenchState.test.ts`

预期：如果当前仓库不能直接执行 `.ts` 测试，则记录这一点，并改为把这一步落为 `pnpm build` 的失败基线；不要为本轮额外引入测试框架。

- [ ] **Step 3: 最小实现 Workbench 收口**

需要完成：

- 删除内嵌 `EditPanel` 文本编辑主流程
- 删除 `pasteMutation`、`updateTextMutation` 在 Workbench 主界面的直接使用
- 搜索结果列表仅负责选中当前项与触发“编辑当前项”
- 关闭 Workbench 时只恢复其自身来源窗口，不再恢复 Picker 链路

- [ ] **Step 4: 运行验证**

运行：

```bash
pnpm build
cargo test
```

预期：PASS。

- [ ] **Step 5: 提交本任务**

运行：

```bash
git add src/features/workbench/WorkbenchShell.tsx src/features/workbench/store.ts src/features/workbench/state.ts tests/workbenchState.test.ts src/bridge/commands.ts
git commit -m "refactor: 收口资料库窗口搜索职责"
```

### Task 8: 端到端回归与文档收尾

**文件：**
- Modify: `docs/superpowers/specs/2026-03-21-editor-window-design.md`（仅当实现与规格存在必要偏差时）
- Modify: `docs/superpowers/plans/2026-03-21-editor-window-implementation-plan.md`（勾选执行进度时）

- [ ] **Step 1: 运行最终自动验证**

运行：

```bash
pnpm build
cargo test
```

预期：全部 PASS。

- [ ] **Step 2: 做手工回归**

至少验证：

- `Picker -> Editor -> Picker`
- `Workbench -> Editor -> Workbench`
- `Editor` 中 `Enter` 换行
- `Editor` 中方向键仅移动光标
- 未保存关闭三路径：关闭按钮、`Esc`、窗口系统关闭
- 保存后来源窗口可见最新文本

- [ ] **Step 3: 若实现与规格不一致，先更新 spec，再更新计划勾选状态**

要求：

- 只记录真实发生的偏差
- 不要修改已确认的产品边界

- [ ] **Step 4: 提交最终收尾**

运行：

```bash
git add docs/superpowers/specs/2026-03-21-editor-window-design.md docs/superpowers/plans/2026-03-21-editor-window-implementation-plan.md
git commit -m "docs: 更新独立编辑窗口实施记录"
```

## 执行备注

- 现有仓库没有正式前端测试框架，因此不要为了这次需求临时引入大型测试基础设施。
- 所有前端行为改动至少通过 `pnpm build`。
- Rust 侧窗口协调与快捷键边界优先使用单元测试锁定。
- 如 `tests/workbenchState.test.ts` 目前没有稳定执行入口，可以保留该文件仅作为纯函数样例，并把正式自动验证集中到 `pnpm build` 与 `cargo test`。
- 每个任务完成后都要立即提交，避免跨任务大混改。

## 完成标准

满足以下条件才能视为计划执行完成：

- `Picker` 不再直接打开资料库窗口
- `Workbench` 不再嵌入文本编辑主流程
- `Editor` 成为唯一文本编辑入口
- `Enter` 与方向键在 `Editor` 中恢复文本语义
- 未保存关闭保护完整
- `pnpm build` 与 `cargo test` 通过
