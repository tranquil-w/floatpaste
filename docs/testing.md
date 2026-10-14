# 测试体系

自动化测试分三层，各层只覆盖自己擅长的不稳定面，不越层重复：

| 层 | 位置 | 覆盖目标 |
|----|------|----------|
| 单元 | 各模块 `#[cfg(test)]` | 纯函数、仓储（临时 SQLite）、服务的独立行为 |
| L1 进程内集成 | `floatpaste-core/tests/` | 临时库组装 `CoreState`，沿公共 API 串行验证连贯业务流（录入→去重→标签→收藏→搜索→软删→保留清理） |
| L2 无头 UI 驱动 | `floatpaste-native` 内联测试 | `set_size` + `dispatch_event` 驱动 Slint 窗口断言 UI 行为；winit 事件循环是进程级单例，此类测试必须单函数串行多段 |
| L3 真进程 E2E | `floatpaste-native/tests/e2e_smoke.rs` | spawn 真实二进制走完整用户链路，断言隔离库与窗口几何 |

运行入口统一为 `cargo test`（含全部层）；单跑 L3 用
`cargo test -p floatpaste-native --test e2e_smoke`。

## L2 无头驱动约束

- 入口唯一：各窗口契约段以 `pub(crate) fn run()` 提供并由单一 `#[test]`
  （`l2_headless_ui_contracts`）串行调用——Slint 平台绑定首个初始化它的
  线程，`#[test]` 各占一线程，多入口会抢占线程绑定而实例化失败
- 布局读数须在事件循环泵帧后取：无循环时布局 pass 不跑，行模型注入后
  `content-height` 等布局派生属性停留在旧值；不受布局影响的属性
  （如 Flickable 偏移）可直接赋值断言
- `changed` 回调由事件循环调度而非属性赋值时同步执行：断言其副作用须
  先泵一次循环（`run_event_loop_until_quit` + 定时线程 `quit_event_loop`，
  异步落地（`invoke_from_event_loop` 的闭包）同样只有循环运行时才消费）
- 查询链路用临时 SQLite 直写预置数据，不经系统剪贴板（互斥资源，
  见 L3 约束）

## L3 端到端约束

### 隔离

被测实例的全部状态必须圈在临时目录内，靠以下钩子实现（缺省会走生产行为，钩子仅测试使用）：

- `APPDATA=<临时目录>`：进程级环境变量重定向，数据库、图片存储、日志随之整体隔离
- `FLOATPASTE_INSTANCE_ID`（仅字母数字与连字符）：给单实例互斥量与唤醒事件名追加后缀，被测实例与日常实例共存
- `FLOATPASTE_RETENTION_FIRST_DELAY_MS`：覆盖保留清理首轮延迟（默认 60s），让接线验证落在测试时间尺度
- `FLOATPASTE_SKIP_AUTOSTART_SYNC=1`：跳过自启同步——它操作真实系统的 Run 键与任务计划，不随 APPDATA 隔离

### 驱动与断言

- 外部驱动只走真实用户路径：写系统剪贴板触发录入、二次启动进程经唤醒事件开关速贴面板；不使用键盘注入（SendInput 在安全软件环境不可靠），上屏注入环节留给人工验收
- 托盘菜单命令（切换监听）经命名事件 `signal_test_toggle_monitoring` 驱动：模态 TrackPopupMenu 点击无法可靠自动化，事件触发的处理路径与菜单选择完全相同（翻转落库 + 运行时联动），仅 `FLOATPASTE_INSTANCE_ID` 设置时装载
- 速贴面板显隐是「屏内 ↔ 屏外停屏（-32000 平移）」语义而非 `SW_HIDE`，`IsWindowVisible` 恒真——面板开合断言必须看窗口几何（`GetWindowRect` 左上角是否在屏内）
- 速贴定位预置 `LastPosition` 模式与屏内记忆位置：Mouse/Caret 模式依赖运行环境的光标与前台窗口，E2E 验证的是唤醒链路而非定位模式

### 卫生纪律

- 单测试函数串行推进：系统剪贴板、全局热键、速贴窗口都是会话级全局资源
- 子进程回收必须用 Drop 守卫覆盖 panic unwind 路径：孤儿被测实例持有外层管道句柄会把整条测试命令挂死
- 被测实例收尾走优雅退出（`signal_test_exit` 命名事件 → 正常收尾路径），超时才兜底强杀：进程在剪贴板打开临界区内死亡会把系统剪贴板锁死（见下方故障模式）
- 剪贴板是互斥资源，输入法、剪贴板管理器等外部进程会间歇占用，写入类操作短重试，持续失败让测试失败（不掩盖环境故障）
- 运行副作用：被测实例短暂占用主/搜索快捷键组合、读写系统剪贴板（结束时尽力恢复原文本）

### 故障模式：系统剪贴板锁死

系统剪贴板是全局互斥资源，锁死后 `OpenClipboard` 持续返回
`ERROR_ACCESS_DENIED`，无法自愈。两种成因，恢复手段相同：

- 外部进程打开剪贴板后持有不释放（实测：shell 组件——explorer 树内的
  TextInputHost/WebView2 组件群会周期性进入此态，锁死瞬间无进程死亡、
  `GetOpenClipboardWindow` 查无持有者，杀 explorer 树 1 秒内释放）
- 任意进程在剪贴板打开临界区内死亡（强杀、崩溃——受控实验已实证）

恢复手段：重启 explorer（`taskkill /f /im explorer.exe && start
explorer.exe`，连带退出坏态 shell 组件并拉起新实例）或注销重登。

测试对此分两段处理：

- 启动前探测（`clipboard_unusable_before_start`）：~1.2 秒内无法打开即
  视为环境先在的锁死/长占用态——此时测试与被测实例都还没碰过剪贴板，
  责任不在被测物，打印恢复指引后跳过，不把环境故障计成测试失败
- 运行中途失败仍照常报错：写入短重试后 panic，附带锁死恢复指引——
  中途失效可能是被测实例把剪贴板搞坏，不得跳过掩盖

多测试目标全量并行时为低概率竞态，单跑 E2E 稳定。
