# Repository Guidelines

## 项目结构与模块组织

### 前端 (`src/`)
- `app/` - 应用壳与查询客户端
- `features/manager/` - 资料库
- `features/picker/` - 速贴面板
- `bridge/` - Tauri 运行时与浏览器模拟的区分层
- `shared/` - 通用 UI、类型与工具

### 后端 (`src-tauri/`)
- `commands/` - Tauri 命令暴露层
- `services/` - 业务逻辑
- `repository/` - SQLite 数据访问
- `platform/windows/` - Windows 平台集成
- `migrations/` - 数据库迁移

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
| `pnpm tauri build` | 桌面应用打包 |
| `cargo test` | 运行 Rust 测试（在 `src-tauri/` 下执行） |

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
- 目前未配置 ESLint 或 Prettier
- 提交前保持既有格式，避免无关样式改动

---

## 测试指南

### 前端
- 暂无独立测试框架
- 每次改动至少执行 `pnpm build` 验证

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
- 示例：`doc: 架构设计与MVP技术方案`

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
