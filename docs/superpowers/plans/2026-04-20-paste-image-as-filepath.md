# 图片条目粘贴为文件路径 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 支持在 Picker 和 Search 窗口中，对图片条目通过 Shift+Enter 将其文件路径作为纯文本写入剪贴板并粘贴到目标窗口；Search 窗口为图片条目增加一个操作按钮触发相同行为。

**Architecture:** 在现有 `paste_item` 管道中扩展 `PasteOption` 增加 `as_file` 标志。后端在 `write_item_to_clipboard` 中检测该标志，对图片条目改为将绝对文件路径字符串作为文本写入剪贴板。前端在 Picker 和 Search 的键盘处理器中检测 Shift+Enter，并通过新的 Tauri 事件 `picker://confirm-as-file` 协调 Picker 的全局快捷键场景。

**Tech Stack:** Rust (Tauri commands, arboard clipboard), TypeScript (React, Tauri events)

---

### Task 1: 后端 — 扩展 PasteOption 和 write_item_to_clipboard

**Files:**
- Modify: `src-tauri/src/domain/clip_item.rs:150-160` (PasteOption struct)
- Modify: `src-tauri/src/services/paste_executor.rs:316-394` (write_item_to_clipboard)

- [ ] **Step 1: 在 PasteOption 中新增 as_file 字段**

