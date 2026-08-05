use std::{error::Error, fmt};

/// 宿主策略扩展点返回的稳定边界错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductError {
    pub code: &'static str,
    pub message: String,
}

impl ProductError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ProductError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for ProductError {}
