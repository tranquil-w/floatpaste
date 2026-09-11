# Repository Guidelines

## 项目结构与模块组织

桌面端处于「Tauri 壳 → 原生 Slint 壳」迁移期，结构如下：

- `crates/floatpaste-core/` - 业务、数据与 Windows 平台共享核心。新业务逻辑一律落这里，两个壳共用
- `crates/floatpaste-native/` - 目标壳（Slint 软件渲染）。新窗口与新功能只在此实现
- `src-tauri/` - 旧 Tauri 壳（迁移期保留）：只做与 core 等价的修复和迁移收尾，不新增功能；与 core 同名的逻辑以 core 为唯一实现，壳内只允许写薄适配层
- `src/` - 旧壳前端（迁移期保留）：作为复刻对齐的行为规格，不新增功能。复刻验收通过后与 `src-tauri/` 一并删除，届时 `crates/floatpaste-native/` 是唯一桌面壳

旧壳目录细分（迁移期内仍适用）：

- `src/app/` - 应用壳与查询客户端
- `src/features/` - 资料库（manager）、速贴面板（picker）、搜索、编辑器、设置
- `src/bridge/` - Tauri 运行时与浏览器模拟的区分层
- `src/shared/` - 通用 UI、类型与工具
- `src-tauri/commands/` - Tauri 命令暴露层
- `src-tauri/services/` - 依赖 Tauri 窗口类型的壳层窗口服务（纯逻辑在 core）
- `src-tauri/platform/windows/` - 依赖 Tauri 窗口类型的平台适配
- `src-tauri/migrations/` - 数据库迁移

### 其他目录
- `docs/` - 架构文档
- `dist/` - 构建产物（请勿手动修改）

---

## 构建、测试与开发命令

| 命令 | 说明 |
|------|------|
| `pnpm install` | 安装前端与 Tauri CLI 依赖 |
| `pnpm dev` | 启动浏览器预览（使用 `mockBackend.ts`） |
| `pnpm tauri dev` | 启动桌面应用（连接真实 Rust 命令） |
| `pnpm build` | TypeScript 检查 + Vite 打包 |
| `pnpm lint` | ESLint 检查（`src/`） |
| `pnpm format` / `pnpm format:check` | Prettier 格式化写入 / 校验 |
| `pnpm test` | 前端单元测试（Node 内置 test runner） |
| `pnpm preflight` | 一次执行 lint + build + test + Rust 测试 |
| `pnpm tauri build` | 桌面应用打包 |
| `cargo test` | 运行 Rust 测试（在 `src-tauri/` 下执行） |

### 环境限制（WSL）

本项目为 Tauri 桌面应用，后端依赖 Windows 原生 API（剪贴板、系统托盘、全局快捷键）。
在 WSL 中开发时，默认不要直接依赖 WSL/Linux 工具链，统一优先使用仓库内脚本转发到 Windows 工具链执行。
若 Windows 环境已安装 `rtk.exe`，这些脚本会自动通过 `rtk` 包裹底层命令，以压缩终端输出。

| 推荐命令 | 说明 |
|------|------|
| `./scripts/win-pnpm install` | 使用 Windows `pnpm` 安装依赖 |
| `./scripts/win-pnpm dev` | 使用 Windows `pnpm dev` 启动浏览器预览 |
| `./scripts/win-pnpm build` | 使用 Windows 前端工具链执行构建检查 |
| `./scripts/win-pnpm tauri dev` | 使用 Windows Tauri 工具链启动桌面应用 |
| `./scripts/win-pnpm tauri build` | 使用 Windows Tauri 工具链执行桌面构建 |
| `./scripts/win-cargo test` | 使用 Windows `cargo.exe` 执行 Rust 测试 |

如需在 Windows 命令行中执行，也可以使用等价的 npm scripts：

| 命令 | 说明 |
|------|------|
| `pnpm install:win` | Windows 侧安装依赖 |
| `pnpm dev:win` | Windows 侧启动前端预览 |
| `pnpm build:win` | Windows 侧前端构建 |
| `pnpm tauri:dev:win` | Windows 侧桌面调试 |
| `pnpm tauri:build:win` | Windows 侧桌面构建 |
| `pnpm test:rust:win` | Windows 侧 Rust 测试 |

前端改动后，保证 `pnpm lint`、`pnpm format:check`、`pnpm build`、`pnpm test` 全部通过（在 WSL 中用 `./scripts/win-pnpm build` 等脚本执行，lint/format 可直接用 WSL 侧 pnpm 跑）。
涉及 Rust 或 Tauri 改动时，优先执行 `./scripts/win-cargo test`，并按需要补充 `./scripts/win-pnpm tauri dev` 或 `./scripts/win-pnpm tauri build`。

---

## 代码风格与命名约定

### 前端
- 缩进：2 空格
- 引号：双引号
- 组件命名：`PascalCase`
- 函数、状态、查询工具：`camelCase`
- 按功能目录拆分文件
- 运行时分支优先通过 `bridge/` 封装，避免在组件中散落环境判断

### Rust
- 遵循 `rustfmt` 默认格式
- 模块与函数：`snake_case`
- 类型：`PascalCase`

### 工具配置
- ESLint 与 Prettier 均已配置，脚本见 `package.json`
- 提交钩子（husky + lint-staged）会对暂存的 `src/**/*.{ts,tsx}` 自动执行 Prettier
- 提交信息与文档保持中文，避免无关样式改动

---

## 测试指南

### 前端
- 单元测试基于 Node 内置 test runner：`pnpm test`
- 每次改动至少执行 `pnpm build` 与 `pnpm test` 验证

### Rust
- 建议为逻辑变更补充单元测试
- 测试可就近写在模块内，或放入 `src-tauri/tests/`
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