Edit `src-tauri/src/domain/clip_item.rs` — 在 `PasteOption` 结构体的 `paste_to_target` 字段后新增：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteOption {
    pub restore_clipboard_after_paste: bool,
    #[serde(default = "default_true")]
    pub paste_to_target: bool,
    #[serde(default)]
    pub as_file: bool,
}
```

- [ ] **Step 2: 修改 write_item_to_clipboard 增加 as_file 参数和分支**

Edit `src-tauri/src/services/paste_executor.rs` — 修改 `write_item_to_clipboard` 函数签名，新增 `as_file` 参数：

```rust
fn write_item_to_clipboard(
    app: &AppHandle,
    state: &AppState,
    clipboard: &mut Clipboard,
    detail: &ClipItemDetail,
    as_file: bool,
) -> Result<(), AppError> {
    match detail.r#type.as_str() {
        "text" => {
            if let Some(normalized) = NormalizeService::normalize_text(&detail.full_text, None) {
                state
                    .self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }

            clipboard
                .set_text(detail.full_text.clone())
                .map_err(|error| AppError::Clipboard(error.to_string()))
        }
        "image" => {
            let Some(image_path) = detail.image_path.as_deref() else {
                return Err(AppError::Message(
                    "图片记录缺少可恢复的文件引用".to_string(),
                ));
            };

            // as_file 模式：将绝对文件路径作为纯文本写入剪贴板
            if as_file {
                let absolute_path = state.image_storage.resolve_existing_image_path(image_path)?;
                let path_str = absolute_path.to_string_lossy().to_string();

                if let Some(normalized) = NormalizeService::normalize_text(&path_str, None) {
                    state
                        .self_write_guard()
                        .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
                }

                clipboard
                    .set_text(path_str)
                    .map_err(|error| AppError::Clipboard(error.to_string()))
            } else {
                let image = state.image_storage.load_image(image_path)?;
                if let Some(owner_window) = resolve_clipboard_owner_window(app) {
                    let crate::services::image_storage::DecodedImage {
                        rgba,
                        width,
                        height,
                        png_bytes,
                    } = image;
                    write_image_to_clipboard(
                        owner_window,
                        &ClipboardImageData {
                            rgba,
                            width,
                            height,
                            png_bytes: Some(png_bytes),
                        },
                    )?;
                } else {
                    clipboard
                        .set_image(ImageData {
                            width: image.width,
                            height: image.height,
                            bytes: Cow::Owned(image.rgba),
                        })
                        .map_err(|error| AppError::Clipboard(error.to_string()))?;
                }

                state
                    .self_write_guard()
                    .suppress_hash(detail.hash.clone(), Duration::from_secs(3))?;

                Ok(())
            }
        }
        "file" => {
            if detail.file_paths.is_empty() {
                return Err(AppError::Message("文件记录缺少文件路径".to_string()));
            }

            if let Some(normalized) = NormalizeService::normalize_files(
                detail.file_paths.clone(),
                detail.directory_count,
                detail.total_size,
                None,
            ) {
                state
                    .self_write_guard()
                    .suppress_hash(normalized.normalized.hash, Duration::from_secs(3))?;
            }

            write_file_paths_to_clipboard(&detail.file_paths)
        }
        other => Err(AppError::Message(format!("暂不支持 {other} 类型的写回"))),
    }
}
```

- [ ] **Step 3: 更新 PasteExecutor::paste_item 中的调用点**

Edit `src-tauri/src/services/paste_executor.rs` — 在 `paste_item` 方法中，将 `as_file` 传给 `write_item_to_clipboard`：

将第 49 行：
```rust
write_item_to_clipboard(app, state, &mut clipboard, &detail)?;
```

改为：
```rust
write_item_to_clipboard(app, state, &mut clipboard, &detail, option.as_file)?;
```

- [ ] **Step 4: 运行 Rust 测试确认编译通过**

Run: `rtk cargo test` (在 `src-tauri/` 目录下)
Expected: 所有测试通过，无编译错误

- [ ] **Step 5: Commit**

```bash
rtk git add src-tauri/src/domain/clip_item.rs src-tauri/src/services/paste_executor.rs
rtk git commit -m "feat: PasteOption 增加 as_file 字段，支持图片路径写入剪贴板"
```

---

### Task 2: 后端 — 注册 Shift+Enter 快捷键和新事件

Picker 在 Tauri 运行时通过全局快捷键处理键盘输入。需要注册 `Shift+Enter` 并发射新事件。

**Files:**
- Modify: `src-tauri/src/domain/events.rs` (新增事件常量)
- Modify: `src-tauri/src/services/shortcut_manager.rs:30-46,255-256` (快捷键注册和回调)

- [ ] **Step 1: 新增 PICKER_CONFIRM_AS_FILE_EVENT 常量**

Edit `src-tauri/src/domain/events.rs` — 在 `PICKER_CONFIRM_EVENT` 行后新增：

```rust
pub const PICKER_CONFIRM_EVENT: &str = "picker://confirm";
pub const PICKER_CONFIRM_AS_FILE_EVENT: &str = "picker://confirm-as-file";
```

- [ ] **Step 2: 在 shortcut_manager 中注册 Shift+Enter**

Edit `src-tauri/src/services/shortcut_manager.rs` — 在 `PICKER_SESSION_SHORTCUTS` 数组中添加 `"Shift+Enter"`（在 `"Enter"` 之后）：

```rust
const PICKER_SESSION_SHORTCUTS: [&str; 16] = [
    "Up",
    "Down",
    "Enter",
    "Shift+Enter",
    "Escape",
    "Ctrl+Space",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    "Ctrl+Enter",
];
```

- [ ] **Step 3: 在快捷键回调中处理 shift+enter**

Edit `src-tauri/src/services/shortcut_manager.rs` — 在 imports 中添加新事件常量：

```rust
use crate::domain::events::{
    PICKER_CONFIRM_EVENT, PICKER_CONFIRM_AS_FILE_EVENT, PICKER_FAVORITE_EVENT,
    PICKER_NAVIGATE_EVENT, PICKER_OPEN_EDITOR_EVENT, PICKER_SELECT_INDEX_EVENT,
};
```

在快捷键回调的 match 分支中，在 `"enter"` 分支之后添加：

```rust
"enter" => app.emit(PICKER_CONFIRM_EVENT, ()),
"shift+enter" => app.emit(PICKER_CONFIRM_AS_FILE_EVENT, ()),
```

- [ ] **Step 4: 运行 Rust 测试确认编译通过**

Run: `rtk cargo test` (在 `src-tauri/` 目录下)
Expected: 所有测试通过

- [ ] **Step 5: Commit**

```bash
rtk git add src-tauri/src/domain/events.rs src-tauri/src/services/shortcut_manager.rs
rtk git commit -m "feat: 注册 Shift+Enter 快捷键，新增 picker://confirm-as-file 事件"
```

---

### Task 3: 前端 — 类型与 Bridge 层扩展

**Files:**
- Modify: `src/shared/types/clips.ts:73-76` (PasteOption type)
- Modify: `src/bridge/events.ts` (新增前端事件常量)

- [ ] **Step 1: 在 TypeScript PasteOption 中新增 asFile 字段**

Edit `src/shared/types/clips.ts` — 在 `PasteOption` 接口中新增：

```typescript
export interface PasteOption {
  restoreClipboardAfterPaste: boolean;
  pasteToTarget?: boolean;
  asFile?: boolean;
}
```

- [ ] **Step 2: 在前端 events.ts 中新增事件常量**

Edit `src/bridge/events.ts` — 在 `PICKER_CONFIRM_EVENT` 行后新增：

```typescript
export const PICKER_CONFIRM_EVENT = "picker://confirm";
export const PICKER_CONFIRM_AS_FILE_EVENT = "picker://confirm-as-file";
```

- [ ] **Step 3: 运行 TypeScript 构建检查**

Run: `rtk pnpm build`
Expected: 构建通过，无类型错误

- [ ] **Step 4: Commit**

```bash
rtk git add src/shared/types/clips.ts src/bridge/events.ts
rtk git commit -m "feat: 前端 PasteOption 新增 asFile 字段和 confirm-as-file 事件"
```

---

### Task 4: 前端 — Picker 窗口支持 Shift+Enter

Picker 有两个键盘处理路径：Tauri 运行时通过全局快捷键事件，浏览器预览通过 keydown 事件。

**Files:**
- Modify: `src/features/picker/PickerShell.tsx:164-178` (confirmSelection 函数)
- Modify: `src/features/picker/PickerShell.tsx:346-353` (Tauri 事件监听)
- Modify: `src/features/picker/PickerShell.tsx:451-455` (浏览器 keydown 处理)

- [ ] **Step 1: 修改 confirmSelection 函数接受 asFile 参数**

Edit `src/features/picker/PickerShell.tsx` — 将 `confirmSelection` 改为：

```typescript
const confirmSelection = async (index: number, asFile = false) => {
    const item = itemsRef.current[index];
    if (!item) {
      return;
    }

    cancelTooltip();

    await pasteItem(item.id, {
      restoreClipboardAfterPaste: restoreClipboardRef.current,
      pasteToTarget: true,
      ...(asFile && item.type === "image" ? { asFile: true } : {}),
    });
    // picker 紧接着会被隐藏，不清空的话下次打开时会闪过旧消息
    setLastMessage("");
  };
