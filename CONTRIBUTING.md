# 开发与贡献

本文提供开发环境、常用命令、验证方式和贡献要求。理解模块关系从 [架构概览](docs/architecture.md) 开始；术语使用 [CONTEXT.md](CONTEXT.md) 的命名。

## 环境与常用命令

使用 Windows 10/11 x64 和 Rust MSVC 工具链（含 C++ 构建工具与 Windows SDK）。发布说明等 Node 脚本使用 Node.js 22，无需安装 npm 依赖。

| 命令 | 说明 |
|------|------|
| `cargo run -p floatpaste-native --bin floatpaste` | 启动桌面应用（debug） |
| `cargo test` | 运行全部 Rust 测试（覆盖 `floatpaste-core` 与 `floatpaste-native`，含端到端） |
| `cargo test -p floatpaste-native --test e2e_smoke` | 单跑真进程端到端（运行十余秒；期间短暂占用全局快捷键并读写系统剪贴板；体系见 [docs/testing.md](docs/testing.md)） |
| `cargo build --release -p floatpaste-native` | 发布构建 |
| `cargo run -p floatpaste-native --bin picker-snapshot -- <light\|dark> <tag> [hover-y] [blur]` | 无头渲染速贴观感快照到 `.artifacts/snapshot/`（诊断工具，不模拟桌面输入）；`blur` 合成彩底走烘焙管线，预览玻璃底图与行卡半透 |
| `node scripts/bump-version.mjs <版本>` | 同步升级 package.json / native Cargo.toml / Cargo.lock |

`package.json` 仅保留版本号（发版 tag 校验用）与 `test:rust` 等价入口，不含任何依赖。

速贴快照可追加 `actions=<从 0 起的行号>` 展开动作浮层，`frame=<毫秒>` 捕获进场帧；`hover-x=<逻辑坐标>` 与 `hover-y` 配合检查展开钮悬停。

### 环境限制（WSL）

本项目依赖 Windows 原生 API（剪贴板、系统托盘、全局快捷键）。
在 WSL 中开发时，统一使用仓库内脚本转发到 Windows 工具链执行：

| 推荐命令 | 说明 |
|------|------|
| `./scripts/win-cargo test` | 使用 Windows `cargo.exe` 执行 Rust 测试 |

Rust 改动后，保证 `cargo test` 与 `cargo build -p floatpaste-native` 通过。

发布说明脚本的检查入口为 `node --test scripts/release-notes.test.mjs`。发版步骤、产物验收与失败恢复统一见 [发布流程](docs/release/process.md)。

## 调试与诊断

- 日志写入 `%APPDATA%\com.floatpaste\logs`，文件以 `floatpaste-native.log` 为前缀按天滚动，最多保留 15 个文件；主目录不可用时写入 `%TEMP%\FloatPaste\logs`。
- 默认不记录窗口标题或 UIA 元素名称；设置 `FLOATPASTE_DIAGNOSTIC_LOG=1` 后启用这些字段。共享诊断日志前检查其中的敏感信息。
- 窗口生命周期用 `session_id` 关联，上屏异步操作同时记录 `operation_id`；日志队列丢弃计数每 30 秒检查并在新增丢弃时报告，panic 日志包含线程名与堆栈。实现见 [system.rs](crates/floatpaste-native/src/system.rs)。
- 用「应用启动诊断」核对版本、PID、实际 exe 路径与敏感文本诊断开关；正常关闭后下次呼出的 `idle_ms` 表示实际闲置时长，首次呼出为空。
- 排查慢呼出时，对齐「命中全局快捷键 / 收到二次启动唤醒」、事件循环处理的 `queue_ms`、「速贴列表与主题准备完成」、`picker_preparation` 中的定位及烘焙耗时（`capture_ms` 为抓屏、`bake_ms` 为像素处理）、以及「速贴实际显示并接管输入」的 `elapsed_ms`；显示日志表示窗口移入屏内并完成输入装配，不保证 DWM 已合成首帧。准备回退的 `reason` 区分结果超过期限（`deadline`）、任务不可用（`worker_unavailable`）与结果通道断开（`worker_disconnected`）；任务忙与线程创建失败另有日志。准备期限与回退见 [窗口指南](docs/no-focus-picker.md)。
- 同步准备较慢时，按 `session_id` 对齐以下细分日志；耗时单位为毫秒，只记录阶段和数量，不记录条目正文：
  - 「速贴主题准备完成」的 `resolve_ms` / `apply_ms` 分别覆盖系统主题读取与 token 推导、向各窗口写入主题；「速贴屏外窗口恢复完成」的 `show_ms` 覆盖 Win32 无激活恢复。
  - 「速贴列表查询完成」的 `query_ms` 包含仓储锁等待和查询，`success` / `row_count` 区分失败与空列表；`keep_anchor` 区分会话重置与保留选中刷新。
  - 「速贴行模型准备完成」的 `geometry_ms` 为宽度和行高度量等准备，`preview_ms` 为所有行的文本度量与裁排累计，`projection_ms` 为其余行数据转换，`publish_ms` 为替换行模型和更新版本号；`row_count` 为本次重建行数。行模型日志也覆盖缩略图回填和宽度变化触发的重建。
  - 「速贴缩略图任务派发完成」的 `dispatch_ms` 只覆盖缓存检查与任务派发，不包含后台解码；「速贴后台准备派发完成」的 `setup_ms` 覆盖备用位置、材质参数和线程派发，`elapsed_ms` 仍从呼出入口累计。
  - 「速贴显屏阶段完成」的 `timer_wait_ms` 是安排显屏到定时回调执行的实际间隔，包含配置的 `configured_delay_ms` 和事件循环调度；`warm_ms` / `place_ms` / `overlay_ms` / `input_setup_ms` 分别覆盖表面预热、落位与重绘请求、焦点与置顶装配、置顶守护和输入会话装配。它们不表示 DWM 首帧耗时。
- 静默启动使用 `--silent`；二次启动通过单实例命名事件唤醒已有实例的速贴后退出。
- E2E 的隔离、日志保留和失败排查见 [测试体系](docs/testing.md)。真实快捷键分发、上屏注入、第三方应用兼容与视觉效果按改动范围在交互桌面验证。

## 代码与文档

- Rust 遵循 `rustfmt` 默认格式，模块与函数使用 `snake_case`，类型使用 `PascalCase`。
- 文档、提交信息与评审说明使用中文，领域命名与术语表一致，避免无关样式改动。
- 行为或结构变化时，同一改动更新对应活文档；文档职责与维护规则见 [文档地图](docs/README.md) 和 [AGENTS.md](AGENTS.md)。

## 提交与 Pull Request

- 提交以中文摘要为主，可带 `feat:`、`fix:`、`docs:` 等前缀，或直接使用简洁中文动宾句。
- PR 说明用户可见变化、涉及模块与关联问题，附自动检查结果；自动化未覆盖的相关行为提供手动验证步骤及结果。
- 界面改动附截图或录屏。
- 涉及数据库、快捷键或系统权限时，说明迁移与回退影响。
