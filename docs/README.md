# 文档地图

按要解决的问题查找文档；文件平铺于 `docs/`，发布流程与模板放 `release/`。维护规则见 [AGENTS.md](../AGENTS.md)。

## 开始使用与参与开发

| 需要做什么 | 入口 |
|------------|------|
| 安装与开始使用 | [产品 README](../README.md) |
| 准备环境、运行、验证、贡献与诊断 | [开发与贡献](../CONTRIBUTING.md) |
| 理解领域命名 | [术语表](../CONTEXT.md) |
| 定位模块与依赖关系 | [架构概览](architecture.md) |

## 当前行为与维护

| 文档 | 维护范围 |
|------|----------|
| [运行行为与数据约定](runtime-behavior.md) | 监听入库、富文本格式与快照恢复、搜索排序、上屏、跨窗口数据、自启提权与存储 |
| [无焦点速贴指南](no-focus-picker.md) | 窗口生命周期、焦点、定位、会话键与 Win32 防坑 |
| [主题系统](theme-system.md) | 色彩 token、对比度、材质消费约束与新增预设 |
| [编辑窗口规格](editor-window.md) | 标签面板、编辑滚动、保存与关闭行为 |
| [测试体系](testing.md) | 测试分工、覆盖边界、隔离驱动与失败排查 |
| [发布流程](release/process.md) | tag 发布、自动验收、说明生成与失败恢复 |
| [发布说明模板](release/notes-template.md) / [使用说明模板](release/usage-template.md) | 流水线引用的对外交付模板 |

## 决策依据

ADR 保留决策时的背景、备选方案与后果；查当前行为时使用对应活文档，决策状态以文件头为准。

| 决策记录 | 当前行为入口 |
|----------|--------------|
| [ADR-0001：Win+V 注册表接管](adr-0001-winv-takeover-via-registry.md) | [运行行为与数据约定](runtime-behavior.md) |
| [ADR-0002：固定显示与停靠](adr-0002-picker-docked-mode.md) | [窗口时序与防坑](no-focus-picker.md) |
| [ADR-0003：编辑窗口选型](adr-0003-editor-window-redesign.md) | [编辑窗口规格](editor-window.md) |
| [ADR-0005：窗口材质路线](adr-0005-window-material.md) | [主题系统](theme-system.md) / [窗口时序与防坑](no-focus-picker.md) |
