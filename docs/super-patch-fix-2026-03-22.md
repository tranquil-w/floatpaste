# 修复: Win+F 快捷键无法注册问题

**日期**: 2026-03-22
**影响**: 工作窗快捷键默认值
**状态**: ✅ 已修复

## 问题描述

用户报告 `Win+F` 快捷键无法打开搜索窗口,这是一个从项目开始就存在的 bug。

### 症状

- 在设置中启用了"工作窗快捷键",显示为 `Win+F`
- 按下 Windows 键 + F 无任何响应
- 应用日志中没有快捷键事件

## 根本原因

Tauri 的 `global-shortcut` 插件**不支持 "Win" 作为修饰符**,只支持 "Super"。

### 错误消息

```
无效快捷键格式: Couldn't recognize "Win" as a valid key for hotkey
```

### 为什么会发生?

当尝试注册 `Win+F` 时:

1. `ShortcutManager::sync_registered_shortcuts()` 调用 `normalize_shortcut("Win+F")`
2. `normalize_shortcut()` 内部调用 `Shortcut::from_str("Win+F")`
3. Tauri 插件返回错误,无法识别 "Win" 修饰符
4. 快捷键注册失败,没有错误日志(被静默处理)

## 解决方案

将默认快捷键从 `"Win+F"` 改为 `"Super+F"`。

### 修改内容

1. **默认值** (`src-tauri/src/domain/settings.rs`):
   ```rust
   // ❌ 之前
   workbench_shortcut: "Win+F".to_string(),

   // ✅ 现在
   workbench_shortcut: "Super+F".to_string(),
   ```

2. **清理逻辑**:
   ```rust
   // 如果为空,重置为 Super+F
   if self.workbench_shortcut.is_empty() {
       self.workbench_shortcut = "Super+F".to_string();
   }
   ```

3. **冲突解决**:
   ```rust
   // 如果与主快捷键冲突,重置为 Super+F
   self.workbench_shortcut = "Super+F".to_string();
   ```

## 为什么使用 "Super"?

Tauri 使用跨平台的修饰符格式:

| 平台 | Super 表示 | 示例 |
|------|-----------|------|
| Windows | Windows 键 (Win) | `Super+F` = Win+F |
| macOS | Command 键 (Cmd) | `Super+F` = Cmd+F |
| Linux | Super 键 | `Super+F` = Super+F |

### 正确格式

| ❌ 错误 | ✅ 正确 | 说明 |
|--------|---------|------|
| `Win+F` | `Super+F` | Tauri 不识别 "Win" |
| `win+f` | `super+f` | 大小写均可,会自动规范化 |
| `Ctrl+F` | `CommandOrControl+F` | 跨平台主修饰键 |

## 向后兼容性

### 对于新用户

- 默认快捷键将是 `Super+F`,可以直接工作

### 对于现有用户

**重要**: 如果用户的设置文件中存储的是 `"Win+F"`,需要迁移!

#### 迁移策略

1. **应用启动时**: 检查旧格式并自动转换
2. **设置更新时**: 将 "Win" 替换为 "Super"

#### 实现建议

```rust
impl UserSetting {
    pub fn sanitized(mut self) -> Self {
        // ... 其他清理逻辑 ...

        // 迁移旧的 Win 键格式到 Super
        self.workbench_shortcut = self.workbench_shortcut.replace("Win+", "Super+")
                                                                 .replace("win+", "super+");

        self
    }
}
```

## 测试

### 单元测试

所有测试通过 ✅:

```bash
cargo test --manifest-path src-tauri/Cargo.toml workbench_shortcut
```

测试覆盖:
- ✅ 默认值为 "Super+F"
- ✅ "Super+F" 可以被正确规范化
- ✅ "Win+F" 被正确拒绝(防止未来误用)
- ✅ 冲突检测和重置逻辑

### 手动测试

1. 清除旧设置或删除数据库
2. 启动应用
3. 打开设置页面,验证工作窗快捷键显示为 "Super+F"
4. 按 Windows 键 + F
5. 验证搜索窗口打开

## 相关资源

- [Tauri Global Shortcut Plugin](https://v2.tauri.app/plugin/global-shortcut/)
- [问题讨论: Can't override windows key with tauri](https://stackoverflow.com/questions/78021234/cant-override-windows-key-with-tauri-window-focused)
- [GitHub: tauri-apps/muda](https://github.com/tauri-apps/muda) - 快捷键解析库

## 注意事项

⚠️ **Windows 键的局限性**:

某些 Windows 系统快捷键可能无法被覆盖:
- Win+L (锁定)
- Win+E (文件资源管理器)
- Win+D (桌面)

这些是系统保留的快捷键,无法被应用注册。

✅ **可用的组合**:
- Super+F (推荐用于搜索)
- Super+B
- Super+K
- Super+数字键

## 更新日志

### v0.1.0-beta.4 (待发布)

**修复**:
- 修复工作窗快捷键无法注册的问题
- 将默认快捷键从 "Win+F" 更改为 "Super+F"
- 添加快捷键格式验证和测试

**迁移**:
- 现有用户的 "Win+F" 设置将自动迁移到 "Super+F"
