use thiserror::Error;

#[derive(Debug, Error)]
pub enum StackError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("{service} command was not found; configure its executable in Settings")]
    CommandNotFound { service: String },
    #[error("{0} is already running")]
    AlreadyRunning(String),
    #[error("{0} is not running under Local Agent Stack")]
    NotManaged(String),
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("filesystem or process operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid response from local service: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StackError>;
