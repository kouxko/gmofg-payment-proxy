use thiserror::Error;

pub type AndroidControlResult<T> = Result<T, AndroidControlError>;

/// Wire failures; application adapters own public error codes and localized messages.
#[derive(Debug, Error)]
pub enum AndroidControlError {
    #[error("operation is outside the protocol whitelist")]
    InvalidOperation,
    #[error("request encoding failed: {0}")]
    Encode(serde_json::Error),
    #[error("request exceeds the frame limit")]
    RequestTooLarge,
    #[error("frame length overflows u32")]
    LengthOverflow,
    #[error("frame exceeds the frame limit")]
    FrameTooLarge,
    #[error("frame has no length prefix")]
    MissingLengthPrefix,
    #[error("frame length is invalid")]
    InvalidLength,
    #[error("invalid JSON: {0}")]
    InvalidJson(serde_json::Error),
}