```

- [ ] **Step 2: 添加 PICKER_CONFIRM_AS_FILE_EVENT 导入和监听**

Edit `src/features/picker/PickerShell.tsx` — 在导入事件的位置添加 `PICKER_CONFIRM_AS_FILE_EVENT`：

```typescript
import {
  CLIPS_CHANGED_EVENT,
  PICKER_CONFIRM_AS_FILE_EVENT,
  PICKER_CONFIRM_EVENT,
  // ... 其余不变
} from "../../bridge/events";
```

在 Tauri 事件监听的 `useEffect` 中（约第 346 行 `PICKER_CONFIRM_EVENT` 监听之后），添加：

```typescript
void listen(PICKER_CONFIRM_AS_FILE_EVENT, async () => {
    if (disposed) {
      return;
    }
    await confirmSelection(selectedIndexRef.current, true);
}).then((cleanup) => {
    unlistenConfirmAsFile = cleanup;
});
```

同时在 `useEffect` 顶部声明 `let unlistenConfirmAsFile: (() => void) | undefined;`，在 cleanup 中调用 `unlistenConfirmAsFile?.()`。

- [ ] **Step 3: 在浏览器 keydown 处理中检测 Shift+Enter**

Edit `src/features/picker/PickerShell.tsx` — 在浏览器 keydown 处理器（约第 451 行），修改 Enter 处理：

```typescript
if (event.key === "Enter") {
    event.preventDefault();
    const item = itemsRef.current[selectedIndexRef.current];
    const asFile = event.shiftKey && item?.type === "image";
    void confirmSelection(selectedIndexRef.current, asFile);
    return;
}
```

- [ ] **Step 4: 运行 TypeScript 构建检查**

Run: `rtk pnpm build`
Expected: 构建通过

- [ ] **Step 5: Commit**

```bash
rtk git add src/features/picker/PickerShell.tsx
rtk git commit -m "feat: Picker 窗口支持 Shift+Enter 将图片粘贴为文件路径"
```

---

### Task 5: 前端 — Search 窗口支持 Shift+Enter 和按钮

Search 窗口有两个键盘处理路径：正常模式直接处理，inputSuspended 模式转发给 Picker。

**Files:**
- Modify: `src/features/search/SearchShell.tsx:941-1035` (键盘处理器)
- Modify: `src/features/search/SearchShell.tsx:1066-1081` (handlePasteSelected 函数)
- Modify: `src/features/search/SearchShell.tsx:1474-1573` (操作按钮区域)

- [ ] **Step 1: 新增 handlePasteSelectedAsFile 函数**

Edit `src/features/search/SearchShell.tsx` — 在 `handlePasteSelected` 函数之后添加：

```typescript
async function handlePasteSelectedAsFile() {
    const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
    if (!currentItem || currentItem.type !== "image") {
      return;
    }

    try {
      await pasteItem(currentItem.id, {
        restoreClipboardAfterPaste: restoreClipboardRef.current,
        pasteToTarget: true,
        asFile: true,
      });
    } catch (error) {
      showError("执行粘贴失败，请稍后重试");
      console.error("执行粘贴失败", error);
    }
  }
