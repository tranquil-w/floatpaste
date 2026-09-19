---
status: accepted
---

# Win+V 接管走注册表释放路线

系统 Win+V 被 TextInputHost.exe 抢先注册，普通进程 RegisterHotKey 必然失败（1409），无法直接覆盖。决定提供「接管 Win+V」一键开关：警告后写注册表 `HKCU\...\Explorer\Advanced\DisabledHotkeys` 释放该组合，注销/重启后自动注册，关闭开关即恢复原热键并清除注册表值，全程可逆。

## Considered Options

- **低级键盘钩子（WH_KEYBOARD_LL）硬拦**：拒绝。前台为高完整性进程时收不到按键（与管理员上屏能力冲突）、响应过慢会被系统摘钩、与其他钩子类工具互斥。
- **仅注册失败提示、不提供接管**：拒绝。用户明确要求一键开启（含副作用警告与可回退）。

## Consequences

- 生效与回退都只需重启 Explorer（真机 Windows 11 26100 实测）：写入 `DisabledHotkeys=V` 后重启 Explorer，`RegisterHotKey(Win+V)` 立即成功；删除该值并重启 Explorer 后系统恢复持有（1409）。无需注销或重启系统，实施时可提供「重启 Explorer」的引导按钮（任务栏会短暂消失，需用户确认）。
- 系统剪贴板历史**保持开启**亦可成功释放并注册 Win+V（同机同轮实测，`EnableClipboardHistory=1`），接管向导无需引导用户关闭系统历史。
- 未重启 Explorer 前，开关状态与实际注册状态不一致（用户已开启接管但 Win+V 仍归系统）：启动注册失败时在设置页给出「需重启 Explorer 生效」提示，而非视为错误。
- 调研依据：RegisterHotKey 官方文档（Win 组合保留声明）、CopyQ #1668/#2422、Ditto #143、本仓库 2026-09-19 真机 spike（`.artifacts/winv-spike/`）。
