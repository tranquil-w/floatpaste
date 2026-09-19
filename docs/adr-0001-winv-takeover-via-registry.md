---
status: accepted
---

# Win+V 接管走注册表释放路线

系统 Win+V 被 TextInputHost.exe 抢先注册，普通进程 RegisterHotKey 必然失败（1409），无法直接覆盖。决定提供「接管 Win+V」一键开关：警告后写注册表 `HKCU\...\Explorer\Advanced\DisabledHotkeys` 释放该组合，注销/重启后自动注册，关闭开关即恢复原热键并清除注册表值，全程可逆。

## Considered Options

- **低级键盘钩子（WH_KEYBOARD_LL）硬拦**：拒绝。前台为高完整性进程时收不到按键（与管理员上屏能力冲突）、响应过慢会被系统摘钩、与其他钩子类工具互斥。
- **仅注册失败提示、不提供接管**：拒绝。用户明确要求一键开启（含副作用警告与可回退）。

## Consequences

- 需注销/重启才生效，开关状态与实际注册状态可能短暂不一致（启动时校验并提示）。
- CopyQ 的实证场景是「系统剪贴板历史已关闭」；实施前需真机 spike 验证「历史开启 + DisabledHotkeys」组合，若不释放则向导需加关闭系统历史的引导步骤。
- 调研依据：RegisterHotKey 官方文档（Win 组合保留声明）、CopyQ #1668/#2422、Ditto #143。
