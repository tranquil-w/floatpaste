# 运行行为与数据约定

本文维护剪贴监听、搜索、上屏、跨窗口数据、自启提权和数据保留的当前行为。模块关系见 [架构概览](architecture.md)，窗口时序见 [无焦点速贴指南](no-focus-picker.md)。

## 剪贴监听与入库

- 三类条目：文本（原文/预览/搜索文本/来源应用/文本用途/可选 HTML）、图片（PNG 直传与 DIB、缩略图、哈希去重）、文件（路径列表/数量/总大小）。
- 文本条目按保守规则识别用途（整段链接 → Windows 绝对路径/UNC/逐行路径 → JSON 对象数组/完整围栏/多强特征裸代码，顺序即优先级），识别不确定时回到普通文本。用途只影响类型徽章、悬浮气泡与检索筛选，不改写原文、不访问磁盘与网络、不影响可上屏性；用户分类仍走标签。规则见 `services/text_kind.rs`。
- 链接识别校验 HTTP(S) scheme、主机标签、IP 字面量与 0–65535 端口；列表摘要突出主机与路径/查询/片段，不显示 userinfo。路径摘要突出首条路径的文件名与父路径，并标识额外路径数量。摘要最多 120 字符，悬浮气泡和上屏仍使用完整原文；代码气泡使用等宽字体并保留换行、缩进，度量与渲染共用字体。
- 同 hash（未删除）内容刷新既有条目并置顶，不重复插入。文本 hash 按原始 `full_text` 的 UTF-8 字节与可选 HTML 原载荷按固定顺序、格式标记和长度边界计算（带版本与基础类型前缀的版本化编码，见 `services/text_identity.rs`）：代码缩进、连续空格等原文差异是不同内容；空白折叠加小写的归一化文本仅用于搜索索引，不用于身份。文本用途不参与 hash，调整识别规则不会制造新条目；编辑保存文本后按同一规则重算用途与身份。图片、文件 hash 语义不变。
- 监听使用 `AddClipboardFormatListener`；暂停与来源应用排除先于解析。一轮复制在同一个 OpenClipboard 会话内收集受支持表示，释放后再解析/入库，入库前比较快照序列号，跳过过期结果。
- 文件优先；无文件时，CF_HTML 有可读选区文字则保存同一条目中的纯文本与 HTML，优先使用来源纯文本，缺失时仅从有效片段提取。纯图片 HTML 包装配有可读位图时走图片链路。HTML 损坏或超限时保留可用纯文本；没有可读文字或支持的位图时跳过。
- HTML 使用注册名 `HTML Format`、UTF-8 与字节偏移校验；单载荷最多 4 MiB，文字提取最多 1 MiB、50,000 个 token、128 层嵌套。超限回退纯文本；不执行脚本，不加载图片、样式或网络资源。HTML 原载荷仅用于本地存储与上屏，不承诺网页完整离线保存或跨应用显示一致。
- 上屏与恢复按主身份和实际纯文本/图片回退身份设置 3 秒抑制窗。
- 排除应用名单、暂停监听、自写回抑制（上屏写剪贴板不再入库）。

## 搜索排序

- `recent_desc`：`COALESCE(last_used_at, created_at) DESC`，再按 `created_at DESC`。
- `relevance_desc`：收藏与带标签条目加权置前，其次 `search_text` 子串命中，再按 FTS5 `bm25 ASC`，最后同 `recent_desc` 的时间序。
- 空关键词强制 `recent_desc`；收藏是独立筛选条件、不在速贴置顶，但在 `relevance_desc` 中有加权。
- 类型筛选「文本」下可再按用途细分（全部/普通/链接/代码/路径），细分 chip 仅在文本筛选激活时出现；「仅收藏」为独立开关，与类型、用途、关键词和标签按 AND 组合。切换收藏保留类型与用途，切出文本类型时清空用途；「全部类型」保留收藏开关，「清除筛选」同时清除类型、用途、收藏和标签。不选细分时「文本」包含全部用途。
- 操作按钮与输入暂停提示显示当前配置的会话键，设置保存后即时更新；无历史条目时根据监听状态引导复制或恢复监听。

## 上屏执行

