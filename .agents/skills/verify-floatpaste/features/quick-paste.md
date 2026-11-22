# 速贴面板

速贴是 FloatPaste 的无焦点快速面板：冷启动时应安静停靠在屏外不闪现，唤醒后在不抢占目标应用焦点的前提下置顶呈现，支持收藏、两段式删除与上屏动作。

## 子功能

- `picker-cold-silent`：冷启动完成窗口装配，所有业务窗口停在屏外不闪现，自动写入欢迎条目并收藏。
- `picker-second-launch-wake`：通过二次启动单实例唤醒（或 `wake` 命名事件）呼出速贴，保持 `WS_EX_TOPMOST` 且不夺取前台焦点。
- `picker-favorite-toggle`：在速贴活跃会话内切换当前选中条目的收藏状态，并同步持久化到 SQLite。
- `picker-two-stage-delete`：首次触发删除进入待确认态（条目未删），3 秒内再次触发才执行软删。

## 用户入口（用户视角）

- 全局主快捷键打开/关闭速贴（或开启「Win+V 接管」后按 `Win+V`）。
- 托盘图标 / 再次运行 `floatpaste.exe`（二次启动单实例唤醒）。
- 速贴会话内按收藏会话键（默认 `Ctrl+D`）或点击行内「⋯」更多操作菜单。
- 速贴会话内按删除键（默认 `Delete`，3 秒内连按两次）或点击行内「⋯」确认删除。

## 驱动配方

前置条件：

- 已通过 `fp_verify.py launch --diagnostic-log --picker-pos 80,80` 启动隔离实例且 `cold_start_rogue_windows` 为空（在 `exebox-*` 沙盒中与后续 `windows`/`capture` 同在一次 Shell 调用内运行）。
- `fp_verify.py doctor` 通过；运行 `db --mode items` 与 `db --mode summary` 记录第 0 行（当前选中项）初始收藏状态 `fav0`、活跃条目数 `active0` 与已删除条目数 `deleted0`。

- **记录原前台与二次启动唤醒。** 执行 `fp_verify.py foreground` 记录当前前台 PID；再运行 `fp_verify.py signal wake --via-second-launch`，等待其退出码 `0`。紧接着运行 `fp_verify.py windows --wait-role picker --expect-state onscreen --timeout 10`。断言返回 `matched.onscreen == true`、`matched.topmost == true`，且再次执行 `fp_verify.py foreground` 确认前台未切到隔离实例 PID。
- **实机画面取证。** 等待 500ms 让异步列表渲染就绪后，运行 `fp_verify.py capture --role picker --out quick-paste/picker-open.png --feature quick-paste --entry-point "second-launch wake" --action "open quick-paste"`。断言 `pixel_stats.all_black == false`，随后读取生成 PNG 确认窗口边框、条目文本与 DPI 清晰无空白。
- **收藏切换与数据库副作用。** 运行 `fp_verify.py signal picker-favorite`；等待 400ms 后重查 `fp_verify.py db --mode items`，断言目标条目收藏状态由 `fav0` 翻转为 `1 - fav0`。
- **两段式删除（第一击不删、3 秒内第二击软删）。** 确认速贴仍处于活跃态（或先 `signal wake` 并等 400ms），运行第一次 `fp_verify.py signal picker-delete`，等待 300ms 后查 `fp_verify.py db --mode summary` 断言 `active_items == active0` 且 `deleted_items == deleted0`；紧接着在 3 秒窗口内运行第二次 `fp_verify.py signal picker-delete`，等待 400ms 后重查 `fp_verify.py db --mode summary` 断言 `active_items == active0 - 1` 且 `deleted_items == deleted0 + 1`。

## 注意点与已知边界

- 冷启动时 `ClipboardMonitor` 可能会先把当前系统剪贴板文本入库并排在欢迎条目上方，因此不应硬编码第 0 行的初始收藏值或总条目数，而应按“操作前 → 操作后”差值断言。
- 速贴停屏语义为坐标平移至 `-32000` 附近，`IsWindowVisible` 恒为真，不能靠 `IsWindowVisible` 判断显隐，必须看 `onscreen`（含坐标阈值与 DWM cloak 检查）。
- 在隔离沙盒或无光标桌面环境下，默认 `Mouse`/`Caret` 定位可能因无可用光标/UIA 超时而回落隐藏（`速贴定位失败，窗口保持停屏位`），故同 `docs/testing.md` E2E 一样应传 `--picker-pos 80,80` 预置 `LastPosition`。
- 刚唤醒后异步 `fetch_next` 尚未落完时行缓存为空，立即发送 `picker-favorite` / `picker-delete` 会被故意忽略，须等约 400–500ms 再操作。
- 已知边界：`wake --via-second-launch` 覆盖单实例互斥量、唤醒事件与面板切换全链路；`picker-favorite` / `picker-delete` 绕过 `RegisterHotKey` 按键分发；真实物理按键与实际向外部应用注入 `Ctrl+V` 上屏须另行在交互桌面人工/真实热键核对。速贴半透明材质迭代同时参考无头诊断入口 `picker-snapshot`。
