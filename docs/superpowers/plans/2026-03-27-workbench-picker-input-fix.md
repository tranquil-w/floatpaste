# 搜索窗口与速贴窗口输入模型修复 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复搜索窗口错误拦截全局按键、长按方向键只触发一次，以及搜索窗口与速贴窗口并存时的快捷键失效与卡死问题。

**Architecture:** 将 Workbench 从“全局会话快捷键驱动”调整为“窗口聚焦时的本地键盘处理”，仅保留其唤起快捷键为全局入口。Picker 继续维持“无焦点 + 全局会话快捷键”模型，并在打开与关闭过程中显式暂停或恢复 Workbench 会话，确保同一时刻只有一个输入所有者。

**Tech Stack:** React 19、TypeScript、Tauri 2、Rust、node:test

---

## Chunk 1: Workbench 本地键盘模型

### Task 1: 先补前端行为测试

**Files:**
- Create: `src/features/workbench/keyboard.ts`
- Create: `tests/workbenchKeyboard.test.ts`

- [ ] **Step 1: 写失败测试**
- [ ] **Step 2: 运行 `node --test tests/workbenchKeyboard.test.ts`，确认失败**
- [ ] **Step 3: 实现最小键盘动作映射函数**
- [ ] **Step 4: 再次运行 `node --test tests/workbenchKeyboard.test.ts`，确认通过**

### Task 2: Workbench 改为窗口内键盘处理

**Files:**
- Modify: `src/features/workbench/WorkbenchShell.tsx`
- Modify: `src/features/workbench/state.ts`

- [ ] **Step 1: 在 `WorkbenchShell` 中接入本地 `keydown` 处理**
- [ ] **Step 2: 用浏览器长按重复语义支持 `ArrowUp/ArrowDown` 连续导航**
- [ ] **Step 3: 保持 `Enter / Ctrl+Enter / Escape` 与现有行为一致**
- [ ] **Step 4: 确认输入框聚焦时不会影响文本输入和快捷键组合**

## Chunk 2: Rust 侧会话所有权收口

### Task 3: 先补 Rust 回归测试

**Files:**
- Modify: `src-tauri/src/services/shortcut_manager.rs`

- [ ] **Step 1: 写失败测试，约束 Workbench 不再注册会话导航快捷键**
- [ ] **Step 2: 运行 `cargo test --manifest-path src-tauri/Cargo.toml shortcut_manager`，确认失败**
- [ ] **Step 3: 实现最小改动让测试通过**
- [ ] **Step 4: 再次运行同一条命令，确认通过**

### Task 4: Picker / Workbench 会话独占

**Files:**
- Modify: `src-tauri/src/services/shortcut_manager.rs`
- Modify: `src-tauri/src/services/window_coordinator.rs`
- Modify: `src-tauri/src/app_bootstrap.rs`

- [ ] **Step 1: 移除 Workbench 会话级全局方向键与确认键注册**
- [ ] **Step 2: 打开 Picker 时显式暂停 Workbench 会话与其残留快捷键**
- [ ] **Step 3: Picker 从 Workbench 来源关闭后恢复 Workbench 窗口焦点与本地输入**
- [ ] **Step 4: 保持 Picker 从外部应用来源关闭后仍恢复目标软件**

## Chunk 3: 验证

### Task 5: 完整验证

**Files:**
- Modify: `tests/workbenchState.test.ts`

- [ ] **Step 1: 运行 `node --test tests/workbenchState.test.ts tests/workbenchKeyboard.test.ts tests/editorKeyboard.test.ts`**
- [ ] **Step 2: 运行 `cargo test --manifest-path src-tauri/Cargo.toml shortcut_manager`**
- [ ] **Step 3: 运行 `pnpm build`**
- [ ] **Step 4: 手动验证以下场景**

手动验证清单：
- [ ] 搜索窗口打开后，切到其他软件，`Enter / 上下键` 不再被 FloatPaste 拦截
- [ ] 搜索窗口内长按 `上/下` 能连续切换选中项
- [ ] 搜索窗口打开后再打开 Picker，双击粘贴后搜索窗口仍可继续使用快捷键
- [ ] 搜索窗口打开后再打开 Picker，`Esc` 只关闭当前拥有输入权的窗口，不再出现 Picker 卡死
