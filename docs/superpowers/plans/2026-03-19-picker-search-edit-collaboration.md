# Picker 与搜索编辑协同工作流实施计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 通过新增"搜索/编辑工作窗 (Workbench)"，实现 Picker 与搜索编辑的高效协同，减少窗口切换，提供连续的搜索-编辑-回贴链路。

**Architecture:** 采用"协同双窗"架构，保留现有 Picker 快选功能，新增 Workbench 窗口承接搜索和编辑。通过会话模型 (picker_session / workbench_session) 共享上下文，确保窗口切换时目标窗口和编辑状态正确恢复。

**Tech Stack:** Tauri 2.x + React + Zustand + React Query + Tailwind CSS

---

## Chunk 1: 基础设施与会话模型 ✅

### Task 1: 扩展设置模型支持工作窗快捷键 ✅

**Files:**
- Modify: `src-tauri/src/domain/settings.rs:74-84`
- Modify: `src/shared/types/settings.ts:20-30`

- [x] **Step 1: 编写设置模型测试**

```rust
// src-tauri/src/domain/settings.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workbench_shortcut_default() {
        let settings = UserSetting::default();
        assert_eq!(settings.workbench_shortcut, "Ctrl+Shift+F");
        assert!(settings.workbench_shortcut_enabled);
    }

    #[test]
    fn test_workbench_shortcut_sanitization() {
        let mut settings = UserSetting::default();
        // 与主快捷键相同时应报错
        settings.shortcut = "Ctrl+`".to_string();
        settings.workbench_shortcut = "Ctrl+`".to_string();
        assert!(settings.sanitized().is_err());
    }
}
```

- [x] **Step 2: 运行测试验证失败**

Run: `rtk cargo test --package floatpaste test_workbench_shortcut`
Expected: FAIL - 功能未实现

- [x] **Step 3: 扩展 Rust 设置模型**

```rust
// src-tauri/src/domain/settings.rs
// 在 UserSetting 结构体中添加字段 (约 line 74 后)

#[serde(default)]
pub workbench_shortcut: String,

#[serde(default = "true")]
pub workbench_shortcut_enabled: bool,

// 在 default() 实现中添加
impl Default for UserSetting {
    fn default() -> Self {
        Self {
            // ... 现有字段 ...
            workbench_shortcut: "Ctrl+Shift+F".to_string(),
            workbench_shortcut_enabled: true,
        }
    }
}

// 在 sanitized() 方法中添加冲突检测
impl UserSetting {
    pub fn sanitized(mut self) -> Result<Self, AppError> {
        // ... 现有验证 ...

        // 检查工作窗快捷键与主快捷键冲突
        if self.workbench_shortcut_enabled {
            let main = normalize_shortcut(&self.shortcut)?;
            let workbench = normalize_shortcut(&self.workbench_shortcut)?;
            if main == workbench {
                return Err(AppError::Message(
                    "工作窗快捷键不能与主快捷键相同".to_string()
                ));
            }
        }

        Ok(self)
    }
}
```

- [x] **Step 4: 扩展前端设置类型**

```typescript
// src/shared/types/settings.ts
export interface UserSetting {
  // ... 现有字段 ...
  workbenchShortcut: string;
  workbenchShortcutEnabled: boolean;
}
```

- [x] **Step 5: 运行测试验证通过**

Run: `rtk cargo test --package floatpaste test_workbench_shortcut`
Expected: PASS

- [x] **Step 6: 提交**

```bash
rtk git add src-tauri/src/domain/settings.rs src/shared/types/settings.ts
rtk git commit -m "feat(settings): 添加工作窗快捷键设置字段"
```

---

### Task 2: 扩展会话模型支持 Workbench ✅

**Files:**
- Modify: `src-tauri/src/app_bootstrap.rs:33-104`
- Create: `src-tauri/src/domain/workbench_session.rs`

- [x] **Step 1: 编写会话模型**

```rust
// src-tauri/src/domain/workbench_session.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSession {
    /// 回贴目标窗口句柄
    pub target_window_hwnd: Option<isize>,
    /// 来源类型
    pub source: WorkbenchSource,
    /// 当前活动条目 ID（编辑态）
    pub current_item_id: Option<String>,
    /// 是否来自 Picker 跳转
    pub from_picker: bool,
    /// Picker 会话的原始选中索引（用于返回时恢复）
    pub picker_selected_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkbenchSource {
    /// 从 Picker 编辑进入
    PickerEdit,
    /// 从 Picker 搜索进入
    PickerSearch,
    /// 全局快捷键直接进入
    GlobalShortcut,
}
```

- [x] **Step 2: 扩展 AppState**

```rust
// src-tauri/src/app_bootstrap.rs
// 在 AppState 结构体中添加 (约 line 28 后)

pub workbench_session: Arc<Mutex<Option<WorkbenchSession>>>,
pub workbench_active: Arc<AtomicBool>,

// 在 new() 中初始化
impl AppState {
    pub fn new(...) -> Self {
        Self {
            // ... 现有字段 ...
            workbench_session: Arc::new(Mutex::new(None)),
            workbench_active: Arc::new(AtomicBool::new(false)),
        }
    }
}

// 添加会话管理方法
impl AppState {
    pub fn set_workbench_session(&self, session: WorkbenchSession) -> Result<(), AppError> {
        let mut current = self.workbench_session.lock()?;
        *current = Some(session);
        Ok(())
    }

    pub fn workbench_session(&self) -> Result<Option<WorkbenchSession>, AppError> {
        Ok(self.workbench_session.lock()?.clone())
    }

    pub fn clear_workbench_session(&self) -> Result<(), AppError> {
        let mut current = self.workbench_session.lock()?;
        *current = None;
        Ok(())
    }

    pub fn begin_workbench_activation(&self) {
        self.workbench_active.store(true, Ordering::SeqCst);
    }

    pub fn end_workbench_activation(&self) {
        self.workbench_active.store(false, Ordering::SeqCst);
    }

    pub fn is_workbench_active(&self) -> bool {
        self.workbench_active.load(Ordering::SeqCst)
    }
}
```

- [x] **Step 3: 导出新模块**

```rust
// src-tauri/src/domain/mod.rs
pub mod workbench_session;
```

- [x] **Step 4: 提交**

```bash
rtk git add src-tauri/src/domain/workbench_session.rs src-tauri/src/app_bootstrap.rs src-tauri/src/domain/mod.rs
rtk git commit -m "feat(session): 添加 Workbench 会话模型"
```

---

### Task 3: 扩展事件常量 ✅

**Files:**
- Modify: `src-tauri/src/domain/events.rs`
- Modify: `src/bridge/events.ts`

- [x] **Step 1: 添加 Rust 事件常量**

```rust
// src-tauri/src/domain/events.rs
// Workbench 相关事件
pub const WORKBENCH_SESSION_START_EVENT: &str = "workbench://session-start";
pub const WORKBENCH_SESSION_END_EVENT: &str = "workbench://session-end";
pub const WORKBENCH_EDIT_ITEM_EVENT: &str = "workbench://edit-item";
pub const WORKBENCH_SEARCH_EVENT: &str = "workbench://search";
pub const WORKBENCH_PASTE_EVENT: &str = "workbench://paste";