```

- [ ] **Step 2: 在正常模式键盘处理器中检测 Shift+Enter**

Edit `src/features/search/SearchShell.tsx` — 在键盘处理器的 `switch (action)` 中，修改 `"paste"` 分支（约第 1018 行）：

```typescript
case "paste":
    if (event.shiftKey) {
      const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
      if (currentItem?.type === "image") {
        void handlePasteSelectedAsFile();
        return;
      }
    }
    void handlePasteSelected();
    return;
```

- [ ] **Step 3: 在 inputSuspended 模式中转发 Shift+Enter**

Edit `src/features/search/SearchShell.tsx` — 在 inputSuspended 路径中（约第 984 行），修改 Enter 处理：

```typescript
if (event.key === "Enter") {
    event.preventDefault();
    if (event.shiftKey) {
      try {
        await emitTo("picker", PICKER_CONFIRM_AS_FILE_EVENT);
      } catch (error) {
        console.error("控制速贴面板失败", error);
      }
    } else {
      void forwardPickerConfirm();
    }
    return;
}
```

在文件顶部导入中添加 `PICKER_CONFIRM_AS_FILE_EVENT`：

```typescript
import {
  // ... 现有导入
  PICKER_CONFIRM_AS_FILE_EVENT,
  PICKER_CONFIRM_EVENT,
  // ... 其余不变
} from "../../bridge/events";
```

- [ ] **Step 4: 在选中条目操作按钮区域添加「粘贴为路径」按钮**

Edit `src/features/search/SearchShell.tsx` — 在操作按钮区域（约第 1507 行粘贴按钮之后、编辑按钮之前），为图片条目插入新按钮：

```tsx
<button
  aria-label="粘贴当前条目"
  className={STYLES.actionButton}
  onMouseDown={(event) => {
    event.preventDefault();
    event.stopPropagation();
  }}
  onClick={() => void handlePasteSelected()}
  title="粘贴"
  type="button"
>
  {/* 现有粘贴图标 SVG */}
</button>
{(detailQuery.data?.type ?? item.type) === "image" ? (
  <button
    aria-label="粘贴为文件路径"
    className={STYLES.actionButtonSecondary}
    onMouseDown={(event) => {
      event.preventDefault();
      event.stopPropagation();
    }}
    onClick={() => void handlePasteSelectedAsFile()}
    title="粘贴为路径"
    type="button"
  >
    <svg
      aria-hidden="true"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      viewBox="0 0 24 24"
    >
      <path d="M4 7V4h16v3" />
      <path d="M9 20h6" />
      <path d="M12 4v16" />
    </svg>
  </button>
) : null}
{(detailQuery.data?.type ?? item.type) === "text" ? (
  /* 现有编辑按钮 */
```

- [ ] **Step 5: 运行 TypeScript 构建检查**

Run: `rtk pnpm build`
Expected: 构建通过

- [ ] **Step 6: Commit**

```bash
rtk git add src/features/search/SearchShell.tsx
rtk git commit -m "feat: Search 窗口支持 Shift+Enter 和按钮将图片粘贴为文件路径"
```

---

### Task 6: 集成验证

- [ ] **Step 1: 运行完整前端构建**

Run: `rtk pnpm build`
Expected: 构建成功

- [ ] **Step 2: 运行 Rust 测试**

Run: `rtk cargo test` (在 `src-tauri/` 目录下)
Expected: 所有测试通过

- [ ] **Step 3: 启动桌面应用手动验证**

Run: `pnpm tauri dev`

验证清单：
1. 复制一张截图，使其作为图片条目出现在列表中
2. 在 Picker 中选中该图片条目，按 Shift+Enter → 应将文件路径粘贴到目标窗口
3. 在 Picker 中选中该图片条目，按 Enter → 应正常粘贴图片内容
4. 在 Search 中选中该图片条目，按 Shift+Enter → 应将文件路径粘贴到目标窗口
5. 在 Search 中选中该图片条目，看到「粘贴为路径」按钮，点击 → 应将文件路径粘贴到目标窗口
6. 在 Search 中选中文本条目，Shift+Enter → 应正常粘贴文本（不触发 as_file）
7. 在 Search 中选中文本条目，不应出现「粘贴为路径」按钮
