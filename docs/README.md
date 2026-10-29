# 文档地图

按要解决的问题查找文档；文件平铺于 `docs/`，发布流程与模板放 `release/`。维护规则见 [AGENTS.md](../AGENTS.md)。

## 按任务进入

选择最贴近任务的一行，只读相关章节；每格列出按用途选择的入口，不要求通读整篇或依次完成所有行。仅在发现共享调用、契约冲突或证据缺口时扩展阅读。

| 当前任务 | 按需入口与用途 |
| --- | --- |
| 实现功能、修复缺陷或重构 | [架构概览](architecture.md#改动入口) 定位相关模块与共享调用；按行为归属查 [运行行为](runtime-behavior.md) 或下方专题；[贡献指南](../CONTRIBUTING.md#验证与交付) 核实命令与门槛，[测试体系](testing.md) 选择相关验证层与隔离方式 |
| 修改窗口、交互或观感 | [窗口职责](architecture.md#窗口职责) 定位入口；选择 [窗口时序](no-focus-picker.md)、[编辑窗口](editor-window.md) 或 [主题系统](theme-system.md) 的受影响章节；视觉问题先看 [取证与验收](testing.md#缺陷复现与验收)，验证门槛见 [贡献指南](../CONTRIBUTING.md#验证与交付) |
| 修改或整理文档 | [协作约定的文档规则](../AGENTS.md#文档规则) → 下表定位主要维护位置 → 核对实现、测试或已认可契约；使用 [术语表](../CONTEXT.md) 统一命名 |
| 执行或维护自动检查与 CI | [统一检查入口](../CONTRIBUTING.md#统一检查入口) 核对范围、结果报告与脚本回归；[测试体系](testing.md) 核对桌面覆盖边界及失败排查 |
| 理解决策为何如此 | 先查对应活文档确认当前行为，再查下方 ADR；历史决定不自动替代现行规格 |
| 发布或修改发布流程 | 先读 [发布流程](release/process.md)，再读本次涉及的测试与交付约束 |

## 开始使用与参与开发

| 需要做什么 | 入口 |
|------------|------|
| 安装与开始使用 | [产品 README](../README.md) |
| 准备环境、运行与贡献 | [开发与贡献](../CONTRIBUTING.md) |
| 核对必需检查、自审与交付证据 | [验证与交付](../CONTRIBUTING.md#验证与交付) / [交付前自审](../CONTRIBUTING.md#交付前自审) |
| 一条命令执行检查、生成证据报告 | [统一检查入口](../CONTRIBUTING.md#统一检查入口) |
| 查看日志与诊断开关 | [调试与诊断](../CONTRIBUTING.md#调试与诊断) |
| 排查慢呼出与显屏 | [呼出缓慢与显屏延迟](no-focus-picker.md#呼出缓慢与显屏延迟) |
| 理解领域命名 | [术语表](../CONTEXT.md) |
| 定位模块与依赖关系 | [架构概览](architecture.md) |

## 当前行为与维护

| 文档 | 维护范围 |
|------|----------|
| [运行行为与数据约定](runtime-behavior.md) | 监听入库、采集隐私与输入上限、锁屏取消、富文本与快照恢复、搜索排序、上屏与文本文件附件、自启提权、保留清除与数据目录迁移 |
| [无焦点速贴指南](no-focus-picker.md) | 窗口生命周期、焦点、定位、快捷键设置与升级、外部操作退出、更多操作与单条删除、悬浮气泡预览与元信息、Win32 防坑 |
| [主题系统](theme-system.md) | 色彩 token、对比度、材质消费约束与新增预设 |
| [编辑窗口规格](editor-window.md) | 标签面板、编辑滚动、首帧显示、保存与关闭行为 |
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
