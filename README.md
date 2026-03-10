# FloatPaste / 浮贴

当前仓库已从架构文档进入首轮实现，现阶段落地内容如下：

- React + Vite + Tailwind 前端基础骨架
- Manager 资料库窗口基础界面
- Picker 速贴面板基础界面
- Tauri 2 + Rust 后端工程骨架
- SQLite 数据表、FTS5 搜索与设置持久化
- 文本剪贴监听轮询版原型
- 收藏、删除、编辑、搜索、写入剪贴板命令
- 全局快捷键、Picker 窗口协调与 Windows `Ctrl+V` 注入原型
- 托盘菜单与监听状态切换
- 基于真实前台进程名的排除应用过滤
- 可聚焦 Picker 窗口与单窗口切换策略
- 更明确的回贴结果状态码

## 当前状态

这一轮实现已经覆盖文档中的 Phase 1、Phase 2、Phase 3，并推进到 Phase 4 的稳定性收口。

已经具备：

- 文本剪贴入库
- 历史与收藏列表
- Manager 全文搜索
- 文本编辑与删除
- 设置项保存
- 写入系统剪贴板
- 全局快捷键唤起 Picker
- Picker 中方向键、数字键、Enter、Esc 操作
- 回贴前恢复目标窗口并尝试注入 `Ctrl+V`
- Picker 当前为可聚焦窗口，键盘操作由前端本地按键事件处理
- 当前运行时只显示一个主窗口：打开 Picker 时隐藏 Manager，关闭 Picker 后按来源返回目标窗口或 Manager
- 托盘打开资料库、打开设置、切换监听与退出
- 排除应用列表可按前台进程名生效

尚未完成：

- 更细的前台应用识别与敏感场景预设
- 更接近“无打断”目标的窗口体验与应用兼容性验证
- 回贴失败场景的更细颗粒诊断与灰度策略

## 本地启动

前端浏览器预览：

```bash
pnpm install
pnpm dev
```

Tauri 桌面运行：

```bash
pnpm install
pnpm tauri dev
```

仓库当前使用项目内的 `@tauri-apps/cli`，默认不要求全局安装 Cargo 版 CLI。

如果你确实想单独安装 Rust 版全局 CLI，正确命令是：

```bash
cargo install tauri-cli
```

浏览器预览模式下会自动切换到本地模拟数据，便于先开发界面；Tauri 环境下则走真实 Rust 命令。
