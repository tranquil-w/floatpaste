# 剪贴采集与隐私

剪贴采集与隐私覆盖日常监听开关持久化、排除应用、系统剪贴板写入触发录入与去重，以及锁屏停止采集等隐私边界，验证时必须严格保护用户个人数据库与系统剪贴板。

## 子功能

- `monitoring-toggle-persist`：在不写系统剪贴板的前提下切换监听状态，验证 `user_settings.pauseMonitoring` 在只读 SQLite 中即时落盘。
- `clipboard-ingest-and-retention`：写系统剪贴板触发真实录入、去重、溢出清理与快照恢复（必须先暂停日常实例）。

## 用户入口（用户视角）

- 右键系统托盘图标 → 点击「暂停监听」/「恢复监听」，或设置窗口「隐私与数据」页切换「暂停监听」。
- 在任意应用中执行复制（`Ctrl+C`），触发 FloatPaste 监听入库。

## 驱动配方

前置条件（`monitoring-toggle-persist`，不碰剪贴板，可与日常实例并行）：

- 隔离实例已启动且通过 `doctor`。

- **暂停监听并核对落盘。** 先运行 `fp_verify.py db --mode summary` 确认初始 `pause_monitoring == false`；运行 `fp_verify.py signal toggle-monitoring`，等待 300ms 后重查 `fp_verify.py db --mode summary`，断言 `pause_monitoring == true`（也可用 `fp_verify.py db --mode settings` 查看完整 `user_settings`）。
- **恢复监听并核对落盘。** 再次运行 `fp_verify.py signal toggle-monitoring`，等待 300ms 后重查 `fp_verify.py db --mode summary`，断言 `pause_monitoring == false`。

前置条件（`clipboard-ingest-and-retention`，**强依赖独占系统剪贴板**）：

- 如 `fp_verify.py daily status` 显示 `mutex_active == true`，**严禁**直接运行写剪贴板测试；必须先暂停日常实例。

- **安全暂停日常实例并执行完整系统剪贴板 E2E。**
  1. 运行 `fp_verify.py daily pause`，确认退出码 `0` 且 `daily-state.json` 已记录原实例路径与参数。
  2. 运行仓库严格 E2E：`pwsh -NoProfile -File scripts/check.ps1 -Scope Rust`（或单跑 `$env:FLOATPASTE_E2E_STRICT='1'; cargo test -p floatpaste-native --test e2e_smoke -- --nocapture`）。
  3. 验证完成后运行 `fp_verify.py doctor` 确认剪贴板 `openable == true`（且确认无测试内容残留）。
  4. 运行 `fp_verify.py daily restore` 按原路径与启动参数恢复日常实例，再跑 `fp_verify.py daily status` 确认原日常实例形态恢复正常运行。

## 注意点与已知边界

- `APPDATA` 重定向无法阻止用户日常实例监听同一系统剪贴板，因此任何写入系统剪贴板的测试绝不能在日常实例运行时偷跑。
- 若日常实例正开启「始终以管理员身份启动」（UAC）或常规退出超时，`daily pause` 会返回退出码 `2` 并拒绝强杀，须由用户在托盘手动退出后继续。
- 真实 Windows `Win+L` 锁屏停采边界与第三方富文本目标程序（Word/Excel/浏览器）粘贴格式兼容性仍依赖真机桌面样本验收，不能仅靠 L3 合成 `WM_WTSSESSION_CHANGE` 推断。