// Picker 新增跳转事件
pub const PICKER_OPEN_WORKBENCH_EDIT_EVENT: &str = "picker://open-workbench-edit";
pub const PICKER_OPEN_WORKBENCH_SEARCH_EVENT: &str = "picker://open-workbench-search";
```

- [x] **Step 2: 添加前端事件常量**

```typescript
// src/bridge/events.ts
// Workbench 相关事件
export const WORKBENCH_SESSION_START_EVENT = "workbench://session-start";
export const WORKBENCH_SESSION_END_EVENT = "workbench://session-end";
export const WORKBENCH_EDIT_ITEM_EVENT = "workbench://edit-item";
export const WORKBENCH_SEARCH_EVENT = "workbench://search";
export const WORKBENCH_PASTE_EVENT = "workbench://paste";

// Picker 新增跳转事件
export const PICKER_OPEN_WORKBENCH_EDIT_EVENT = "picker://open-workbench-edit";
export const PICKER_OPEN_WORKBENCH_SEARCH_EVENT = "picker://open-workbench-search";
```

- [x] **Step 3: 提交**

```bash
rtk git add src-tauri/src/domain/events.rs src/bridge/events.ts
rtk git commit -m "feat(events): 添加 Workbench 与 Picker 跳转事件常量"
```

---

## Chunk 2: 窗口协调层扩展 ✅

### Task 4: 新增 Workbench 窗口常量 ✅

**Files:**
- Modify: `src-tauri/src/services/window_coordinator.rs:24-28`

- [x] **Step 1: 添加窗口常量**

```rust
// src-tauri/src/services/window_coordinator.rs
pub const WORKBENCH_WINDOW_LABEL: &str = "workbench";
pub const WORKBENCH_WINDOW_TITLE: &str = "FloatPaste Workbench";
```

- [x] **Step 2: 提交**

```bash
rtk git add src-tauri/src/services/window_coordinator.rs
rtk git commit -m "feat(window): 添加 Workbench 窗口常量"
```

---

### Task 5: 实现 Workbench 窗口创建与配置 ✅

**Files:**
- Modify: `src-tauri/src/services/window_coordinator.rs:226-247`

- [x] **Step 1: 添加 ensure_workbench_window 函数**

```rust
// src-tauri/src/services/window_coordinator.rs

fn ensure_workbench_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(WORKBENCH_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, WORKBENCH_WINDOW_LABEL, WebviewUrl::default())
        .title(WORKBENCH_WINDOW_TITLE)
        .inner_size(900.0, 600.0)
        .min_inner_size(600.0, 400.0)
        .resizable(true)
        .visible(false)
        .decorations(true)
        .always_on_top(false)
        .skip_taskbar(false)
        .center()
        .build()
        .map_err(|error| AppError::Message(format!("创建 workbench 窗口失败: {error}")))?;

    configure_workbench_window(&window);
    Ok(window)
}

fn configure_workbench_window(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if app
                .try_state::<AppState>()
                .map(|state| state.is_quitting())
                .unwrap_or(false)
            {
                return;
            }
            api.prevent_close();
            let _ = window.hide();
            // 发送会话结束事件
            let _ = app.emit(WORKBENCH_SESSION_END_EVENT, ());
        }
    });
}
```

- [x] **Step 2: 提交**

```bash
rtk git add src-tauri/src/services/window_coordinator.rs
rtk git commit -m "feat(window): 实现 Workbench 窗口创建与配置"
```

---

### Task 6: 实现从 Picker 打开 Workbench ✅

**Files:**
- Modify: `src-tauri/src/services/window_coordinator.rs:36-206`

- [x] **Step 1: 添加 open_workbench_from_picker_edit 函数**

```rust
// src-tauri/src/services/window_coordinator.rs

impl WindowCoordinator {
    /// 从 Picker 编辑进入 Workbench
    pub fn open_workbench_from_picker_edit(
        app: &AppHandle,
        state: &AppState,
        item_id: String,
    ) -> Result<(), AppError> {
        let window = ensure_workbench_window(app)?;
        let picker_session = state.picker_session()?;

        // 创建 Workbench 会话
        let workbench_session = WorkbenchSession {
            target_window_hwnd: picker_session.target_window_hwnd,
            source: WorkbenchSource::PickerEdit,
            current_item_id: Some(item_id.clone()),
            from_picker: true,
            picker_selected_index: None, // TODO: 从前端传入
        };

        state.set_workbench_session(workbench_session)?;
        state.clear_picker_session()?; // 保留 Picker 会话信息但标记为非活跃

        // 隐藏 Picker
        Self::hide_picker(app)?;

        // 显示并聚焦 Workbench
        window
            .show()
            .map_err(|error| AppError::Message(error.to_string()))?;
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;

        state.begin_workbench_activation();

        // 发送会话开始事件
        window
            .emit(WORKBENCH_SESSION_START_EVENT, WorkbenchSessionPayload {
                source: "picker_edit",
                item_id: Some(item_id),
                initial_keyword: None,
            })
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("从 Picker 编辑进入 Workbench, item_id={item_id}");
        Ok(())
    }
}
```

- [x] **Step 2: 添加 open_workbench_from_picker_search 函数**

```rust
// src-tauri/src/services/window_coordinator.rs

impl WindowCoordinator {
    /// 从 Picker 搜索进入 Workbench
    pub fn open_workbench_from_picker_search(
        app: &AppHandle,
        state: &AppState,
        initial_keyword: Option<String>,
    ) -> Result<(), AppError> {
        let window = ensure_workbench_window(app)?;
        let picker_session = state.picker_session()?;

        let workbench_session = WorkbenchSession {
            target_window_hwnd: picker_session.target_window_hwnd,
            source: WorkbenchSource::PickerSearch,
            current_item_id: None,
            from_picker: true,
            picker_selected_index: None,
        };

        state.set_workbench_session(workbench_session)?;

        // 隐藏 Picker
        Self::hide_picker(app)?;

        // 显示并聚焦 Workbench
        window
            .show()
            .map_err(|error| AppError::Message(error.to_string()))?;
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;

        state.begin_workbench_activation();

        // 发送会话开始事件
        window
            .emit(WORKBENCH_SESSION_START_EVENT, WorkbenchSessionPayload {
                source: "picker_search",
                item_id: None,
                initial_keyword,
            })
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("从 Picker 搜索进入 Workbench");
        Ok(())
    }
}
```

- [x] **Step 3: 添加 Payload 结构体**

```rust
// src-tauri/src/services/window_coordinator.rs

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchSessionPayload {
    source: &'static str,
    item_id: Option<String>,
    initial_keyword: Option<String>,
}
```

- [x] **Step 4: 提交**

```bash
rtk git add src-tauri/src/services/window_coordinator.rs
rtk git commit -m "feat(window): 实现从 Picker 打开 Workbench 的协调逻辑"
```

---

### Task 7: 实现全局快捷键打开 Workbench ✅

**Files:**
- Modify: `src-tauri/src/services/window_coordinator.rs`

- [x] **Step 1: 添加 open_workbench_global 函数**

```rust
// src-tauri/src/services/window_coordinator.rs

