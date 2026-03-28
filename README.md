# FloatPaste / 浮贴

一款专注于「无打断」体验的 Windows 剪贴板工具。通过全局快捷键唤起速贴面板，无需离开当前输入场景即可完成历史剪贴项的选择与回贴。

## 核心特点

**无焦点速贴**：全局快捷键唤起，不抢占当前窗口焦点。数字直选 + 方向键导航，回贴后自动返回原窗口继续工作。

**高效检索**：Manager 资料库支持全文搜索（FTS5）+ 250ms 防抖 + 分页加载。哈希去重自动刷新已有条目。

**本地优先**：SQLite 本地存储，Tauri 2 + Rust 构建，启动驻留托盘。

## 快速开始

```bash
pnpm install
pnpm tauri dev
```

构建便携版：

```bash
pnpm release:portable
```

## 系统要求

- Windows 10 / Windows 11 x64
- WebView2 运行时（Windows 11 已内置）

## 下载

详见 [Releases](../../releases) 页面。当前为 Beta 测试阶段，首次运行可能出现系统安全警告。

## 文档

- [发布流程](docs/release/流程.md)
- [架构设计](docs/architecture/架构设计与MVP技术方案.md)
