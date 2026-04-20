# 图片条目粘贴为文件路径

## 背景

当前图片类型的剪贴板条目粘贴时，会将像素数据写入剪贴板，目标窗口接收到的是图片内容。某些场景（如聊天窗口上传文件）更适合以文件路径的形式粘贴。

## 需求

- **Shift+Enter**：在 Picker 和 Search 窗口中，选中图片条目时按 Shift+Enter，将图片文件路径作为纯文本写入剪贴板并粘贴到目标窗口
- **Search 窗口按钮**：为图片条目新增一个操作按钮，点击行为与 Shift+Enter 相同
- **适用范围**：所有 `type === "image"` 的条目

## 设计

### 后端

#### PasteOptions 扩展

`src-tauri/src/commands/clips.rs` 中 `PasteOptions` 新增字段：

```rust
pub as_file: bool,
```

#### write_item_to_clipboard 分支

`src-tauri/src/services/paste_executor.rs` 中，当 `as_file == true` 且条目为图片类型时，将 `imagePath` 字符串作为纯文本写入剪贴板（与文本条目处理方式相同）。其余条目类型在 `as_file == true` 时忽略该标志，走原有逻辑。

```
match (item_type, as_file) {
    (image, true)  => 写入 imagePath 为纯文本
    (image, false) => 写入图片像素数据（原有逻辑）
    _              => 走原有逻辑
}
```

PasteExecutor 的其余流程（窗口恢复、Ctrl+V 触发、剪贴板恢复）不变，仅透传 `as_file` 标志。

### 前端

#### Bridge 层

`src/bridge/commands.ts` 中 `pasteItem` 的选项类型新增 `asFile?: boolean`，透传到 Tauri 的 `paste_item` 命令。

#### Picker 窗口（PickerShell.tsx）

Enter 键处理中检测 `event.shiftKey`：

- 当前选中条目为图片类型且按住 Shift → `pasteItem(id, { asFile: true, ... })`
- 其余情况 → 走原有逻辑

#### Search 窗口（SearchShell.tsx）

**Shift+Enter**：同 Picker，键盘事件中检测 `event.shiftKey`，对图片条目传入 `asFile: true`。

**新增按钮**：在现有操作按钮区域中，当选中条目为图片类型时，在粘贴按钮和编辑按钮之间增加一个「粘贴为路径」按钮，点击行为与 Shift+Enter 相同（调用 `pasteItem` 并传入 `asFile: true`）。

## 不做的事

- 不支持非图片条目的 `as_file` 模式（文本条目粘贴为"文本"没有意义）
- 不新增独立的 Tauri 命令（复用现有 `paste_item`）
- Picker 窗口不增加按钮（仅通过 Shift+Enter 触发）