- 速贴负责"选择"，上屏细节统一在 `paste_flow.rs` / `services/paste_support.rs` 收口：写入剪贴板 → 恢复目标窗口 → `SendInput` 注入 Ctrl+V；可选恢复原剪贴板内容。
- 各窗口只选择主动作或次级动作，共用能力解析为 `PasteMode::Original / PlainText / PathText`。Enter 上屏本体，带 HTML 的文本同时发布 HTML 与纯文本；Shift+Enter 对带格式文本只发布纯文本，对图片发布保存路径，对文件发布逐行路径，对普通文本保持原文。次级动作不修改条目。
- 多格式发布前准备全部载荷，在同一次 Open/Empty/Set/Close 会话内发布。中途失败尝试在持锁会话内恢复已准备的白名单快照，不注入 Ctrl+V；Win32 不保证数据库式原子发布。未能自动粘贴时通过托盘提示原因；开启恢复原剪贴板时引导重试，关闭时提示手动 Ctrl+V。
- 上屏操作具有固定操作 ID，发布、注入前判定与恢复判定通过共用串行门执行；归还目标与注入前检查本次版本，发布后重新持锁核验属主并读取关闭后的最终序列号，延迟等待不占锁。恢复等待 550ms，并在持有系统剪贴板锁后比较本次发布序列号；期间发生新复制或另一次发布时取消恢复。
- 快照只驻内存，捕获和恢复共用文件列表、Unicode 文本、HTML、PNG/DIB 白名单，可保留同次复制的多种表示；不承诺恢复任意应用私有格式。恢复需要的 HTML 超过 4 MiB 时，在写入条目前取消并提示，保留用户剪贴板。
- 管理员目标（UIPI）：目标窗口以管理员运行且本应用未提权时，按键注入会被静默丢弃。上屏前检测（`elevation.rs` TokenElevation），命中则照常写入剪贴板、还原目标焦点，仅跳过必然无效的注入；写入成功后经托盘气泡一次性说明（每进程一次，速贴/搜索窗口零 UI）。开启恢复原剪贴板时仅引导管理员重启，关闭时另提示手动 Ctrl+V。

## 跨窗口数据一致性

- 标签词汇与列表数据以「窗口打开/会话刷新时刻」为准，不做跨窗推送：设置页的标签改动在其他窗口下次打开或编辑后刷新时反映，不以事件总线同步。
- 设置窗标签区点击「条目数」跳转搜索窗是该原则下唯一的管理跨窗入口：预置该标签筛选（先与当前词汇求交集，标签已不存在则落到默认视图），首开与快捷键打开同一条上屏链、落地后顶边锚定收缩到内容高度（左上不动、底边伸缩）；会话已打开时直接切筛选、不隐藏重开。

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

SQLite（`%APPDATA%\com.floatpaste\floatpaste.db`）+ FTS5：`clip_items`（条目主表）、`clip_items_fts`（全文索引）、`settings`、`excluded_apps`、`tags`、`clip_item_tags`（条目-标签关联）、`clip_text_formats`（一对一 HTML 载荷）。表结构与字段以 `repository` 的迁移与 `schema.rs` 为唯一来源，不在此复述。

存储策略：文本进库，只有带 HTML 的条目在 `clip_text_formats` 中有对应行；HTML 不进入 FTS，列表与普通详情只查询能力标记，上屏详情按需读取载荷。软删保留格式，物理删除通过外键级联清理。仅改标签、收藏或保存未改变的文本保留格式；保存正文修改时，在同一事务内删除 HTML、重算用途与 hash、更新摘要和 FTS。图片以 PNG 存数据目录 `images/` 子目录；文件条目只记路径引用；删除为软删除。标签只经由条目关联成立：全量替换、重命名与物理删除路径上失去全部关联的标签行随之清理（孤儿清理）；仅被软删条目引用的标签保留——软删即保留期缓冲，清理不得改为 join `clip_items` 过滤，否则破坏恢复语义。

数据库升级时按保存的原文回填文本用途、列表摘要并重算文本 hash；v6 数据同样按当前规则刷新派生字段；v8 在事务内用运行时共用的格式身份函数更新文本 hash。不改 ID、标签与收藏，不合并历史条目；此前被旧版空白归一化合并掉的原文差异无法从库中还原。图片与文件行的用途列保持默认值并被忽略。

切回旧二进制前备份数据库；旧版编辑器不认识附加 HTML，不能保证旧版修改后的文字与格式一致。回退优先保留新数据读取能力，不通过降级删除用户载荷。

## 实现入口

- 录入、编辑与自写回过滤：[history_service.rs](../crates/floatpaste-core/src/services/history_service.rs)、[clip_service.rs](../crates/floatpaste-core/src/services/clip_service.rs)、[paste_support.rs](../crates/floatpaste-core/src/services/paste_support.rs)。
- 文本用途识别与内容身份：[text_kind.rs](../crates/floatpaste-core/src/services/text_kind.rs)、[text_identity.rs](../crates/floatpaste-core/src/services/text_identity.rs)。
- 搜索排序与标签关联：[sqlite_repository.rs](../crates/floatpaste-core/src/repository/sqlite_repository.rs)。
- HTML 字节契约与有界文字提取：[html_format.rs](../crates/floatpaste-core/src/services/html_format.rs)；白名单采集与发布：[clipboard_formats.rs](../crates/floatpaste-core/src/platform/windows/clipboard_formats.rs)。
- 上屏编排：[paste_flow.rs](../crates/floatpaste-native/src/paste_flow.rs)。
- 启动权限判断：[launch_mode.rs](../crates/floatpaste-core/src/launch_mode.rs)；自启任务：[elevated_task.rs](../crates/floatpaste-core/src/platform/windows/elevated_task.rs)。
- 表结构与迁移：[schema.rs](../crates/floatpaste-core/src/repository/schema.rs)。
