# 开发与贡献

本文提供开发环境、常用命令、验证方式和贡献要求。理解模块关系从 [架构概览](docs/architecture.md) 开始；术语使用 [CONTEXT.md](CONTEXT.md) 的命名。

## 环境与常用命令

使用 Windows 10/11 x64 和 Rust MSVC 工具链（含 C++ 构建工具与 Windows SDK）。发布说明等 Node 脚本使用 Node.js 22，无需安装 npm 依赖。

| 命令 | 说明 |
|------|------|
| `cargo run -p floatpaste-native --bin floatpaste` | 启动桌面应用（debug） |
| `cargo test` | 运行全部 Rust 测试（覆盖 `floatpaste-core` 与 `floatpaste-native`，含端到端） |
| `cargo test -p floatpaste-native --test e2e_smoke` | 单跑真进程端到端（运行十余秒；期间短暂占用全局快捷键并读写系统剪贴板；体系见 [docs/testing.md](docs/testing.md)） |
| `cargo build -p floatpaste-native` | 独立构建原生程序（debug），与测试构建分别核对 |
| `cargo build --release -p floatpaste-native` | 发布构建 |
| `cargo run -p floatpaste-native --bin picker-snapshot -- <light\|dark> <tag> [hover-y] [blur]` | 无头渲染速贴观感快照到 `.artifacts/snapshot/`（诊断工具，不模拟桌面输入） |

`package.json` 仅保留版本号（发版 tag 校验用）与 `test:rust` 等价入口，不含任何依赖。快照工具的完整参数见 [picker-snapshot.rs](crates/floatpaste-native/src/bin/picker-snapshot.rs) 头部说明，验收场景见 [测试体系](docs/testing.md)。

### 环境限制（WSL）

本项目依赖 Windows 原生 API（剪贴板、系统托盘、全局快捷键）。在 WSL 中开发时，用 `./scripts/win-cargo <cargo 子命令与参数>` 把命令转发到 Windows 工具链执行（如 `./scripts/win-cargo test`）。

## 验证与交付

Rust 改动须通过 `cargo fmt --all -- --check`、`cargo test` 与 `cargo build -p floatpaste-native`。测试分层、共享资源串行、隔离钩子和失败排查遵循 [测试体系](docs/testing.md)；针对当前改动补足对应风险证据，不能用局部检查替代必需门槛。

| 改动范围 | 需要核对的证据 |
| --- | --- |
| 规则、状态、持久化与资源生命周期 | 在合适的单元或 L1 层核对正常、边界及失败条件；服务契约经过公开业务入口 |
| 窗口、交互与观感 | 相关 L2 契约与截图；焦点、真实按键、上屏注入、材质和第三方接收按影响在交互桌面验证 |
| 发布说明脚本与模板 | `node --test scripts/release-notes.test.mjs`；其他发布流程改动按发布指南验收 |
| 仅文档或导航 | 核对事实、命令来源、路径、入站引用和读者阅读路径；不因纯文档修改强制运行 Rust 测试或构建 |

日常 CI 用锁定依赖执行与本地相同的三项门槛；E2E 以严格模式运行，不放过跳过主要业务段，焦点段跳过单独报告，失败实例日志作为 Artifact 保留。发版步骤、交付程序验收与失败恢复统一见 [发布流程](docs/release/process.md)。

交付说明对应实际文件、版本或程序，区分实现进展与验收结果；必要覆盖缺失时说明原因和剩余动作。具体结果状态与授权边界遵循 [协作约定](AGENTS.md#执行与验证)。

## 调试与诊断

- 日志写入 `%APPDATA%\floatpaste\logs`，文件以 `floatpaste-native.log` 为前缀按天滚动，最多保留 15 个文件；主目录不可用时写入 `%TEMP%\FloatPaste\logs`。
- 默认不记录窗口标题或 UIA 元素名称；设置 `FLOATPASTE_DIAGNOSTIC_LOG=1` 后启用这些字段，耗时日志只记录阶段与数量。共享诊断日志前检查其中的敏感信息。
- 窗口生命周期用 `session_id` 关联，上屏异步操作同时记录 `operation_id`；日志队列丢弃计数每 30 秒检查并在新增丢弃时报告，日志基建见 [system.rs](crates/floatpaste-native/src/system.rs)。panic 日志包含线程名与堆栈，钩子在 [main.rs](crates/floatpaste-native/src/main.rs)。
- 「应用启动诊断」记录版本、PID、实际 exe 路径与敏感文本诊断开关，用于核对提权接管后实际运行的程序与开关状态；面板隐藏后下次呼出的 `idle_ms` 表示实际闲置时长，首次呼出为空。
- 静默启动使用 `--silent`；二次启动通过单实例命名事件唤醒已有实例的速贴后退出。

## 代码与文档

- Rust 遵循 `rustfmt` 默认格式，模块与函数使用 `snake_case`，类型使用 `PascalCase`。
- 文档语言、领域术语与同步维护要求遵循 [协作约定](AGENTS.md)；按 [文档地图](docs/README.md) 找到对应位置，避免无关样式改动。
- 本文档维护环境、命令、验证门槛与贡献流程；实现新增的行为细节、日志字段与配置项在代码注释或对应专题文档就近说明，不随实现提交在本文件逐项登记。

## 提交与 Pull Request

- 操作前查看 `git status --short`、`git diff` 和 `git diff --cached`，确认工作区与暂存区中的改动归属。提交时用 `git add -- <本任务文件路径>` 明确选择文件，并检查暂存 diff；同一文件混有其他任务修改时按 hunk 暂存。未获授权，不提交已有的其他修改。
- 混合工作区不使用 `git add .`、`git add -A`、`git commit -a`，也不执行整仓恢复、清理或 stash（如 `git reset --hard`、`git restore .`、`git checkout -- .`、`git clean -fd`、`git stash`）；不要为方便验证而收走或丢弃协作者的修改。
- 评审先使用 `git diff`、`git show` 等只读方式；除非任务需要切换检出，不为查看另一个分支或 PR 直接切换当前工作树。
- 提交以中文摘要为主，可带 `feat:`、`fix:`、`docs:` 等前缀，或直接使用简洁中文动宾句。
- PR 说明用户可见变化、涉及模块与关联问题，附自动检查结果；自动化未覆盖的相关行为提供手动验证步骤及结果。
- 界面改动附截图或录屏。
- 涉及数据库、快捷键或系统权限时，说明迁移与回退影响。