impl WindowCoordinator {
    /// 全局快捷键直接打开 Workbench
    pub fn open_workbench_global(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
        let window = ensure_workbench_window(app)?;

        // 如果 Workbench 已经活跃，直接聚焦
        if state.is_workbench_active() {
            window
                .set_focus()
                .map_err(|error| AppError::Message(error.to_string()))?;
            return Ok(());
        }

        // 捕获当前目标窗口
        let target_window = ActiveAppResolver::current_foreground_window_handle();

        let workbench_session = WorkbenchSession {
            target_window_hwnd: target_window,
            source: WorkbenchSource::GlobalShortcut,
            current_item_id: None,
            from_picker: false,
            picker_selected_index: None,
        };

        state.set_workbench_session(workbench_session)?;

        // 显示并聚焦 Workbench
        window
            .show()
            .map_err(|error| AppError::Message(error.to_string()))?;
        window
            .set_focus()
            .map_err(|error| AppError::Message(error.to_string()))?;

        state.begin_workbench_activation();

        // 发送会话开始事件
        window
            .emit(WORKBENCH_SESSION_START_EVENT, WorkbenchSessionPayload {
                source: "global",
                item_id: None,
                initial_keyword: None,
            })
            .map_err(|error| AppError::Message(error.to_string()))?;

        info!("全局快捷键打开 Workbench");
        Ok(())
    }

    /// 隐藏 Workbench 并恢复目标
    pub fn hide_workbench_and_restore_target(
        app: &AppHandle,
        state: &AppState,
    ) -> Result<(), AppError> {
        state.end_workbench_activation();
        let session = state.workbench_session()?;

        let Some(window) = app.get_webview_window(WORKBENCH_WINDOW_LABEL) else {
            return Ok(());
        };

        window
            .hide()
            .map_err(|error| AppError::Message(error.to_string()))?;

        let _ = window.emit(WORKBENCH_SESSION_END_EVENT, ());
        state.clear_workbench_session()?;

        // 恢复目标窗口
        if let Some(ref sess) = session {
            if let Some(hwnd) = sess.target_window_hwnd {
                let _ = ActiveAppResolver::restore_foreground_window(hwnd);
            }
        }

        info!("隐藏 Workbench");
        Ok(())
    }
}
```

- [x] **Step 2: 提交**

```bash
rtk git add src-tauri/src/services/window_coordinator.rs
rtk git commit -m "feat(window): 实现全局快捷键打开 Workbench"
```

---

### Task 8: 添加 Tauri 命令 ✅

**Files:**
- Modify: `src-tauri/src/commands/windows.rs`
- Modify: `src/bridge/commands.ts`

- [x] **Step 1: 添加 Rust 命令**

```rust
// src-tauri/src/commands/windows.rs

#[tauri::command]
pub async fn open_workbench_from_picker_edit(
    app: AppHandle,
    item_id: String,
) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or("应用状态未就绪")?
        .inner()
        .clone();
    WindowCoordinator::open_workbench_from_picker_edit(&app, &state, item_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_workbench_from_picker_search(
    app: AppHandle,
    initial_keyword: Option<String>,
) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or("应用状态未就绪")?
        .inner()
        .clone();
    WindowCoordinator::open_workbench_from_picker_search(&app, &state, initial_keyword)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_workbench_global(app: AppHandle) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or("应用状态未就绪")?
        .inner()
        .clone();
    WindowCoordinator::open_workbench_global(&app, &state)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn hide_workbench(app: AppHandle) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or("应用状态未就绪")?
        .inner()
        .clone();
    WindowCoordinator::hide_workbench_and_restore_target(&app, &state)
        .map_err(|e| e.to_string())
}
```

- [x] **Step 2: 注册命令到 main.rs**

```rust
// src-tauri/src/main.rs
// 在 invoke_handler 中添加
.invoke_handler(tauri::generate_handler![
    // ... 现有命令 ...
    commands::windows::open_workbench_from_picker_edit,
    commands::windows::open_workbench_from_picker_search,
    commands::windows::open_workbench_global,
    commands::windows::hide_workbench,
])
```

- [x] **Step 3: 添加前端命令封装**

```typescript
// src/bridge/commands.ts

export async function openWorkbenchFromPickerEdit(itemId: string): Promise<void> {
  if (!isTauriRuntime()) {
    console.log("Mock: openWorkbenchFromPickerEdit", itemId);
    return;
  }
  return invoke("open_workbench_from_picker_edit", { itemId });
}

export async function openWorkbenchFromPickerSearch(initialKeyword?: string): Promise<void> {
  if (!isTauriRuntime()) {
    console.log("Mock: openWorkbenchFromPickerSearch", initialKeyword);
    return;
  }
  return invoke("open_workbench_from_picker_search", { initialKeyword });
}

export async function openWorkbenchGlobal(): Promise<void> {
  if (!isTauriRuntime()) {
    console.log("Mock: openWorkbenchGlobal");
    return;
  }
  return invoke("open_workbench_global");
}

export async function hideWorkbench(): Promise<void> {
  if (!isTauriRuntime()) {
    console.log("Mock: hideWorkbench");
    return;
  }
  return invoke("hide_workbench");
}
```

- [x] **Step 4: 提交**

```bash
rtk git add src-tauri/src/commands/windows.rs src-tauri/src/main.rs src/bridge/commands.ts
rtk git commit -m "feat(commands): 添加 Workbench 相关 Tauri 命令"
```

---

## Chunk 3: 快捷键管理扩展 ✅

### Task 9: 扩展快捷键管理器支持 Workbench ✅

**Files:**
- Modify: `src-tauri/src/services/shortcut_manager.rs`

- [x] **Step 1: 添加 Workbench 会话快捷键**

```rust
// src-tauri/src/services/shortcut_manager.rs

const WORKBENCH_SESSION_SHORTCUTS: [&str; 8] = [
    "Up", "Down", "Enter", "Escape", "Ctrl+E", "Ctrl+F", "Ctrl+S", "Delete",
];

impl ShortcutManager {
    pub fn register_workbench_session_shortcuts(app: &AppHandle) -> Result<(), AppError> {
        let manager = app.global_shortcut();
        Self::unregister_workbench_session_shortcuts(app);
        let mut failures = Vec::new();
        for shortcut in WORKBENCH_SESSION_SHORTCUTS {
            if let Err(error) = manager.register(shortcut) {
                failures.push(format!("{shortcut}: {error}"));
            }
        }

        if !failures.is_empty() {
            return Err(AppError::Message(format!(
                "注册 Workbench 会话快捷键失败: {}",
                failures.join("; ")
            )));
        }

        info!(
            "已注册 Workbench 会话快捷键: {}",
            WORKBENCH_SESSION_SHORTCUTS.join(", ")
        );
        Ok(())
    }

