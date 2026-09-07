//! Exchange 数据面的单一错误类型。
//!
//! 稳定错误码、阶段与方向作为独立字段沿 `Result` 传播；message 只保留可读详情，
//! adapter 不需要再把控制信息编码进字符串后由调用方解析。

use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};

use crate::DirectionKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorStage {
    CapabilityFactory,
    Connect,
    Read,
    Frame,
    Decode,
    Display,
    Rules,
    Encode,
    Finalize,
    Write,
    Flush,
    Shutdown,
    Protocol,
}

impl ErrorStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CapabilityFactory => "capability_factory",
            Self::Connect => "connect",
            Self::Read => "read",
            Self::Frame => "frame",
            Self::Decode => "decode",
            Self::Display => "display",
            Self::Rules => "rules",
            Self::Encode => "encode",
            Self::Finalize => "finalize",
            Self::Write => "write",
            Self::Flush => "flush",
            Self::Shutdown => "shutdown",
            Self::Protocol => "protocol",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalPackageCallStage {
    Frame,
    Decode,
    Display,
    Encode,
    Finalize,
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
    pub code: Option<String>,
    pub stage: Option<ErrorStage>,
    pub direction: Option<DirectionKind>,
    pub message: String,
    pub external_package_call: Option<Box<ExternalPackageCallFailure>>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: None,
            stage: None,
            direction: None,
            message: message.into(),
            external_package_call: None,
        }
    }

    pub fn coded(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(message).with_code(code)
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    #[must_use]
    pub fn with_stage(mut self, stage: ErrorStage) -> Self {
        self.stage = Some(stage);
        self
    }

    #[must_use]
    pub fn with_direction(mut self, direction: DirectionKind) -> Self {
        self.direction = Some(direction);
        self
    }

    /// Add pipeline context without replacing a more precise adapter annotation.
    #[must_use]
    pub fn at(mut self, stage: ErrorStage, direction: DirectionKind) -> Self {
        self.stage.get_or_insert(stage);
        self.direction.get_or_insert(direction);
        self
    }

    #[must_use]
    pub fn with_external_package_call(mut self, failure: ExternalPackageCallFailure) -> Self {
        self.external_package_call = Some(Box::new(failure));
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::{DirectionKind, ErrorStage};

    use super::Error;

    #[test]
    fn structured_annotations_do_not_modify_the_human_message() {
        let error = Error::coded("ENCODE_FAILED", "message with Upstream|READ_TIMEOUT: text")
            .with_stage(ErrorStage::Encode)
            .with_direction(DirectionKind::Downstream)
            .at(ErrorStage::Read, DirectionKind::Upstream);

        assert_eq!(error.code.as_deref(), Some("ENCODE_FAILED"));
        assert_eq!(error.stage, Some(ErrorStage::Encode));
        assert_eq!(error.direction, Some(DirectionKind::Downstream));
        assert_eq!(error.message, "message with Upstream|READ_TIMEOUT: text");
    }
}
