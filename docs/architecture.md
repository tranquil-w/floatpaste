# FloatPaste 架构概览

> 活文档：描述当前架构与关键行为规格，随实现更新；失真视同 bug。
> 术语以根目录 [CONTEXT.md](../CONTEXT.md) 为准；窗口时序与坑位细节见
> [无焦点速贴窗口架构与防坑指南](no-focus-picker.md)，颜色体系见[主题系统设计](theme-system.md)。

## 产品定位与设计原则

FloatPaste 是 Windows 桌面剪贴板工具，核心价值不是"保存很多剪贴内容"，而是**在不打断当前工作窗口的前提下，快速找回并重新粘贴历史内容**。

- **不抢焦点优先于功能完备**：速贴唤起 → 选择 → 上屏这条主链路尽量不碰目标窗口焦点；复杂输入、深度搜索、编辑一律进有焦点的窗口。
- **速贴与管理分离**：速贴只做快速选择与上屏；搜索/编辑/设置在正常获焦窗口中完成。
- **本地优先**：数据全部在本机，不向外部服务发送剪贴内容。
- **平台特化**：仅支持 Windows 10/11 x64，剪贴板监听、键鼠钩子、无焦点窗口等直接使用 Win32 原生实现，不为跨平台妥协。

## 总体结构

Cargo workspace 双 crate：

```text
floatpaste-core（与 GUI 无关的共享核心）
  domain/        纯数据与业务概念（clip_item / settings / search_session / editor_session …）
  repository/    rusqlite 数据访问（SQLite + FTS5 与迁移）
  services/      业务规则编排（clip / history / search / normalize / dedup / privacy / retention /
                 paste_support / picker_position / image_storage / image_decode / startup /
                 tag / time_format / clip_display）
  platform/windows/  Win32 原生集成（clipboard_monitor / session_keyboard / mouse_monitor /
                 hotkey / window_control / active_app / single_instance …，回调向 UI 暴露事件）
  theme.rs       色板 / 强调色 / 语义 token 派生与对比度校正

floatpaste-native（唯一桌面壳，Slint 软件渲染）
  src/           窗口会话模块（各带 wire() 绑定回调），main.rs 只管启动顺序与接线
  ui/*.slint     各窗口界面与 Theme 全局
```

分层约束：core 不依赖 Slint；platform 不反向依赖窗口框架；不依赖 Slint 类型的纯逻辑下沉 core（如 `clip_display`），保证无 GUI 即可测试。

## 窗口与职责

| 窗口 | 焦点 | 职责 |
|------|------|------|
| 速贴 Picker | 无焦点（`WS_EX_NOACTIVATE`） | 最近活跃列表（文本/图片缩略图/文件摘要）、会话键导航、收藏、确认上屏；不提供自由文本输入 |
| 搜索 Search | 正常获焦 | FTS5 全文搜索 + 类型/标签筛选、防抖、触底分页、两段式删除、进编辑器 |
| 编辑 Editor | 正常获焦 | 条目文本编辑（脏状态与关闭确认）+ 标签管理；从速贴或搜索进入，关闭返回来源窗口 |
| 设置 Settings | 正常获焦 | 通用/快捷键/外观/行为/排除应用/标签六区，防抖自动保存 + 运行时联动 |
| 悬浮气泡 Tooltip | 无焦点、点击穿透 | 行悬停预览（文本摘要/图片大图），400ms 延迟、屏幕边缘自适应定位，全局单例 |
| 托盘 | — | 打开速贴/搜索/设置、暂停与恢复监听、退出 |

五窗口单进程共享一个事件循环，启动即建：速贴/搜索/悬浮气泡屏外停屏、按需上屏（`overlay.rs`，winit 窗口常驻）；编辑/设置窗关闭即销毁回收帧缓冲、重开两段式重建。

## 关键行为规格

### 剪贴监听与入库

- 三类条目：文本（原文/预览/搜索文本/来源应用）、图片（PNG 直传与 DIB、缩略图、哈希去重）、文件（路径列表/数量/总大小）。
- 同 hash（未删除）内容刷新既有记录并置顶，不重复插入；入库防抖由剪贴板序列号检测与自写回过滤（3 秒抑制窗）承担。
- 排除应用名单、暂停监听、自写回抑制（上屏写剪贴板不再入库）。

### 搜索排序

- `recent_desc`：`COALESCE(last_used_at, created_at) DESC`，再按 `created_at DESC`。
- `relevance_desc`：FTS5 `bm25 ASC` 优先，其后同上。
- 空关键词强制 `recent_desc`；收藏是独立筛选条件，不参与默认排序、不在速贴置顶。

### 上屏执行

- 速贴负责"选择"，上屏细节统一在 `paste_flow.rs` / `services/paste_support.rs` 收口：写入剪贴板 → 恢复目标窗口 → `SendInput` 注入 Ctrl+V；可选恢复原剪贴板内容。
- 图片条目 Enter 上屏图片数据，Shift+Enter 上屏文件路径；注入失败提示手动粘贴。
- 管理员目标（UIPI）：目标窗口以管理员运行且本应用未提权时，按键注入会被静默丢弃。上屏前检测（`elevation.rs` TokenElevation），命中则照常写入剪贴板、还原目标焦点，仅跳过必然无效的注入，并经托盘气泡一次性说明（每进程一次，速贴/搜索窗口零 UI）。