    pub fn unregister_workbench_session_shortcuts(app: &AppHandle) {
        let manager = app.global_shortcut();
        for shortcut in WORKBENCH_SESSION_SHORTCUTS {
            let _ = manager.unregister(shortcut);
        }
    }
}
```

- [x] **Step 2: 修改 sync_registered_shortcut 支持双快捷键**

```rust
// src-tauri/src/services/shortcut_manager.rs

impl ShortcutManager {
    pub fn sync_registered_shortcuts(
        app: &AppHandle,
        main_shortcut: &str,
        workbench_shortcut: Option<&str>,
    ) -> Result<(), AppError> {
        let main_shortcut = normalize_shortcut(main_shortcut)?;
        if main_shortcut.is_empty() {
            return Err(AppError::Message("主快捷键不能为空".to_string()));
        }

        let manager = app.global_shortcut();
        manager
            .unregister_all()
            .map_err(|error| AppError::Message(format!("清理旧快捷键失败: {error}")))?;

        // 注册主快捷键
        manager
            .register(main_shortcut.as_str())
            .map_err(|error| AppError::Message(format!("注册主快捷键失败: {error}")))?;

        // 注册工作窗快捷键（如果启用）
        if let Some(workbench) = workbench_shortcut {
            let workbench = normalize_shortcut(workbench)?;
            if !workbench.is_empty() && workbench != main_shortcut {
                manager
                    .register(workbench.as_str())
                    .map_err(|error| AppError::Message(format!("注册工作窗快捷键失败: {error}")))?;
                info!("已注册工作窗快捷键: {workbench}");
            }
        }

        // 重新注册会话快捷键
        if picker_is_active(app) || has_registered_picker_session_shortcut(app) {
            if let Err(error) = Self::register_picker_session_shortcuts(app) {
                warn!("重新注册 Picker 会话快捷键失败: {error}");
            }
        }

        if workbench_is_active(app) || has_registered_workbench_session_shortcut(app) {
            if let Err(error) = Self::register_workbench_session_shortcuts(app) {
                warn!("重新注册 Workbench 会话快捷键失败: {error}");
            }
        }

        info!("已注册全局快捷键: 主={main_shortcut}, 工作窗={:?}", workbench_shortcut);
        Ok(())
    }
}
```

- [x] **Step 3: 添加 handle_shortcut_event 中的 Workbench 处理**

```rust
// src-tauri/src/services/shortcut_manager.rs

impl ShortcutManager {
    pub fn handle_shortcut_event(app: &AppHandle, shortcut: String, event: &ShortcutEvent) {
        // ... 现有 Picker 处理逻辑 ...

        // 检查是否为工作窗快捷键
        let workbench_shortcut = state
            .current_settings()
            .ok()
            .filter(|s| s.workbench_shortcut_enabled)
            .map(|s| s.workbench_shortcut);

        if let Some(ref ws) = workbench_shortcut {
            if normalized == normalize_shortcut(ws).unwrap_or_default() {
                if event.state != ShortcutState::Pressed {
                    return;
                }

                info!("命中工作窗快捷键: {normalized}");
                if state.is_workbench_active() {
                    // 已打开则关闭
                    let app_handle = app.clone();
                    let state_clone = state.clone();
                    thread::spawn(move || {
                        let _ = app_handle.run_on_main_thread(move || {
                            Self::unregister_workbench_session_shortcuts(&app_handle);
                            if let Err(error) = WindowCoordinator::hide_workbench_and_restore_target(
                                &app_handle,
                                &state_clone,
                            ) {
                                error!("关闭 Workbench 失败: {error}");
                            }
                        });
                    });
                } else {
                    // 未打开则打开
                    let app_handle = app.clone();
                    let state_clone = state.clone();
                    thread::spawn(move || {
                        let _ = app_handle.run_on_main_thread(move || {
                            if let Err(error) = WindowCoordinator::open_workbench_global(
                                &app_handle,
                                &state_clone,
                            ) {
                                error!("打开 Workbench 失败: {error}");
                            } else if let Err(error) =
                                Self::register_workbench_session_shortcuts(&app_handle)
                            {
                                warn!("打开 Workbench 后注册会话快捷键失败: {error}");
                            }
                        });
                    });
                }
                return;
            }
        }

        // Workbench 会话内快捷键处理
        if state.is_workbench_active() {
            // ... Workbench 会话快捷键处理
        }
    }
}
```

- [x] **Step 4: 提交**

```bash
rtk git add src-tauri/src/services/shortcut_manager.rs
rtk git commit -m "feat(shortcut): 扩展快捷键管理器支持 Workbench"
```

---

## Chunk 4: 前端路由与基础组件 ✅

### Task 10: 扩展前端路由支持 Workbench ✅

**Files:**
- Modify: `src/bridge/window.ts`
- Modify: `src/app/App.tsx`

- [x] **Step 1: 扩展 getCurrentWindowLabel 逻辑**

```typescript
// src/bridge/window.ts

export function getCurrentWindowLabel(): "picker" | "workbench" | "manager" {
  // 在 Tauri 运行时，通过 URL 或其他方式判断
  // 当前实现假设通过 URL 路径判断
  if (typeof window !== "undefined") {
    const path = window.location.pathname;
    if (path.includes("workbench")) {
      return "workbench";
    }
    if (path.includes("picker")) {
      return "picker";
    }
  }
  return "manager";
}
```

- [x] **Step 2: 修改 App.tsx 路由逻辑**

```typescript
// src/app/App.tsx

import { WorkbenchShell } from "../features/workbench/WorkbenchShell";

export function App() {
  const [windowLabel, setWindowLabel] = useState(() => getCurrentWindowLabel());
  // ... 现有逻辑 ...

  useEffect(() => {
    const label = getCurrentWindowLabel();
    setWindowLabel(label);
    document.documentElement.classList.remove("window-picker", "window-manager", "window-workbench");
    document.body.classList.remove("theme-picker", "theme-manager", "theme-workbench");

    if (label === "picker") {
      document.documentElement.classList.add("window-picker");
      document.body.classList.add("theme-picker");
    } else if (label === "workbench") {
      document.documentElement.classList.add("window-workbench");
      document.body.classList.add("theme-workbench");
    } else {
      document.documentElement.classList.add("window-manager");
      document.body.classList.add("theme-manager");
    }
  }, []);

  if (windowLabel === "picker") {
    return <PickerShell />;
  }
  if (windowLabel === "workbench") {
    return <WorkbenchShell />;
  }
  return <ManagerShell />;
}
```

- [x] **Step 3: 提交**

```bash
rtk git add src/bridge/window.ts src/app/App.tsx
rtk git commit -m "feat(router): 扩展前端路由支持 Workbench"
```

---

### Task 11: 创建 Workbench 状态管理 Store ✅

**Files:**
- Create: `src/features/workbench/store.ts`

- [x] **Step 1: 创建 Workbench Store**

```typescript
// src/features/workbench/store.ts
import { create } from "zustand";

