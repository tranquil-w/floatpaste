<p align="center">
  <img src="crates/floatpaste-native/assets/icon-512.png" width="140" alt="FloatPaste logo" />
</p>

<h1 align="center">FloatPaste · 浮贴</h1>

<p align="center">
  无打断的 Windows 剪贴板历史工具<br/>
  快捷键唤起 · 不抢焦点 · 本地存储 · 低内存常驻
</p>

<p align="center">
  <a href="../../releases"><img src="https://img.shields.io/github/v/release/tranquil-w/floatpaste?style=flat-square&label=%E5%8F%91%E7%89%88" alt="Release"/></a>
  <a href="../../actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/tranquil-w/floatpaste/ci.yml?branch=main&style=flat-square&label=CI" alt="CI"/></a>
  <img src="https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Windows%2010%2F11%20x64-blue?style=flat-square" alt="Platform"/>
  <img src="https://img.shields.io/badge/%E6%B8%B2%E6%9F%93-Rust%20%2B%20Slint%20%E8%BD%AF%E4%BB%B6%E6%B8%B2%E6%9F%93-orange?style=flat-square" alt="Stack"/>
</p>

复制过的内容自动入库；随时按快捷键唤出速贴面板，**不离开当前输入场景**就能选一条重新上屏。文本、图片、文件路径都支持，图片、文件还可粘贴为路径文本。

## ✨ 核心特性

- **无焦点速贴**：全局快捷键唤起面板，不抢占当前窗口焦点；数字直选 + 方向键导航，上屏后自动回到原窗口继续工作
- **图片与文件**：缩略图预览、悬停大图、哈希去重；图片/文件条目支持次级上屏——粘贴为图片路径 / 文件路径（路径作为文本上屏）
- **高效检索**：独立搜索窗口提供全文搜索（FTS5）、类型与标签筛选、收藏、两段式删除与条目编辑
- **主题系统**：浅色 / 深色 / 跟随系统 × 多套预设 × 安全强调色，OKLCH 对比度校正保证偏色屏幕上依然可读
- **本地优先**：SQLite 本地存储，不向任何外部服务发送数据；排除应用、暂停监听、上屏后恢复原剪贴板
- **轻量常驻**：Rust + Slint 软件渲染单进程，无 WebView 依赖，后台常驻内存占用低

## ⌨️ 快捷键

| 键 | 作用 |
|----|------|
| `Ctrl+Q` | 唤起 / 关闭速贴面板（可自定义） |
| `Alt+S` | 唤起搜索窗口（可自定义，可停用） |
| `↑` `↓` / `1-9` | 速贴内导航 / 数字直达并上屏 |
| `Enter` | 上屏选中条目 |
| `Shift+Enter` | 次级上屏：图片粘贴为图片路径，文件粘贴为文件路径 |
| `Ctrl+Enter` | 进编辑器 |
| `Ctrl+Space` | 收藏 / 取消收藏 |
| `Esc` | 关闭并归还焦点 |

## 📥 安装

从 [Releases](../../releases) 下载最新版本：

- **安装包**（Inno Setup，中文向导，默认安装至 Program Files，自动迁移旧版与自启动设置）
- **便携版** zip，解压即用

系统要求：Windows 10 / Windows 11 x64。

## 🔨 从源码构建

```bash
cargo run -p floatpaste-native     # 运行（debug）
cargo test                         # 全部测试
cargo build --release -p floatpaste-native
```

WSL 环境下用 `./scripts/win-cargo test` 转发到 Windows 工具链。发版由 tag 驱动 GitHub Actions 产出安装包与便携包。

## 📚 文档

- [架构概览](docs/architecture.md) — 结构、行为规格与关键文件索引
- [无焦点速贴窗口架构与防坑指南](docs/no-focus-picker.md) — 窗口时序与 Win32 坑位
- [主题系统设计](docs/theme-system.md) — 三层 token 与对比度规则
- [文档地图](docs/README.md) · [发布流程](docs/release/process.md)
