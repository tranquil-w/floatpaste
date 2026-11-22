# FloatPaste 验证地图

本目录是验证 FloatPaste 用户可见行为与可观察副作用的持续维护真相来源。在驱动应用前先核对基线前置条件，再按对应功能文件执行。

## 基线前置条件

- 二进制已构建（默认 `target/debug/floatpaste.exe`，由 `doctor` 核对是否过期）。
- 通过 `fp_verify.py launch` 启动隔离实例，独占使用 `.artifacts/verify-floatpaste/<run-id>/runtime/appdata` 与 `FLOATPASTE_INSTANCE_ID=vf-<run-id>`，`FLOATPASTE_SKIP_AUTOSTART_SYNC=1`。
- `launch` 后立即执行 `doctor`，确认零冷启动闪现、日志含 `应用启动诊断` 且无 `ERROR`/panic。
- **日常实例与剪贴板铁律**：只要测试会写入系统剪贴板或抢占默认热键，**必须先**执行 `fp_verify.py daily pause` 优雅退出日常实例，并在确认剪贴板恢复后再 `daily restore`；不接触系统剪贴板的隔离 UI/数据库验证可直接与日常实例并行。
- 交互桌面未锁屏（`doctor` 返回 `desktop.is_lock_screen == false` 且 `hwnd != 0`）。

## 驱动约定

- 严格按配方使用 `fp_verify.py` 字面命令，不猜测坐标，不复用历史 PID 或残留数据目录。
- `picker-favorite`、`picker-delete` 等会话操作触发前，等待窗口 `onscreen` 并预留约 400ms 让异步列表加载完成（高度收缩且行缓存就绪）。
- 每次数据库或设置变更后，必须用只读 `fp_verify.py db` 查询二次核对持久化结果。
- 清理时只使用 `fp_verify.py cleanup`，保留 `.artifacts/verify-floatpaste/<run-id>/evidence/`。

## 证据与跳过报告

- 每张截图经 `fp_verify.py capture` 输出到 `evidence/` 并记录 `feature`、`entry_point`、`action`、`dpi` 与前景窗口信息到 `manifest.jsonl`。
- 必须明确区分三类覆盖：**L2 无头渲染/事件契约**、**L3 隔离真进程（命名事件/二次启动驱动）**、**交互桌面物理输入/跨进程第三方接收**。
- 当锁屏或日常实例未退出导致路径不可触达时，报告「未触达」、未满足前提与已尝试命令，不以无头检查冒充真机覆盖。

## 功能索引

- [速贴面板](./quick-paste.md) — 冷启动零闪现、二次启动唤醒、无焦点与置顶、收藏切换、两段式删除与实机截图。
- [搜索窗口](./search-window.md) — 打开搜索窗口、屏内显隐与 `Esc` 归还前台、两段式删除及无头过滤分工。
- [剪贴采集与隐私](./capture-and-privacy.md) — 暂停/恢复监听持久化、`daily pause/restore` 保护下的系统剪贴板录入与锁屏隔离分工。
- [编辑与设置窗口](./editor-and-settings.md) — 窗口装配核对、100%/125%/150% 深浅主题无头截图检查与当前实机交互覆盖边界。