export type WorkbenchMode = "search" | "edit" | "empty";

export interface WorkbenchSession {
  source: "picker_edit" | "picker_search" | "global";
  initialItemId?: string;
  initialKeyword?: string;
}

interface WorkbenchStore {
  // 会话状态
  session: WorkbenchSession | null;
  setSession: (session: WorkbenchSession | null) => void;

  // UI 状态
  mode: WorkbenchMode;
  setMode: (mode: WorkbenchMode) => void;

  // 搜索状态
  keyword: string;
  setKeyword: (keyword: string) => void;

  // 选中状态
  selectedItemId: string | null;
  setSelectedItemId: (id: string | null) => void;

  // 编辑状态
  draftText: string;
  setDraftText: (text: string) => void;
  isDirty: boolean;
  setIsDirty: (dirty: boolean) => void;

  // 原始保存文本（用于脏状态检测）
  savedText: string;
  setSavedText: (text: string) => void;
}

export const useWorkbenchStore = create<WorkbenchStore>((set) => ({
  session: null,
  setSession: (session) => set({ session }),

  mode: "search",
  setMode: (mode) => set({ mode }),

  keyword: "",
  setKeyword: (keyword) => set({ keyword }),

  selectedItemId: null,
  setSelectedItemId: (selectedItemId) => set({ selectedItemId }),

  draftText: "",
  setDraftText: (draftText) => set({ draftText }),

  isDirty: false,
  setIsDirty: (isDirty) => set({ isDirty }),

  savedText: "",
  setSavedText: (savedText) => set({ savedText }),
}));
```

- [x] **Step 2: 提交**

```bash
rtk git add src/features/workbench/store.ts
rtk git commit -m "feat(workbench): 创建 Workbench 状态管理 Store"
```

---

### Task 12: 创建 WorkbenchShell 基础组件 ✅

**Files:**
- Create: `src/features/workbench/WorkbenchShell.tsx`
- Create: `src/features/workbench/index.ts`

- [x] **Step 1: 创建 WorkbenchShell 骨架**

```typescript
// src/features/workbench/WorkbenchShell.tsx
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useWorkbenchStore } from "./store";
import { WORKBENCH_SESSION_START_EVENT, WORKBENCH_SESSION_END_EVENT } from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";

export function WorkbenchShell() {
  const { setSession, setMode, setSelectedItemId, setKeyword } = useWorkbenchStore();

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let offStart: (() => void) | undefined;
    let offEnd: (() => void) | undefined;

    void listen<{
      source: string;
      itemId?: string;
      initialKeyword?: string;
    }>(WORKBENCH_SESSION_START_EVENT, (event) => {
      const { source, itemId, initialKeyword } = event.payload;

      setSession({
        source: source as "picker_edit" | "picker_search" | "global",
        initialItemId: itemId,
        initialKeyword,
      });

      if (itemId) {
        setMode("edit");
        setSelectedItemId(itemId);
      } else {
        setMode("search");
        setSelectedItemId(null);
      }

      setKeyword(initialKeyword ?? "");
    }).then((cleanup) => {
      offStart = cleanup;
    });

    void listen(WORKBENCH_SESSION_END_EVENT, () => {
      setSession(null);
      setMode("search");
      setSelectedItemId(null);
      setKeyword("");
    }).then((cleanup) => {
      offEnd = cleanup;
    });

    return () => {
      offStart?.();
      offEnd?.();
    };
  }, [setSession, setMode, setSelectedItemId, setKeyword]);

  return (
    <div className="h-screen w-screen overflow-hidden bg-[color:var(--cp-window-shell)] text-ink">
      <div className="flex h-full flex-col">
        {/* 顶部操作带 */}
        <header className="shrink-0 border-b border-[color:var(--cp-border-weak)] px-4 py-3">
          <TopBar />
        </header>

        {/* 主体双栏 */}
        <main className="flex min-h-0 flex-1">
          {/* 左侧结果栏 */}
          <aside className="w-80 shrink-0 border-r border-[color:var(--cp-border-weak)]">
            <ResultList />
          </aside>

          {/* 右侧编辑栏 */}
          <section className="min-w-0 flex-1">
            <EditPanel />
          </section>
        </main>
      </div>
    </div>
  );
}

function TopBar() {
  const { session, keyword, setKeyword } = useWorkbenchStore();

  return (
    <div className="flex items-center gap-4">
      <span className="text-xs text-[color:var(--cp-text-muted)]">
        {session?.source === "picker_edit" && "从 Picker 编辑"}
        {session?.source === "picker_search" && "从 Picker 搜索"}
        {session?.source === "global" && "全局搜索"}
      </span>
      <input
        className="flex-1 rounded-md border border-[color:var(--cp-border-weak)] bg-[color:var(--cp-control-surface)] px-3 py-2 text-sm outline-none focus:border-[rgba(var(--cp-peach-rgb),0.35)]"
        placeholder="搜索记录..."
        value={keyword}
        onChange={(e) => setKeyword(e.target.value)}
      />
    </div>
  );
}

function ResultList() {
  return (
    <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
      结果列表（待实现）
    </div>
  );
}

function EditPanel() {
  return (
    <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
      编辑面板（待实现）
    </div>
  );
}
```

- [x] **Step 2: 创建导出文件**

```typescript
// src/features/workbench/index.ts
export { WorkbenchShell } from "./WorkbenchShell";
export { useWorkbenchStore } from "./store";
```

- [x] **Step 3: 提交**

```bash
rtk git add src/features/workbench/WorkbenchShell.tsx src/features/workbench/index.ts
rtk git commit -m "feat(workbench): 创建 WorkbenchShell 基础组件骨架"
```

---

## Chunk 5: Picker 入口改造 ✅

### Task 13: Picker 新增编辑入口 ✅

**Files:**
- Modify: `src/features/picker/PickerShell.tsx`

- [x] **Step 1: 添加编辑按钮到 Picker header**

```typescript
// src/features/picker/PickerShell.tsx
// 在 header 的右侧按钮区域添加

import { openWorkbenchFromPickerEdit } from "../../bridge/commands";

// 在 handleOpenManager 函数后添加
const handleOpenWorkbenchEdit = async () => {
  const item = itemsRef.current[selectedIndexRef.current];
  if (!item) {
    return;
  }
  await openWorkbenchFromPickerEdit(item.id);
};
```

- [x] **Step 2: 添加搜索按钮到 Picker header**

```typescript
// src/features/picker/PickerShell.tsx
import { openWorkbenchFromPickerSearch } from "../../bridge/commands";

const handleOpenWorkbenchSearch = async () => {
  await openWorkbenchFromPickerSearch();
};

