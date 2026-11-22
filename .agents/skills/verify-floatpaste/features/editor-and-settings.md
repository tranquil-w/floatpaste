# 编辑与设置窗口

编辑窗口与设置窗口是 Slint 无边框桌面内容窗口（`no-frame: true`），承担条目全文编辑、标签管理及六大设置页配置与热键/主题/隐私即时联动。

## 子功能

- `content-windows-precreated`：冷启动时两扇内容窗口已完成创建/停屏管理且未在启动期闪现。
- `dpi-and-theme-snapshots`：通过统一检查脚本输出 100%、125%、150% 三档 DPI 及深浅主题无头截图，核对排版裁切与隐私确认框交互。

## 用户入口（用户视角）

- 编辑窗口：在速贴或搜索窗口按下编辑会话键（或通过行内「⋯」更多操作选择编辑）。
- 设置窗口：右键托盘菜单选择「设置」。

## 驱动配方

前置条件：

- 本仓库 Rust 工具链与 Node 环境可用；运行 L2 UI 截图不写系统剪贴板，但若附带 `-Scope Rust` 会同时执行完整 `e2e_smoke`（需先按 [剪贴采集与隐私](./capture-and-privacy.md) 执行 `daily pause`）。

- **查看实机装配与冷启动隔离态。** 启动隔离实例后在同桌面运行 `fp_verify.py windows`，核对返回列表中的窗口句柄与 `onscreen` 状态：实测 `EditorWindow`（初始几何 `800×600`）与 `SettingsWindow`（初始几何 `920×760`）在 Win32 侧均呈 `title="floatpaste"`、`class="Window Class"`、`visible=false` 且停靠于 `(-32000, -32000)`（由 `fp_verify.py` 按宽高区分标为 `editor` 与 `settings`），未被用户唤起前绝不出现在屏内可见区域。
- **生成三档缩放深浅主题 UI 截图（100% / 125% / 150%）。**
  - 如日常实例正在运行且只需检查 L2 无头契约与设置/编辑窗快照，可单独运行：
    - `$env:FLOATPASTE_UI_SNAPSHOTS='1'; $env:SLINT_SCALE_FACTOR='1'; cargo test --locked -p floatpaste-native --bin floatpaste l2_headless_ui_contracts -- --nocapture`
    - 分别替换 `SLINT_SCALE_FACTOR='1.25'` 与 `'1.5'`，按提示重新经 Cargo 编译运行。
  - 若需端到端跑完整交付门槛并生成三档截图，先 `fp_verify.py daily pause`，再运行 `pwsh -NoProfile -File scripts/check.ps1 -Scope Rust -UiSnapshots`，完成后 `fp_verify.py daily restore`。
- **视觉核对。** 抽查输出目录 `.artifacts/` 中生成的 `editor-note-*.png`、`editor-reveal-*.png`、`settings-page-*.png`、`shortcut-errors-*.png` 与 `privacy-data-*.png`（确认中文/emoji 编辑字数、长文本滚动与最小窗宽下无裁切）。

## 注意点与已知边界

- 当前二进制仅对速贴、搜索窗口与监听切换暴露了测试命名事件（见 `crates/floatpaste-core/src/platform/windows/single_instance.rs`），未对「打开设置」「打开编辑」「文本粘贴为文件…」单设命名事件，因此编辑/设置窗口的自动化UI行为主要通过 `l2_headless_ui_contracts` 驱动；真机 Mica 材质、最小化恢复重挂材质与拖拽标题栏手势目前仍需真机交互核对，不可虚报为自动化覆盖。
- 切换 `SLINT_SCALE_FACTOR` 时 Slint 编译器会消费该环境变量，必须经 Cargo 重新构建对应测试档，不能复用上一档测试二进制。
