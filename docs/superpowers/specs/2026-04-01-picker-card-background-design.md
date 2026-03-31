# Picker 列表项实心卡片背景设计

## 背景

Picker 窗口使用透明背景，列表项默认背景也为 `bg-transparent`。在非选中、非悬停状态下，相邻列表项之间缺少视觉边界，整体看起来像一整块文字，难以快速区分每条记录。

## 目标

让每个列表项都有实心卡片式背景，卡片之间留有间隙，通过间隙透出的容器背景色形成自然分隔线。

## 方案

采用**容器与卡片分层**策略（方案 B）：容器区域使用比卡片更深的背景色，卡片间间隙透出容器色，形成三层视觉层次。

### 改动点

#### 1. 列表容器区域

当前列表容器 `<div className="grid flex-1 gap-1 ...">` 无背景色。

改动：在其外层 `<div className="flex min-h-0 flex-1 flex-col px-1 py-1.5">` 添加 `bg-pg-canvas-subtle rounded-md`，让容器有明确背景。

#### 2. 列表项样式 (`STYLES.itemButton`)

| 状态 | 当前样式 | 改动后 |
|------|---------|--------|
| 普通 | `bg-transparent border-transparent` | `bg-pg-canvas-default border-pg-border-subtle` |
| 悬停 | `hover:bg-pg-canvas-subtle` | 不变 |
| 选中 | `bg-pg-accent-subtle border-pg-accent-fg/30` | 不变 |
| 收藏 | `bg-transparent border-l-[3px] border-l-pg-accent-fg` | `bg-pg-canvas-default border-l-[3px] border-l-pg-accent-fg`，其余三边 `border-pg-border-subtle` |

每个列表项保持 `rounded-md` 圆角。

#### 3. 间距

保持现有 `gap-1`（4px），间隙中透出容器背景色 `bg-pg-canvas-subtle`。

### 不改动的部分

- 窗口透明配置（`transparent: true`）
- 头部区域样式
- 选中态高亮样式
- 键盘快捷键标签、类型标签等

## 色彩层次（暗色主题示例）

```
透明窗口背景
  └─ 容器区域 bg-pg-canvas-subtle (#262C36)
       └─ 卡片 bg-pg-canvas-default (#212830)
            └─ 选中卡片 bg-pg-accent-subtle (蓝色)
```

## 影响范围

- `src/features/picker/` 中的样式常量
- 仅视觉样式改动，无逻辑变更