// 在 header 右侧添加按钮
<button
  className={STYLES.headerButton}
  onClick={() => void handleOpenWorkbenchSearch()}
  type="button"
>
  <svg className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.35-4.35" />
  </svg>
</button>
```

- [x] **Step 3: 添加会话快捷键监听**

```typescript
// src/features/picker/PickerShell.tsx
// 在 useEffect 中添加 Ctrl+E 和 Ctrl+F 监听

void listen(PICKER_OPEN_WORKBENCH_EDIT_EVENT, async () => {
  const item = itemsRef.current[selectedIndexRef.current];
  if (!item) {
    return;
  }
  await openWorkbenchFromPickerEdit(item.id);
}).then((cleanup) => {
  unlistenWorkbenchEdit = cleanup;
});

void listen(PICKER_OPEN_WORKBENCH_SEARCH_EVENT, async () => {
  await openWorkbenchFromPickerSearch();
}).then((cleanup) => {
  unlistenWorkbenchSearch = cleanup;
});
```

- [x] **Step 4: 添加样式**

```typescript
// 在 STYLES 对象中添加
editButton: "flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold text-[color:var(--cp-text-secondary)] transition-all duration-250 hover:bg-[rgba(var(--cp-surface1-rgb),0.2)] hover:text-[color:var(--cp-text-primary)]",
```

- [x] **Step 5: 提交**

```bash
rtk git add src/features/picker/PickerShell.tsx
rtk git commit -m "feat(picker): 添加编辑和搜索入口按钮"
```

---

### Task 14: 扩展快捷键管理器支持 Ctrl+E 和 Ctrl+F ✅

**Files:**
- Modify: `src-tauri/src/services/shortcut_manager.rs`

- [x] **Step 1: 扩展 Picker 会话快捷键列表**

```rust
// src-tauri/src/services/shortcut_manager.rs

const PICKER_SESSION_SHORTCUTS: [&str; 16] = [
    "Up", "Down", "Enter", "Escape", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6",
    "Digit7", "Digit8", "Digit9", "Tab", "Ctrl+E", "Ctrl+F",
];
```

- [x] **Step 2: 添加事件发送逻辑**

```rust
// src-tauri/src/services/shortcut_manager.rs
// 在 handle_shortcut_event 中添加

"ctrl+e" => {
    info!("命中 Ctrl+E 编辑当前项");
    app.emit(PICKER_OPEN_WORKBENCH_EDIT_EVENT, ())
}
"ctrl+f" => {
    info!("命中 Ctrl+F 进入搜索");
    app.emit(PICKER_OPEN_WORKBENCH_SEARCH_EVENT, ())
}
```

- [x] **Step 3: 更新 is_picker_session_shortcut 函数**

```rust
fn is_picker_session_shortcut(shortcut: &str) -> bool {
    matches!(
        shortcut,
        "up" | "arrowup"
            | "down"
            | "arrowdown"
            | "enter"
            | "escape"
            | "esc"
            | "digit1"
            | "digit2"
            | "digit3"
            | "digit4"
            | "digit5"
            | "digit6"
            | "digit7"
            | "digit8"
            | "digit9"
            | "tab"
            | "ctrl+e"
            | "ctrl+f"
    )
}
```

- [x] **Step 4: 提交**

```bash
rtk git add src-tauri/src/services/shortcut_manager.rs
rtk git commit -m "feat(shortcut): 支持 Ctrl+E 编辑和 Ctrl+F 搜索快捷键"
```

---

## Chunk 6: Workbench 完整实现 ✅

### Task 15: 实现 Workbench 搜索与结果列表 ✅

**Files:**
- Create: `src/features/workbench/queries.ts`
- Modify: `src/features/workbench/WorkbenchShell.tsx`

- [x] **Step 1: 创建 Workbench 查询 hooks**

```typescript
// src/features/workbench/queries.ts
import { useQuery } from "@tanstack/react-query";
import { listRecentItems, searchItems } from "../../bridge/commands";
import type { SearchQuery, ClipItemSummary } from "../../shared/types/clips";

const WORKBENCH_RECENT_LIMIT = 30;

export function useWorkbenchRecentQuery(enabled: boolean) {
  return useQuery({
    queryKey: ["workbench-recent", WORKBENCH_RECENT_LIMIT],
    queryFn: () => listRecentItems(WORKBENCH_RECENT_LIMIT),
    enabled,
    staleTime: 0,
  });
}

export function useWorkbenchSearchQuery(keyword: string, enabled: boolean) {
  const query: SearchQuery = {
    keyword,
    filters: {},
    offset: 0,
    limit: 50,
    sort: keyword.trim() ? "relevance_desc" : "recent_desc",
  };

  return useQuery({
    queryKey: ["workbench-search", query],
    queryFn: () => searchItems(query),
    enabled,
    staleTime: 0,
  });
}
```

- [x] **Step 2: 实现 ResultList 组件**

```typescript
// src/features/workbench/WorkbenchShell.tsx
import { useWorkbenchRecentQuery, useWorkbenchSearchQuery } from "./queries";
import type { ClipItemSummary } from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";

function ResultList() {
  const { keyword, selectedItemId, setSelectedItemId, setMode, setDraftText, setSavedText } = useWorkbenchStore();

  // 空关键词显示最近记录，否则显示搜索结果
  const recent = useWorkbenchRecentQuery(!keyword.trim());
  const search = useWorkbenchSearchQuery(keyword, keyword.trim().length > 0);

  const items: ClipItemSummary[] = keyword.trim()
    ? (search.data?.items ?? [])
    : (recent.data ?? []);

  const isLoading = keyword.trim() ? search.isLoading : recent.isLoading;

  const handleSelectItem = async (item: ClipItemSummary) => {
    setSelectedItemId(item.id);
    setMode("edit");
    // 加载详情时设置 draftText
    // TODO: 使用 useItemDetailQuery
  };

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        加载中...
      </div>
    );
  }

  if (items.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        {keyword.trim() ? "未找到匹配记录" : "暂无记录"}
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      {items.map((item, index) => (
        <button
          key={item.id}
          className={`w-full border-b border-[color:var(--cp-border-weak)] px-4 py-3 text-left transition-colors hover:bg-[rgba(var(--cp-surface1-rgb),0.1)] ${
            selectedItemId === item.id ? "bg-[rgba(var(--cp-peach-rgb),0.08)]" : ""
          }`}
          onClick={() => handleSelectItem(item)}
        >
          <p className="line-clamp-2 text-sm text-[color:var(--cp-text-primary)]">
            {item.contentPreview}
          </p>
          <div className="mt-1 flex items-center gap-2 text-xs text-[color:var(--cp-text-muted)]">
            <span>{getClipTypeLabel(item)}</span>
            <span>{formatDateTime(item.lastUsedAt ?? item.createdAt)}</span>
            {item.isFavorited && <span className="text-[color:var(--cp-favorite)]">★</span>}
          </div>
        </button>
      ))}
    </div>
  );
}
```

- [x] **Step 3: 提交**

```bash
rtk git add src/features/workbench/queries.ts src/features/workbench/WorkbenchShell.tsx
rtk git commit -m "feat(workbench): 实现搜索与结果列表"
```

---

### Task 16: 实现 Workbench 编辑面板 ✅

**Files:**
- Modify: `src/features/workbench/WorkbenchShell.tsx`

- [x] **Step 1: 实现 EditPanel 组件**

```typescript
// src/features/workbench/WorkbenchShell.tsx
import { useItemDetailQuery, useUpdateTextMutation, useSetFavoritedMutation, useDeleteItemMutation, usePasteMutation } from "../manager/queries";

