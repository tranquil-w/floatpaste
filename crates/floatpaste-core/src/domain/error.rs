use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    SerdeJson(#[from] serde_json::Error),
    /// 仅供老 Tauri 壳启用 `tauri` 特性后使用，使壳层代码可对
    /// `tauri::Error` 直接 `?`；原生壳不启用该特性。
    #[cfg(feature = "tauri")]
    #[error("Tauri 运行时错误: {0}")]
    Tauri(#[from] tauri::Error),
    #[error("Windows 系统调用错误: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("系统剪贴板不可用: {0}")]
    Clipboard(String),
    #[error("状态锁已损坏")]
    Poisoned,
    #[error("{0}")]
    Message(String),
}

impl<T> From<std::sync::PoisonError<T>> for AppError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        Self::Poisoned
    }
}
