use thiserror::Error;

#[derive(Error, Debug)]
pub enum TlqError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Timeout error after {0}ms")]
    Timeout(u64),

    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Max retries exceeded ({max_retries}) for operation")]
    MaxRetriesExceeded { max_retries: u32 },

    #[error("Message too large: {size} bytes (max: 65536)")]
    MessageTooLarge { size: usize },
}

impl TlqError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            TlqError::Connection(_) | TlqError::Timeout(_) | TlqError::Io(_)
        )
    }
}

pub type Result<T> = std::result::Result<T, TlqError>;