### 开机自启与提权启动

- 提权是对一次性动作（对齐 PowerToys）：设置页「以管理员身份重启」（未提权时显示）→ UAC 确认 → 经 `runas` 重启（新实例等旧实例释放单实例互斥量后接管）；重启后即拥有管理员权限。
- 任务计划程序是开机自启的唯一载体（`elevated_task.rs` COM ITaskService）：任务存在 = 「开机自启」开；任务 RunLevel（HIGHEST/LUA）= 「始终以管理员身份运行」开。该勾选仅提权运行时可更改；勾选后每次启动（含登录自启）均带管理员权限。
- 任务 ACL 仅授予 SYSTEM/Administrators/任务所属用户：非提权进程可查询/删除/重建自己的任务，只有注册 HIGHEST 任务需要提权（进程已提权时直接注册；删除另有 `--remove-elevated-autostart` UAC 兜底）。Run 键自启已退役，启动时无条件清理存量条目。
- 同步时机：启动时与设置保存后（后台线程串行执行，`query` 快照幂等跳过已达形态）；同步失败回滚对应开关并提示。触发器延迟 3s 等 Explorer 就绪；任务名带用户名避免多用户冲突。

## 数据模型

SQLite（`%APPDATA%\com.floatpaste\floatpaste.db`）+ FTS5，表：`clip_items`、`clip_items_fts`（索引 full_text/search_text/source_app）、`settings`、`excluded_apps`、`tags`、`clip_item_tags`。

`clip_items` 关键字段：`id`、`type`（text/image/file）、`full_text`/`preview_text`/`search_text`、`source_app`、`is_favorited`、`hash`、图片四字段（`image_path`/`width`/`height`/`format`）、文件四字段（`file_paths` JSON/`file_count`/`directory_count`/`total_size`）、`created_at`/`updated_at`/`last_used_at`/`deleted_at`（软删除）。

存储策略：文本进库；图片以 PNG 存数据目录 `images/` 子目录；文件条目只记路径引用；删除为软删除。

## 关键文件索引

**floatpaste-native（壳）**

- 启动装配：`src/main.rs`、`system.rs`
- 窗口会话：`picker.rs`、`search.rs`、`editor.rs`、`settings.rs`（各含 `wire()`）
- 悬停预览 `tooltip.rs`；停屏与上屏 `overlay.rs`；上屏编排 `paste_flow.rs`；托盘 `tray.rs`
- 缩略图缓存 `thumbnails.rs`；主题桥 `theme_bridge.rs`；共享状态 `app_state.rs`；Win32 辅助 `win32_ext.rs`

**floatpaste-core（核心域）**

- 监听：`platform/windows/clipboard_monitor.rs`、`image_clipboard.rs`、`file_clipboard.rs`
- 会话键盘（LL 钩子）`session_keyboard.rs`；外击关闭（LL 鼠标钩子）`mouse_monitor.rs`
- 窗口手势（拖拽/八方向拉伸）`window_control.rs`；前台与焦点 `active_app.rs`
- 定位 `picker_position.rs` + `services/picker_position_service.rs`；热键 `hotkey.rs`；单实例 `single_instance.rs`；自启 `startup.rs`
- 提权检测与 runas 启动 `elevation.rs`；管理员自启任务 `elevated_task.rs`（COM ITaskService）
- 仓储 `repository/sqlite_repository.rs`；展示格式化 `services/clip_display.rs`；主题 `theme.rs`

## 已知实现取舍

- 无焦点体验依赖 Win32 焦点恢复、低级键鼠钩子与热键注册时序；速贴/搜索/悬浮气泡显隐走「屏外停屏 + 上屏移动」，避免 `ShowWindow` 焦点副作用与首帧闪烁。
- 头部拖拽与八方向拉伸用非模态手势（Slint 事件驱动 down/move/up），规避系统模态循环吞掉指针抬起。
- Slint 软件渲染单进程多窗；`slint::Image` 非 `Send`，缩略图跨线程只传原始像素。
- 光标定位依赖 `GetGUIThreadInfo`，无插入符时回退鼠标定位。
- 窗口生命周期与置顶的坑位结论（500ms 置顶守护、销毁重开两段式收尾等）统一见[无焦点速贴防坑指南](no-focus-picker.md)。

## 运行与调试

构建、测试、版本号命令见 [AGENTS.md](../AGENTS.md)（唯一命令来源）；本节只记调试要点：

- 日志：按天滚动 `%APPDATA%\com.floatpaste\logs\floatpaste-native.log`
- 静默启动：`--silent`；二次启动经单实例命名事件唤醒已有实例的速贴后退出
- 发版流程见 [release/process.md](release/process.md)
- WSL 开发用 `./scripts/win-cargo test` 转发 Windows 工具链
