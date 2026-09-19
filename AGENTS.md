# Repository Guidelines

## 项目结构与模块组织

- `crates/floatpaste-core/` - 业务、数据与 Windows 平台共享核心。不依赖 Slint 的业务与纯逻辑一律落这里
- `crates/floatpaste-native/` - 唯一桌面壳（Slint 软件渲染）。窗口、会话与回调装配在此实现

core 内部分层约束：

- `domain` - 纯数据与业务概念，不依赖任何基础设施
- `repository` - rusqlite 数据访问
- `services` - 业务规则编排，仅依赖 domain/repository
- `platform` - Win32 原生集成（剪贴板、监听、快捷键、单实例等），通过回调向 UI 层暴露事件，不反向依赖任何窗口框架

native 壳内约定：

- `src/` 各窗口模块自带 `wire()` 绑定回调，`main.rs` 只负责启动顺序与模块间接线
- 不依赖 Slint 类型的纯逻辑（格式化、计算）下沉 core（如 `core::clip_display`），保证无 GUI 依赖即可测试

### 其他目录
- `docs/` - 文档，平铺（仅 `release/` 保留子目录）；地图见 [docs/README.md](docs/README.md)
- `CONTEXT.md` - 领域术语表，术语以这里为准
- `packaging/` - Inno 安装包脚本
- `scripts/` - 版本号、发版说明、图标生成等辅助脚本
- `.artifacts/` - 迭代计划、发版草稿等临时产物（不入 Git）

---

## 文档规则

- docs/ 只保留当前有价值且可预见长期有参考价值的文档，失去价值即移除（历史在 git 提交记录中）；计划、路线图、调研草稿等临时物放 `.artifacts/`，不进 Git
- 提交行为或结构变更时，同一改动内更新对应活文档（architecture.md / no-focus-picker.md / theme-system.md），文档与实现不一致视同 bug
- 设计与决策文档平铺编号为 `docs/adr-NNNN-<slug>.md`，状态写文件头部 frontmatter：`proposed`（调研/提案中）/ `accepted` / `rejected` / `superseded by adr-NNNN`；状态变更只改头不改正文，被拒方案原位保留
- 立项写 adr 需同时满足：难以逆转、无上下文会费解、存在真实取舍；三条缺一就不值得单独立项
- 文档与界面文案统一使用 `CONTEXT.md` 术语表的命名

---

## 构建、测试与开发命令

| 命令 | 说明 |
|------|------|
| `cargo run -p floatpaste-native` | 启动桌面应用（debug） |
| `cargo test` | 运行全部 Rust 测试（覆盖 `floatpaste-core` 与 `floatpaste-native`） |
| `cargo build --release -p floatpaste-native` | 发布构建 |
| `node scripts/bump-version.mjs <版本>` | 同步升级 package.json / native Cargo.toml / Cargo.lock |

`package.json` 仅保留版本号（发版 tag 校验用）与 `test:rust` 等价入口，不含任何依赖。

### 环境限制（WSL）

本项目依赖 Windows 原生 API（剪贴板、系统托盘、全局快捷键）。
在 WSL 中开发时，统一使用仓库内脚本转发到 Windows 工具链执行：

| 推荐命令 | 说明 |
|------|------|
| `./scripts/win-cargo test` | 使用 Windows `cargo.exe` 执行 Rust 测试 |

Rust 改动后，保证 `cargo test` 与 `cargo build -p floatpaste-native` 通过。

---

## 代码风格与命名约定

### Rust
- 遵循 `rustfmt` 默认格式
- 模块与函数：`snake_case`
- 类型：`PascalCase`
- 提交信息与文档保持中文，避免无关样式改动

---

## 测试指南

- 建议为逻辑变更补充单元测试
- 测试就近写在模块内，或放入所属 crate 的 `tests/` 目录
- 测试名应描述行为，例如：`ingest_text_skips_self_write`

---

## 提交与 Pull Request 规范

### 提交信息
- 以中文摘要为主，可带简短前缀
- 推荐前缀：`feat:`、`fix:`、`doc:` 等
- 或直接使用简洁中文动宾句
- 示例：`doc: 重构剪贴板监听模块`

### PR 描述
- 说明用户可见变化
- 列出涉及模块
- 提供手动验证步骤
- 关联相关文档或问题
- 界面改动请附截图或录屏
- 若修改数据库、快捷键或系统权限，需说明迁移与回退影响

---

## Agent 协作说明

- 仓库内代理协作统一使用中文
- 生成文档、提交信息与评审说明时保持中文
