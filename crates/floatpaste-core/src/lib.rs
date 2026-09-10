//! FloatPaste 核心域：与 GUI 运行时（Tauri / Slint）无关的领域模型、
//! SQLite 仓储、业务服务与 Windows 平台集成。
//!
//! 分层约束：
//! - `domain`：纯数据与业务概念，不依赖任何基础设施；
//! - `repository`：rusqlite 数据访问；
//! - `services`：业务规则编排，仅依赖 domain/repository；
//! - `platform`：Win32 原生集成（剪贴板、监听、快捷键、单实例等），
//!   通过回调向 UI 层暴露事件，不反向依赖任何窗口框架。

pub mod domain;
pub mod launch_mode;
pub mod platform;
pub mod repository;
pub mod services;
pub mod state;
pub mod theme;
