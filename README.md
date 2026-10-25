<p align="center">
  <img src="crates/floatpaste-native/assets/icon-512.png" width="140" alt="FloatPaste logo" />
</p>

<h1 align="center">FloatPaste · 浮贴</h1>

<p align="center">
  无打断的 Windows 剪贴板历史工具<br/>
  快捷键唤起 · 不抢焦点 · 本地存储 · 低内存常驻
</p>

<p align="center">
  <a href="https://github.com/tranquil-w/floatpaste/releases"><img src="https://img.shields.io/github/v/release/tranquil-w/floatpaste?style=flat-square&label=%E5%8F%91%E7%89%88" alt="Release"/></a>
  <a href="https://github.com/tranquil-w/floatpaste/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/tranquil-w/floatpaste/ci.yml?branch=main&style=flat-square&label=CI" alt="CI"/></a>
  <img src="https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Windows%2010%2F11%20x64-blue?style=flat-square" alt="Platform"/>
  <img src="https://img.shields.io/badge/%E6%B8%B2%E6%9F%93-Rust%20%2B%20Slint%20%E8%BD%AF%E4%BB%B6%E6%B8%B2%E6%9F%93-orange?style=flat-square" alt="Stack"/>
</p>

复制过的内容自动入库；随时按快捷键唤出速贴面板，**不离开当前输入场景**就能选一条重新上屏。文本、图片、文件路径都支持；带 HTML 的文本可保留原格式上屏，也可仅上屏纯文本，图片、文件还提供多种上屏方式（路径、文件名、图片文件），默认次级动作可配置。

## ✨ 核心特性

- **无焦点速贴**：全局快捷键唤起面板，不抢占当前窗口焦点；数字直选 + 方向键导航，上屏后自动回到原窗口继续工作
- **更多操作**：速贴行内「⋯」集中提供上屏方式、收藏、编辑和删除；按 Delete 或选择菜单中的删除后再次确认，见 [速贴更多操作](docs/no-focus-picker.md#速贴更多操作)。
- **富文本保留**：保存同一条目的纯文本与 HTML；Enter 按原格式上屏，Ctrl+Enter 仅上屏纯文本，预览只显示文字。支持范围见 [运行行为](docs/runtime-behavior.md)。
- **图片与文件**：缩略图预览、悬停大图、哈希去重；提供多种上屏方式——图片（图片 / 图片路径 / 图片文件）、文件（文件 / 完整路径 / 文件名），临时选择不改默认，默认次级动作可在设置中按类型配置
- **高效检索**：独立搜索窗口提供全文搜索（FTS5）、类型与标签筛选、收藏、两段式删除与条目编辑
- **主题系统**：浅色 / 深色 / 跟随系统 × 多套预设 × 安全强调色，OKLCH 对比度校正保证偏色屏幕上依然可读
- **本地优先**：SQLite 本地存储，不向任何外部服务发送数据；排除应用、暂停监听、上屏后可选恢复原剪贴板
- **轻量常驻**：Rust + Slint 软件渲染单进程，无 WebView 依赖，后台常驻内存占用低

## ⌨️ 快捷键

下表为默认键位；全局快捷键与会话键可在设置中调整，数字直达键可关闭。界面提示跟随当前配置，升级规则见[快捷键设置与升级](docs/no-focus-picker.md#快捷键设置与升级)。

| 键 | 作用 |
|----|------|
| `` Alt+` `` | 唤起 / 关闭速贴面板（可自定义） |
| `Alt+Q` | 唤起搜索窗口（可自定义，可停用） |
| `↑` `↓` / `1-9` | 速贴内导航 / 数字直达并上屏 |
| `Enter` | 上屏选中条目 |
| `Ctrl+Enter` | 次级上屏：执行该类型配置的默认动作（初始为富文本纯文本、图片/文件路径文本） |
| `Ctrl+E` | 进编辑器 |
| `Ctrl+D` | 收藏 / 取消收藏 |
| `Delete` | 删除选中条目，再次触发确认（速贴与搜索窗口共用，可自定义） |
| `Esc` | 取消当前操作；无待确认或菜单打开时关闭并归还焦点 |

## 📥 安装

从 [Releases](https://github.com/tranquil-w/floatpaste/releases) 下载最新版本：

- **安装包**（Inno Setup），支持覆盖升级；卸载默认保留数据，也可选择清理历史记录与设置，具体行为见 [发布流程](docs/release/process.md#发布形态)
- **便携版** zip，解压即用

系统要求：Windows 10 / Windows 11 x64。

## 🔨 从源码构建

开发环境、运行与测试命令见 [开发与贡献](CONTRIBUTING.md)。

## 📚 文档

- [文档地图](docs/README.md) — 按问题查找架构、行为规格、测试与决策依据
- [术语表](CONTEXT.md) · [发布流程](docs/release/process.md)

## 📄 License

[MIT](LICENSE)
