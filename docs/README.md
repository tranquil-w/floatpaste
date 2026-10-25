# 文档地图

按要解决的问题查找文档；文件平铺于 `docs/`，发布流程与模板放 `release/`。维护规则见 [AGENTS.md](../AGENTS.md)。

## 按任务进入

先定位本次问题，再读取相关章节；各入口负责导航，不要求把所有专题作为每次任务的必读材料。

| 当前任务 | 阅读顺序 |
| --- | --- |
| 实现功能、修复缺陷或重构 | [贡献指南](../CONTRIBUTING.md) 核实环境与命令 → [架构概览](architecture.md) 定位入口与共享调用 → [运行行为](runtime-behavior.md) 及下方对应专题核对契约 → [测试体系](testing.md) 选择验证层与隔离方式 |
| 修改窗口、交互或观感 | 架构中的窗口职责 → [窗口时序](no-focus-picker.md)、[编辑窗口](editor-window.md) 或 [主题系统](theme-system.md) 中受影响部分 → 测试体系中的相关 UI 与桌面验收约束 |
| 修改或整理文档 | [协作约定的文档规则](../AGENTS.md#文档规则) → 下表定位主要维护位置 → 核对实现、测试或已认可契约；使用 [术语表](../CONTEXT.md) 统一命名 |
| 理解决策为何如此 | 先查对应活文档确认当前行为，再查下方 ADR；历史决定不自动替代现行规格 |
| 发布或修改发布流程 | 先读 [发布流程](release/process.md)，再读本次涉及的测试与交付约束 |

## 开始使用与参与开发

| 需要做什么 | 入口 |
|------------|------|
| 安装与开始使用 | [产品 README](../README.md) |
| 准备环境、运行与贡献 | [开发与贡献](../CONTRIBUTING.md) |
| 核对必需检查、自审与交付证据 | [验证与交付](../CONTRIBUTING.md#验证与交付) / [交付前自审](../CONTRIBUTING.md#交付前自审) |
| 查看日志与诊断开关 | [调试与诊断](../CONTRIBUTING.md#调试与诊断) |
| 排查慢呼出与显屏 | [呼出缓慢与显屏延迟](no-focus-picker.md#呼出缓慢与显屏延迟) |
| 理解领域命名 | [术语表](../CONTEXT.md) |
| 定位模块与依赖关系 | [架构概览](architecture.md) |

## 当前行为与维护

| 文档 | 维护范围 |
|------|----------|
| [运行行为与数据约定](runtime-behavior.md) | 监听入库、富文本格式与快照恢复、搜索排序、上屏、跨窗口数据、自启提权、存储与数据目录迁移 |
| [无焦点速贴指南](no-focus-picker.md) | 窗口生命周期、焦点、定位、快捷键设置与升级、更多操作与单条删除、悬浮气泡预览与元信息、Win32 防坑 |
| [主题系统](theme-system.md) | 色彩 token、对比度、材质消费约束与新增预设 |
| [编辑窗口规格](editor-window.md) | 标签面板、编辑滚动、保存与关闭行为 |
| [测试体系](testing.md) | 测试分工、覆盖边界、隔离驱动与失败排查 |
| [发布流程](release/process.md) | tag 发布、安装升级与卸载行为、自动验收、说明生成与失败恢复 |
| [发布说明模板](release/notes-template.md) / [使用说明模板](release/usage-template.md) | 流水线引用的对外交付模板 |

## 决策依据

ADR 保留决策时的背景、备选方案与后果；查当前行为时使用对应活文档，决策状态以文件头为准。

| 决策记录 | 当前行为入口 |
|----------|--------------|
| [ADR-0001：Win+V 注册表接管](adr-0001-winv-takeover-via-registry.md) | [运行行为与数据约定](runtime-behavior.md) |
| [ADR-0002：固定显示与停靠](adr-0002-picker-docked-mode.md) | [窗口时序与防坑](no-focus-picker.md) |
| [ADR-0003：编辑窗口选型](adr-0003-editor-window-redesign.md) | [编辑窗口规格](editor-window.md) |
| [ADR-0005：窗口材质路线](adr-0005-window-material.md) | [主题系统](theme-system.md) / [窗口时序与防坑](no-focus-picker.md) |
