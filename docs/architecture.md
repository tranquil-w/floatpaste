# FloatPaste 架构概览

本文说明模块边界、主要数据流和窗口职责，帮助定位改动入口。领域术语见 [CONTEXT.md](../CONTEXT.md)，行为细节由 [运行行为与数据约定](runtime-behavior.md) 和各专题维护。

## 产品约束

- Windows 10/11 x64 桌面剪贴板工具，业务数据保存在本机，剪贴内容不发送给外部服务。
- 速贴优先保持目标应用的输入焦点；自由文本搜索、编辑和设置由正常获焦窗口承担。
- Rust 单进程、Slint 软件渲染，平台集成直接使用 Win32。

## 模块与依赖

Cargo workspace 包含两个 crate：

| 模块 | 职责与依赖约束 |
|------|----------------|
| `floatpaste-core/domain` | 数据、业务概念与错误类型，不执行数据库、文件或 Win32 操作；`AppError` 保留 rusqlite、IO、JSON 与 Windows 错误转换 |
| `floatpaste-core/repository` | rusqlite 数据访问、FTS5 与数据库迁移 |
| `floatpaste-core/services` | 业务规则、存储服务与平台流程编排；上屏、定位等流程直接调用 platform/windows |
| `floatpaste-core/platform/windows` | 剪贴板、快捷键、单实例、窗口控制等 Win32 集成，通过回调暴露事件，不反向依赖窗口框架 |
| `floatpaste-core/theme.rs` | 色板、语义 token 派生与对比度校正 |
| `floatpaste-native/src` | 窗口会话与回调装配；各窗口提供 `wire()`，`main.rs` 负责启动顺序与模块接线 |
| `floatpaste-native/ui` | Slint 界面与共享主题全局 |

core 不依赖 Slint；不依赖 Slint 类型的纯逻辑（如展示格式化）放在 core，以便脱离 GUI 测试。

native 的 `app.rs` 持有各窗口共享的应用上下文，窗口弱引用只在事件循环线程升级；`app_state.rs` 维护可跨线程访问的会话与缓存数据。纯列表导航由 core 的 `services/list_navigation.rs` 共用，Slint 上屏菜单模型由 native 的 `paste_actions.rs` 投影，避免让一个窗口模块成为其他窗口的公共依赖入口。

## 主要数据流

```mermaid
flowchart LR
    Clipboard[Windows 剪贴板] --> Monitor[Win32 监听]
    Monitor --> Services[core 业务服务]
    Services --> Repository[SQLite / FTS5 / 图片文件]
    Services --> Session[native 窗口会话]
    UI[Slint 界面回调] --> Session
    Session --> Services
    Session --> Paste[上屏编排]
    Paste --> Clipboard
    Paste --> Target[恢复目标窗口并注入 Ctrl+V]
```

平台回调和后台结果转发到事件循环后更新窗口属性与模型；缩略图与速贴底图跨线程传原始像素，Slint 图像在 UI 线程创建。速贴定位与底图准备在后台执行，采用时限与回退规则见 [无焦点速贴指南](no-focus-picker.md)。

## 窗口职责

| 窗口 | 焦点 | 职责 |
|------|------|------|
| 速贴 | 无焦点 | 最近条目、会话键导航、收藏、两段式删除与上屏 |
| 搜索窗口 | 正常获焦 | 全文搜索、类型/标签筛选、分页、收藏、两段式删除与进入编辑 |
| 编辑窗口 | 正常获焦 | 当前条目的内容编辑与标签管理，关闭返回来源窗口 |
| 设置窗口 | 正常获焦 | 设置保存与运行时联动、排除应用、标签管理 |
| 悬浮气泡 | 无焦点、点击穿透 | 条目预览与提示 |
| 托盘 | — | 打开窗口、暂停/恢复监听与退出 |

五窗口启动即建，共享事件循环。速贴、搜索窗口与悬浮气泡平时停在屏外；编辑与设置窗口通过 Win32 真隐藏进入停屏，Slint 侧保持窗口存活。显隐、前台归还、置顶和定位约束统一见 [无焦点速贴指南](no-focus-picker.md)。

## 改动入口

Rust 文件路径相对于对应 crate 的 `src/`；Slint 界面位于 native 的 `ui/`：

| 改动范围 | 主要入口 | 当前约束 |
|----------|----------|----------|
| 录入、去重、搜索与标签 | core 的 `services/`、`repository/sqlite_repository.rs` | [运行行为与数据约定](runtime-behavior.md) |
| HTML 采集、发布与恢复 | core 的 `platform/windows/clipboard_formats.rs`、`services/html_format.rs`、`services/paste_support.rs` | 白名单同会话采集，解析在锁外；HTML 只作载荷，不交界面执行，见 [运行行为与数据约定](runtime-behavior.md) |
| 文本用途识别与内容身份 | core 的 `services/text_kind.rs`、`services/text_identity.rs` | 识别与搜索归一化分离；用途不参与 hash，见 [运行行为与数据约定](runtime-behavior.md) |
| 启动、日志与共享状态 | native 的 `main.rs`、`system.rs`、`app.rs`、`app_state.rs` | [开发与诊断](../CONTRIBUTING.md) |
| 窗口与上屏 | native 的 `picker.rs`、`search.rs`、`overlay.rs`、`paste_flow.rs` | [窗口时序与防坑](no-focus-picker.md) |
| 会话键与窗口外点击 | core 的 `platform/windows/session_keyboard.rs`（RegisterHotKey）、`mouse_monitor.rs`（LL 鼠标钩子） | [窗口时序与防坑](no-focus-picker.md) |
| 插入符定位 | core 的 `platform/windows/picker_position.rs`、`uia_caret.rs`、`services/picker_position_service.rs` | [窗口时序与防坑](no-focus-picker.md) |
| 编辑交互 | native 的 `editor.rs` 与 `ui/editor.slint` | [编辑窗口规格](editor-window.md) |
| 主题与材质 | core 的 `theme.rs`、`backdrop.rs`；native 的 `theme_bridge.rs`、`overlay.rs` | [主题系统](theme-system.md) |
| 自启与提权 | core 的 `launch_mode.rs`、`platform/windows/startup.rs`、`elevated_task.rs` | [运行行为与数据约定](runtime-behavior.md) |

重要方案的背景和取舍由 [文档地图中的 ADR](README.md#决策依据) 索引；测试与发布分别见 [测试体系](testing.md) 和 [发布流程](release/process.md)。
