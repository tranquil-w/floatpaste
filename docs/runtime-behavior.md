# 运行行为与数据约定

本文维护剪贴监听、搜索、上屏、跨窗口数据、自启提权和数据保留的当前行为。模块关系见 [架构概览](architecture.md)，窗口时序见 [无焦点速贴指南](no-focus-picker.md)。

## 剪贴监听与入库

- 三类条目：文本（原文/预览/搜索文本/来源应用）、图片（PNG 直传与 DIB、缩略图、哈希去重）、文件（路径列表/数量/总大小）。
- 同 hash（未删除）内容刷新既有条目并置顶，不重复插入；监听使用 `AddClipboardFormatListener`，上屏写回按内容 hash 设置 3 秒抑制窗。
- 排除应用名单、暂停监听、自写回抑制（上屏写剪贴板不再入库）。

## 搜索排序

- `recent_desc`：`COALESCE(last_used_at, created_at) DESC`，再按 `created_at DESC`。
- `relevance_desc`：收藏与带标签条目加权置前，其次 `search_text` 子串命中，再按 FTS5 `bm25 ASC`，最后同 `recent_desc` 的时间序。
- 空关键词强制 `recent_desc`；收藏是独立筛选条件、不在速贴置顶，但在 `relevance_desc` 中有加权。
- 操作按钮与输入暂停提示显示当前配置的会话键，设置保存后即时更新；无历史条目时根据监听状态引导复制或恢复监听。

## 上屏执行

- 速贴负责"选择"，上屏细节统一在 `paste_flow.rs` / `services/paste_support.rs` 收口：写入剪贴板 → 恢复目标窗口 → `SendInput` 注入 Ctrl+V；可选恢复原剪贴板内容。
- 条目 Enter 上屏本体（文本/图片数据/文件列表），Shift+Enter 次级上屏为路径文本：图片上屏所在路径、文件上屏逐行路径列表（`PasteOption.as_path_text`）。未能自动粘贴时通过托盘提示原因；开启恢复原剪贴板时引导重试，关闭时提示手动 Ctrl+V。
- 管理员目标（UIPI）：目标窗口以管理员运行且本应用未提权时，按键注入会被静默丢弃。上屏前检测（`elevation.rs` TokenElevation），命中则照常写入剪贴板、还原目标焦点，仅跳过必然无效的注入；写入成功后经托盘气泡一次性说明（每进程一次，速贴/搜索窗口零 UI）。开启恢复原剪贴板时仅引导管理员重启，关闭时另提示手动 Ctrl+V。

## 跨窗口数据一致性

- 标签词汇与列表数据以「窗口打开/会话刷新时刻」为准，不做跨窗推送：设置页的标签改动在其他窗口下次打开或编辑后刷新时反映，不以事件总线同步。
- 设置窗标签区点击「条目数」跳转搜索窗是该原则下唯一的管理跨窗入口：预置该标签筛选（先与当前词汇求交集，标签已不存在则落到默认视图），首开与快捷键打开同一条上屏链、落地后底边锚定收缩到内容高度；会话已打开时直接切筛选、不隐藏重开。

## 开机自启与提权启动

- 提权是对一次性动作：设置页「以管理员身份重启」（未提权时显示）→ UAC 确认 → 经 `runas` 重启（新实例等旧实例释放单实例互斥量后接管）；重启后即拥有管理员权限。
- 任务计划程序是开机自启的唯一载体（`elevated_task.rs` COM ITaskService）：任务存在 = 「开机自启」开；任务 RunLevel（HIGHEST/LUA）= 「始终以管理员身份运行」开。该勾选仅提权运行时可更改；勾选后每次启动均带管理员权限——登录自启走 HIGHEST 任务免 UAC；手动启动（asInvoker manifest 不触发 UAC）由启动期自检兜底（`launch_mode::needs_elevated_relaunch`，托盘就绪后执行，透传 `--silent` 等原参数）。
- 启动期提权自检：预期提权而实际未提权时，经 UAC 重入自身（复用 `--elevated-relaunch` 闭环，本进程走退出收尾后让位，提权新实例接管）；UAC 被取消则本次以普通权限运行，托盘气泡说明一次。带 `--elevated-relaunch` 参数的启动不再自检，防 UAC 异常时无限弹窗。
- 任务 ACL 仅授予 SYSTEM/Administrators/任务所属用户：非提权进程可查询/删除/重建自己的任务，只有注册 HIGHEST 任务需要提权（进程已提权时直接注册；删除另有 `--remove-elevated-autostart` UAC 兜底）。启动时清理 Run 键中的本应用旧版自启条目，避免重复启动。
- 同步时机：启动时与设置保存后（后台线程串行执行，`query` 快照幂等跳过已达形态）；同步失败回滚对应开关并提示。触发器延迟 3s 等 Explorer 就绪；任务名带用户名避免多用户冲突。

## Win+V 接管

- Win+V 作为速贴的第二个唤起入口，与主快捷键执行相同动作；组合相同时跳过重复注册。
- 设置开关先弹出影响说明，用户确认后写入或清除 `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\DisabledHotkeys` 的 V 字母并重启资源管理器；取消保持原配置。其他禁用字母保留，删空时删除整个值。
- 开启和关闭接管分别需要使资源管理器重启生效；不要求用户关闭系统剪贴板历史。`TaskbarCreated` 通知后延迟补注册，覆盖资源管理器恢复过程。
- 快捷键录制期间注销全局热键，避免录制组合触发本应用动作。卸载器清理接管的 V 字母并提示重启资源管理器。
- 实现见 [winv_takeover.rs](../crates/floatpaste-core/src/platform/windows/winv_takeover.rs)、[settings.rs](../crates/floatpaste-native/src/settings.rs) 和 [安装卸载脚本](../packaging/floatpaste.iss)；选型理由见 [ADR-0001](adr-0001-winv-takeover-via-registry.md)。

## 数据模型

SQLite（`%APPDATA%\com.floatpaste\floatpaste.db`）+ FTS5：`clip_items`（条目主表）、`clip_items_fts`（全文索引）、`settings`、`excluded_apps`、`tags`、`clip_item_tags`（条目-标签关联）。表结构与字段以 `repository` 的迁移与 `schema.rs` 为唯一来源，不在此复述。

存储策略：文本进库；图片以 PNG 存数据目录 `images/` 子目录；文件条目只记路径引用；删除为软删除。标签只经由条目关联成立：全量替换、重命名与物理删除路径上失去全部关联的标签行随之清理（孤儿清理）；仅被软删条目引用的标签保留——软删即保留期缓冲，清理不得改为 join `clip_items` 过滤，否则破坏恢复语义。

## 实现入口

- 录入与自写回过滤：[clip_service.rs](../crates/floatpaste-core/src/services/clip_service.rs)、[paste_support.rs](../crates/floatpaste-core/src/services/paste_support.rs)。
- 搜索排序与标签关联：[sqlite_repository.rs](../crates/floatpaste-core/src/repository/sqlite_repository.rs)。
- 上屏编排：[paste_flow.rs](../crates/floatpaste-native/src/paste_flow.rs)。
- 启动权限判断：[launch_mode.rs](../crates/floatpaste-core/src/launch_mode.rs)；自启任务：[elevated_task.rs](../crates/floatpaste-core/src/platform/windows/elevated_task.rs)。
- 表结构与迁移：[schema.rs](../crates/floatpaste-core/src/repository/schema.rs)。