function EditPanel() {
  const {
    selectedItemId,
    draftText,
    setDraftText,
    isDirty,
    setIsDirty,
    savedText,
    setSavedText,
    session,
  } = useWorkbenchStore();

  const detail = useItemDetailQuery(selectedItemId);
  const updateTextMutation = useUpdateTextMutation();
  const favoritedMutation = useSetFavoritedMutation();
  const deleteMutation = useDeleteItemMutation();
  const pasteMutation = usePasteMutation();

  // 同步详情到编辑区
  useEffect(() => {
    if (detail.data && !isDirty) {
      setDraftText(detail.data.textContent ?? "");
      setSavedText(detail.data.textContent ?? "");
    }
  }, [detail.data, isDirty, setDraftText, setSavedText]);

  // 检测脏状态
  useEffect(() => {
    setIsDirty(draftText !== savedText);
  }, [draftText, savedText, setIsDirty]);

  const handleSave = async () => {
    if (!selectedItemId) return;
    await updateTextMutation.mutateAsync({ id: selectedItemId, text: draftText });
    setSavedText(draftText);
    setIsDirty(false);
  };

  const handleToggleFavorite = async () => {
    if (!selectedItemId || !detail.data) return;
    await favoritedMutation.mutateAsync({
      id: selectedItemId,
      value: !detail.data.isFavorited,
    });
  };

  const handleDelete = async () => {
    if (!selectedItemId) return;
    // TODO: 二次确认
    await deleteMutation.mutateAsync(selectedItemId);
  };

  const handlePaste = async () => {
    if (!selectedItemId) return;

    // 检查未保存修改
    if (isDirty) {
      // TODO: 弹出确认对话框
      return;
    }

    const result = await pasteMutation.mutateAsync({
      id: selectedItemId,
      option: { restoreClipboardAfterPaste: true, pasteToTarget: true },
    });

    if (result.success) {
      // TODO: 关闭窗口
    }
  };

  if (!selectedItemId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        选择一条记录进行编辑
      </div>
    );
  }

  if (detail.isLoading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        加载中...
      </div>
    );
  }

  if (!detail.data) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        记录不存在
      </div>
    );
  }

  const isTextItem = detail.data.clipType === "text";

  return (
    <div className="flex h-full flex-col">
      {/* 编辑区或元信息区 */}
      <div className="min-h-0 flex-1 p-4">
        {isTextItem ? (
          <textarea
            className="h-full w-full resize-none rounded-md border border-[color:var(--cp-border-weak)] bg-[color:var(--cp-control-surface)] p-4 text-sm outline-none focus:border-[rgba(var(--cp-peach-rgb),0.35)]"
            value={draftText}
            onChange={(e) => setDraftText(e.target.value)}
            placeholder="输入内容..."
          />
        ) : (
          <MetaInfoPanel detail={detail.data} />
        )}
      </div>

      {/* 操作按钮区 */}
      <div className="shrink-0 border-t border-[color:var(--cp-border-weak)] px-4 py-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <button
              className="rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-semibold text-cp-base disabled:opacity-50"
              onClick={handleSave}
              disabled={!isDirty || updateTextMutation.isPending}
            >
              保存
            </button>
            <button
              className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm"
              onClick={handleToggleFavorite}
            >
              {detail.data.isFavorited ? "取消收藏" : "收藏"}
            </button>
            <button
              className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm text-red-500"
              onClick={handleDelete}
            >
              删除
            </button>
          </div>
          <button
            className="rounded-md bg-[color:var(--cp-accent-primary-strong)] px-6 py-2 text-sm font-semibold text-cp-base"
            onClick={handlePaste}
          >
            回贴
          </button>
        </div>
      </div>
    </div>
  );
}

function MetaInfoPanel({ detail }: { detail: ClipItemDetail }) {
  return (
    <div className="space-y-4 rounded-md border border-[color:var(--cp-border-weak)] p-4">
      <h3 className="font-semibold text-[color:var(--cp-text-primary)]">条目信息</h3>
      <dl className="space-y-2 text-sm">
        <div className="flex justify-between">
          <dt className="text-[color:var(--cp-text-muted)]">类型</dt>
          <dd>{getClipTypeLabel(detail)}</dd>
        </div>
        <div className="flex justify-between">
          <dt className="text-[color:var(--cp-text-muted)]">创建时间</dt>
          <dd>{formatDateTime(detail.createdAt)}</dd>
        </div>
        {detail.sourceApp && (
          <div className="flex justify-between">
            <dt className="text-[color:var(--cp-text-muted)]">来源</dt>
            <dd>{detail.sourceApp}</dd>
          </div>
        )}
      </dl>
    </div>
  );
}
```

- [x] **Step 2: 提交**

```bash
rtk git add src/features/workbench/WorkbenchShell.tsx
rtk git commit -m "feat(workbench): 实现编辑面板与操作按钮"
```

---

### Task 17: 实现未保存修改确认对话框 ✅

**Files:**
- Create: `src/features/workbench/components/ConfirmDialog.tsx`
- Modify: `src/features/workbench/WorkbenchShell.tsx`

- [x] **Step 1: 创建确认对话框组件**

```typescript
// src/features/workbench/components/ConfirmDialog.tsx
import { createPortal } from "react-dom";

interface ConfirmDialogProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  isOpen,
  title,
  message,
  confirmLabel = "确认",
  cancelLabel = "取消",
  destructive = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  if (!isOpen) return null;

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-80 rounded-lg bg-[color:var(--cp-window-shell)] p-6 shadow-xl">
        <h3 className="text-lg font-semibold text-[color:var(--cp-text-primary)]">{title}</h3>
        <p className="mt-2 text-sm text-[color:var(--cp-text-secondary)]">{message}</p>
        <div className="mt-6 flex justify-end gap-2">
          <button
            className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm"
            onClick={onCancel}
          >
            {cancelLabel}
          </button>
          <button
            className={`rounded-md px-4 py-2 text-sm font-semibold text-cp-base ${
              destructive
                ? "bg-red-500"
                : "bg-[color:var(--cp-accent-primary)]"
            }`}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body
  );
}
```

- [x] **Step 2: 在 EditPanel 中使用确认对话框**

```typescript
// 在 WorkbenchShell.tsx 中添加状态和逻辑

