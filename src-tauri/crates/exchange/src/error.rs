//! Exchange 数据面的单一错误类型。
//!
//! 错误阶段由失败发生处直接写入 structured tracing；错误值只负责沿 `Result` 传播，
//! 避免业务控制流与 UI/诊断模型耦合。

use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalPackageCallStage {
    Frame,
    Decode,
    Display,
    Encode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalPackageCallFailure {
    pub package: ProtocolPackageRef,
    pub direction: ProtocolDirection,
    pub stage: ExternalPackageCallStage,
    pub method: String,
    pub request_id: Option<String>,
    pub remote_code: Option<i64>,
    pub stable_code: Option<String>,
    pub remote_message: Option<String>,
    pub remote_data_summary: Option<String>,
}

#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
#[error("{message}")]
pub struct Error {
    pub message: String,
    pub external_package_call: Option<Box<ExternalPackageCallFailure>>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            external_package_call: None,
        }
    }

    #[must_use]
    pub fn with_external_package_call(mut self, failure: ExternalPackageCallFailure) -> Self {
        self.external_package_call = Some(Box::new(failure));
        self
    }
}
