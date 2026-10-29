use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("未找到对应条目")]
    ItemNotFound,
    #[error("数据库错误: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Windows 系统调用错误: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("系统剪贴板不可用: {0}")]
    Clipboard(String),
    #[error("已跳过超限内容：{0}")]
    InputLimit(&'static str),
    #[error("操作已取消")]
    OperationCancelled,
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
