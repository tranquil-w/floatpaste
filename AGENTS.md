# Repository Guidelines

## 开发入口

- 开发环境、命令、代码风格和贡献要求遵循 [CONTRIBUTING.md](CONTRIBUTING.md)。
- 实现前按改动范围阅读 [架构概览](docs/architecture.md)、[运行行为](docs/runtime-behavior.md) 与 [文档地图](docs/README.md) 中的对应专题。
- Rust 改动执行贡献指南要求的测试与构建；测试分层、隔离与驱动钩子遵循 [测试体系](docs/testing.md)。
- 执行版本发布或修改发布流程前，阅读并遵循 [发布流程](docs/release/process.md)。

## 文档规则

- `docs/` 保留有明确当前用途且具备持续维护价值的文档；内容失真时先修正，重复时合并独有事实，职责不符时迁移，没有保留价值时删除。
- 文档平铺于 `docs/`，文件名使用英文 kebab-case；仅 `release/` 保留流程与模板子目录。计划、路线图、候选功能与调研草稿放 `.artifacts/`，不入 Git。
- 文档与界面文案统一使用 [CONTEXT.md](CONTEXT.md) 术语；提交行为或结构变更时，同一改动更新对应活文档并修复入站链接和文档地图。
- 架构概览维护模块边界、依赖方向与主要数据流；现行行为和防坑细节放对应专题。相同事实指定一个维护位置，其他入口用摘要和链接。
- 规范类文档只写应遵循的规则及理由；操作历史放对话或提交记录。故障排查区分症状、证据、假设、恢复条件，不把单次观察写成普遍保证。
- ADR 立项需同时满足难以逆转、无上下文会费解、存在真实取舍；小范围设计理由就近写在架构或专题文档中。
- ADR 平铺编号为 `docs/adr-NNNN-<slug>.md`，状态写 frontmatter：`proposed` / `accepted` / `rejected` / `superseded by adr-NNNN`。proposed 随讨论完善；accepted 保留当时正文，后续决策变化通过新 ADR 与状态关联表达；rejected 和 superseded 原位保留。
- 现行约束以活文档和实现为准，ADR 用于解释决策依据；在文档地图中关联两者。迁移或精简前核对独有事实、前提、例外、时序与回退条件的去向。

## Agent 协作

- 仓库内代理协作、生成文档、提交信息与评审说明统一使用中文。
