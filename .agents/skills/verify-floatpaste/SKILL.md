---
name: verify-floatpaste
description: 用于对 FloatPaste 执行冷启动桌面实机验证：隔离 APPDATA 与单实例命名空间启动真实二进制、只读健康诊断、命名事件与二次启动驱动、DPI 感知实机截图、只读 SQLite 副作用核验、日常实例安全暂停/恢复与保留证据的安全清理。适用于需要真实进程桌面取证与功能复核时，代替临时探针脚本。
---

# verify-floatpaste — FloatPaste 冷启动桌面实机验证

本技能面向首次接手 FloatPaste 的代理，提供可复用的隔离启动、桌面观测与真实验证闭环。先读 [features/README.md](features/README.md) 选定功能配方，所有运行通过 [fp_verify.py](scripts/fp_verify.py) 驱动。

## 1. 工具分工（不重复造轮子）

| 工具 | 适用场景 | 覆盖与边界 |
| --- | --- | --- |
| `pwsh -NoProfile -File scripts/check.ps1` | 交付门槛、L1–L3 自动化与三档 L2 截图（`-UiSnapshots`） | 覆盖格式/单测/构建/`e2e_smoke`/无头快照；运行完 Rust 测试需暂停日常实例 |
| `cargo run -p floatpaste-native --bin picker-snapshot -- <light\|dark> <tag> [blur]` | 速贴外观与模糊材质快速对照 | 无头软件渲染合成背景到 `.artifacts/snapshot/`；不检查窗口装配与真桌面 |
| `python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py` | 冷启动真进程桌面隔离验证、UI 取证与状态核验 | 控制隔离进程、窗口几何、DPI 截图、只读 SQLite 与日志；不注入物理按键（`SendInput`） |

## 2. 启动、构建核对与隔离

启动前先确认二进制是否存在且未过期（默认使用 `target/debug/floatpaste.exe`）；若源码更新过，先执行 `cargo build -p floatpaste-native`。

```powershell
# 1. 启动前只读诊断（确认日常实例状态、桌面是否锁屏、二进制是否过期）
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py doctor

# 2. 隔离启动（自动创建独占 .artifacts/verify-floatpaste/<run-id>/runtime/appdata）
#    在无法依赖光标/前台的隔离桌面环境，可附加 --picker-pos 80,80 预置 LastPosition
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py launch --diagnostic-log --picker-pos 80,80
```

`launch` 自动设置并清理干扰环境变量：
- 隔离写入：`APPDATA=<runtime/appdata>`、`FLOATPASTE_INSTANCE_ID=vf-<run-id>`、`FLOATPASTE_SKIP_AUTOSTART_SYNC=1`。
- **同桌面原则**：在按命令分配临时桌面的沙盒环境（如 `exebox-*`）中，`EnumWindows`/`PrintWindow` 仅能看见同一命令桌面的窗口；务必把 `launch -> signal -> windows -> capture -> cleanup` 串联在**同一次 Shell 调用**内完成。
- 四项真实就绪判定：**进程存活** + **`Wake` / `TestExit` 命名事件就绪** + **SQLite 可读** + **`picker` / `search` 窗口已装配**；随后复核**冷启动零闪现**（所有业务窗口均处于屏外停屏，不屏内可见）。

## 3. 只读诊断（`doctor`）

每次驱动前、出现任何超时或意外行为后，先跑只读诊断：

```powershell
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py doctor
```

输出含：日常实例 `Local\FloatPaste.SingleInstance` 与进程清单、系统剪贴板可打开性、前台窗口与锁屏检测、二进制时间戳比对，以及当前会话窗口清单、只读数据库摘要与日志错误/panic 检查。

## 4. 常用驱动与特性地图

按 [features/README.md](features/README.md) 挑选配方。常用字面命令：

```powershell
# 唤起/关闭速贴（推荐用 --via-second-launch 走真实单实例唤醒）
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py signal wake --via-second-launch
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py windows --wait-role picker --expect-state onscreen --timeout 10

# 命名事件驱动与前台/只读库观察
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py signal picker-favorite
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py db --mode items
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py foreground
```

可用 `signal` 动作：`wake`、`open-search`、`search-escape`、`picker-favorite`、`picker-delete`、`search-delete`、`toggle-monitoring`、`exit`。

## 5. 证据标准与诚实报告

所有证明产物统一保存至 `.artifacts/verify-floatpaste/<run-id>/evidence/`（索引追加于 `manifest.jsonl`）：

```powershell
python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py capture `
  --role picker --out quick-paste/picker-open.png `
  --feature quick-paste --entry-point "second-launch wake" --action "open quick-paste"
```

- **动作 + 画面 + 副作用**：不仅截最终画面，还需保留操作前后 `windows` / `foreground` / `db`（只读）副作用比对。
- **截图通道（`--method auto|screen|window`）**：`Default` 桌面默认走 `screen_bitblt` 抓取 DWM 合成画面；处于隔离虚拟桌面时 `auto` 自动切为 `print_window` 直接抓取目标窗口渲染并在 `manifest.jsonl` 诚实标注，不可把 `print_window` 截图冒充真桌面 DWM 材质合成。
- **严禁夸大覆盖范围**：命名事件绕过 `RegisterHotKey` 与 `FocusScope` 按键分发；无头快照不能替代真机 DWM 合成与真实第三方上屏兼容；缺前置条件（如日常实例占用剪贴板或桌面锁屏）时必须报告「未触达」及原因。

## 6. 安全清理与日常实例保护

- **不写剪贴板时不打扰日常实例**：隔离实例的数据库、配置与命名事件与日常实例完全隔离。
- **读写系统剪贴板前必须先暂停日常实例**：禁止强杀或修改日常配置，必须走优雅停机并确认剪贴板干净后再恢复：
  ```powershell
  python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py daily pause
  # ... 执行需写剪贴板的 e2e_smoke 或实机录入验证 ...
  python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py daily restore
  ```
- **结束清理（严禁按进程名强杀，严禁删 `evidence/`）**：
  ```powershell
  python -B .agents/skills/verify-floatpaste/scripts/fp_verify.py cleanup
  ```
  `cleanup` 仅按 `session.json` 记录的 PID 发送 `TestExit`，等待优雅退出（超时才对该 PID 兜底并显式报告 `forced_kill`），随后仅移除 `runtime/` 并输出保留在 `evidence/` 中的全部文件清单。自检单测命令：`python -B -m unittest discover -s .agents/skills/verify-floatpaste/scripts -p "test_*.py" -v`。