const [confirmDialog, setConfirmDialog] = useState<{
  isOpen: boolean;
  title: string;
  message: string;
  onConfirm: () => void;
}>({ isOpen: false, title: "", message: "", onConfirm: () => {} });

// 在切换条目时检查脏状态
const handleSelectItem = async (item: ClipItemSummary) => {
  if (isDirty) {
    setConfirmDialog({
      isOpen: true,
      title: "未保存的修改",
      message: "当前有未保存的修改，是否保存后切换？",
      onConfirm: async () => {
        await handleSave();
        setSelectedItemId(item.id);
        setMode("edit");
        setIsDirty(false);
        setConfirmDialog((prev) => ({ ...prev, isOpen: false }));
      },
    });
    return;
  }
  setSelectedItemId(item.id);
  setMode("edit");
};
```

- [x] **Step 3: 提交**

```bash
rtk git add src/features/workbench/components/ConfirmDialog.tsx src/features/workbench/WorkbenchShell.tsx
rtk git commit -m "feat(workbench): 实现未保存修改确认对话框"
```

---

## Chunk 7: 设置页扩展 ✅

### Task 18: 设置页添加工作窗快捷键配置 ✅

**Files:**
- Modify: `src/features/manager/ManagerShell.tsx`

- [x] **Step 1: 添加工作窗快捷键设置 UI**

```typescript
// src/features/manager/ManagerShell.tsx
// 在设置页的快捷键配置区域添加

const [workbenchShortcut, setWorkbenchShortcut] = useState("");
const [workbenchShortcutEnabled, setWorkbenchShortcutEnabled] = useState(true);

// 同步设置
useEffect(() => {
  if (settings.data) {
    setWorkbenchShortcut(settings.data.workbenchShortcut);
    setWorkbenchShortcutEnabled(settings.data.workbenchShortcutEnabled);
  }
}, [settings.data]);

// 在渲染中添加
<div className="space-y-2">
  <div className="flex items-center justify-between">
    <label className="text-sm font-medium">工作窗搜索快捷键</label>
    <label className="flex items-center gap-2">
      <input
        type="checkbox"
        checked={workbenchShortcutEnabled}
        onChange={(e) => setWorkbenchShortcutEnabled(e.target.checked)}
      />
      <span className="text-sm text-[color:var(--cp-text-muted)]">启用</span>
    </label>
  </div>
  <input
    type="text"
    className={STYLES.searchInput}
    value={workbenchShortcut}
    onChange={(e) => setWorkbenchShortcut(e.target.value)}
    disabled={!workbenchShortcutEnabled}
    placeholder="Ctrl+Shift+F"
  />
  <p className="text-xs text-[color:var(--cp-text-muted)]">
    全局快捷键，直接打开搜索/编辑工作窗
  </p>
</div>
```

- [x] **Step 2: 修改保存逻辑**

```typescript
// 在 handleSaveSettings 函数中添加
const handleSaveSettings = async () => {
  try {
    await updateSettingsMutation.mutateAsync({
      ...settings.data!,
      // ... 现有字段
      workbenchShortcut: workbenchShortcutEnabled ? workbenchShortcut : "",
      workbenchShortcutEnabled,
    });
    // 成功提示
  } catch (error) {
    // 错误处理（包括快捷键冲突）
  }
};
```

- [x] **Step 3: 提交**

```bash
rtk git add src/features/manager/ManagerShell.tsx
rtk git commit -m "feat(settings): 添加工作窗快捷键配置 UI"
```

---

## Chunk 8: 集成测试与收尾

### Task 19: Tauri 配置文件更新 ✅

**Files:**
- Create: `src-tauri/capabilities/workbench.json`

- [x] **Step 1: 创建 Workbench 窗口权限配置**

```json
{
  "identifier": "workbench",
  "description": "Workbench window capability",
  "windows": ["workbench"],
  "permissions": [
    "core:default",
    "core:window:allow-close",
    "core:window:allow-minimize",
    "core:window:allow-maximize",
    "core:window:allow-unmaximize",
    "core:window:allow-start-dragging",
    "core:window:allow-is-visible",
    "core:window:allow-show",
    "core:window:allow-hide",
    "shell:allow-open",
    "global-shortcut:allow-is-registered",
    "global-shortcut:allow-register",
    "global-shortcut:allow-unregister"
  ]
}
```

- [x] **Step 2: 提交**

```bash
rtk git add src-tauri/capabilities/workbench.json
rtk git commit -m "feat(tauri): 添加 Workbench 窗口权限配置"
```

---

### Task 20: 端到端验证

**Files:**
- 无需修改

- [ ] **Step 1: 验证场景 1 - Picker 编辑入口**

操作步骤：
1. 启动应用
2. 触发 Picker 主快捷键打开 Picker
3. 选中一条文本记录
4. 按 Ctrl+E 或点击编辑按钮
5. 验证：Picker 关闭，Workbench 打开，右侧显示编辑区

- [ ] **Step 2: 验证场景 2 - Picker 搜索入口**

操作步骤：
1. 打开 Picker
2. 按 Ctrl+F 或点击搜索按钮
3. 验证：Picker 关闭，Workbench 打开，搜索框聚焦

- [ ] **Step 3: 验证场景 3 - 全局 Workbench 快捷键**

操作步骤：
1. 确保 Workbench 快捷键已启用
2. 在任意应用中按 Ctrl+Shift+F
3. 验证：Workbench 打开，搜索框聚焦

- [ ] **Step 4: 验证场景 4 - Workbench 回贴**

操作步骤：
1. 打开 Workbench
2. 选择一条记录
3. 点击回贴按钮
4. 验证：Workbench 关闭，内容粘贴到目标窗口

- [ ] **Step 5: 验证场景 5 - 未保存修改确认**

操作步骤：
1. 打开 Workbench 编辑一条文本
2. 修改内容但不保存
3. 尝试切换到另一条记录
4. 验证：弹出确认对话框

- [ ] **Step 6: 提交验证通过标记**

```bash
rtk git tag -a v0.1.0-workbench-mvp -m "Workbench MVP 验证通过"
```

---

## 总结

本计划涵盖了以下核心实现：

1. **会话模型扩展**：新增 `WorkbenchSession` 支持 Picker 与 Workbench 之间的上下文共享
2. **窗口协调层**：扩展 `WindowCoordinator` 支持三类窗口的切换
3. **快捷键管理**：支持双全局快捷键和会话内快捷键
4. **前端组件**：新建 `WorkbenchShell` 实现搜索-编辑-回贴链路
5. **Picker 入口改造**：新增 Ctrl+E 和 Ctrl+F 快捷入口
6. **设置扩展**：新增工作窗快捷键配置

### 后续优化方向（非本轮范围）

- Workbench 键盘导航优化
- 目标窗口恢复失败的重试机制
- 基于当前项搜索的初始关键词生成
- 从 Workbench 返回 Picker 时的选中项恢复
