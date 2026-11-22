# 搜索窗口

搜索窗口是 FloatPaste 的正常获焦检索窗口：从托盘菜单或全局搜索快捷键打开，按内容密度收缩高度（顶边锚定），按 `Esc` 关闭后将系统前台焦点归还原目标窗口，并支持两段式删除。

## 子功能

- `search-open-onscreen`：经命名事件触发与托盘「打开搜索」相同的入口，搜索窗口移入屏幕并可见。
- `search-esc-park-and-focus`：在搜索窗口活跃态触发关闭（等价于 `Esc` 经 `FocusScope` 回调），窗口退回屏外停屏，恢复原会话前台窗口。
- `search-two-stage-delete`：搜索窗口选中条目下连续两次删除触发软删（单次仅进入待确认态）。

## 用户入口（用户视角）

- 右键托盘图标 → 点击「打开搜索」。
- 按下全局「搜索窗口」快捷键。
- 窗口内部按 `Esc` 键关闭（或在删除按钮 / `Delete` 键上确认两段式删除）。

## 驱动配方

前置条件：

- 隔离实例已通过 `doctor`，当前处于可交互桌面（非锁屏）。

- **打开搜索窗口并截图。** 运行 `fp_verify.py signal open-search`，再运行 `fp_verify.py windows --wait-role search --expect-state onscreen --timeout 10`。断言返回 `matched.onscreen == true`、`matched.rect.width > 0`、`matched.rect.height > 0`；等待 400ms 后运行 `fp_verify.py capture --role search --out search-window/search-open.png --feature search-window --entry-point "TestOpenSearch event" --action "open search window"`，并读取 PNG 核对界面呈现。
- **关闭搜索窗口并退回屏外。** 运行 `fp_verify.py signal search-escape`，随后运行 `fp_verify.py windows --wait-role search --expect-state parked --timeout 10`。断言 `matched.onscreen == false` 且 `matched.rect.left <= -30000`。

## 注意点与已知边界

- 未打开时 winit 预建窗口或双实例并存时可能存在同标题停屏窗口，必须使用带 Session PID 过滤的 `fp_verify.py windows`。
- `TestOpenSearch` 与 `TestSearchEscape` 分别走托盘动作终点和 `FocusScope` `key-close` 回调终点，绕过了托盘模态 `TrackPopupMenu` 与物理按键分发；关键词实时筛选（含中文 FTS5）、仅收藏与类型/用途组合、搜索行内「更多动作」弹层等由 `l2_headless_ui_contracts` 与核心库 L1 集成测试覆盖，真实输入法输入与全局热键冲突（如 1409）仍需交互桌面补充核查。
- `search-delete` 与速贴共享 3 秒两段式删除逻辑；测试后注意先确认隔离库至少有 1 条活跃条目再驱动搜索删除。
