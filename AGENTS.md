# Repository Guidelines

## 项目结构与模块组织
`src/` 是 React + Vite 前端。`app/` 放应用壳与查询客户端，`features/manager` 和 `features/picker` 分别对应资料库与速贴面板，`bridge/` 负责区分 Tauri 运行时与浏览器模拟，`shared/` 存放通用 UI、类型与工具。`src-tauri/` 是 Rust 后端：`commands/` 暴露 Tauri 命令，`services/` 承载业务逻辑，`repository/` 处理 SQLite，`platform/windows/` 放 Windows 集成，`migrations/` 保存数据库迁移。`docs/` 存架构文档，`dist/` 为构建产物，不要手改。

## 构建、测试与开发命令
`npm install` 安装前端与 Tauri CLI 依赖。`npm run dev` 启动浏览器预览，此模式会走 `src/bridge/mockBackend.ts`。`npm run tauri -- dev` 启动桌面应用并连接真实 Rust 命令。`npm run build` 先执行 TypeScript `--noEmit` 检查，再产出 Vite 包。需要桌面打包时使用 `npm run tauri -- build`。新增 Rust 测试后，在 `src-tauri/` 下运行 `cargo test`。

## 代码风格与命名约定
前端沿用现有风格：2 空格缩进、双引号、组件使用 `PascalCase`，普通函数、状态与查询工具使用 `camelCase`，按功能目录拆分文件。Rust 代码遵循 `rustfmt` 默认格式，模块与函数用 `snake_case`，类型用 `PascalCase`。仓库目前未配置 ESLint 或 Prettier，提交前请保持既有格式，避免无关样式改动。涉及运行时分支时，优先经由 `bridge/` 封装，不要在功能组件里直接散落环境判断。

## 测试指南
当前仓库没有独立前端测试框架，也没有专门的 `tests/` 目录，因此每次改动至少执行 `npm run build`。Rust 逻辑变更建议补充单元测试，可就近写在模块内，或放入 `src-tauri/tests/`。测试名应描述行为，例如 `ingest_text_skips_self_write`。

## 提交与 Pull Request 规范
现有提交历史以中文摘要为主，可带简短前缀，例如 `doc: 架构设计与MVP技术方案`。建议使用 `feat:`、`fix:`、`doc:` 等前缀加中文说明，或直接使用简洁中文动宾句。PR 应说明用户可见变化、涉及模块、手动验证步骤，并关联相关文档或问题；界面改动请附截图或录屏。若修改数据库、快捷键或系统权限行为，请在描述中写清迁移与回退影响。

## Agent 协作说明
仓库内代理协作要求统一使用中文；生成文档、提交信息与评审说明时也保持中文。
