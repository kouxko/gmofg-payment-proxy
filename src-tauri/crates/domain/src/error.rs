//! 领域层统一错误模型。
//!
//! `ErrorCode` 是稳定机器码，中文 `message` 给人阅读，`field_errors` 精确指出错误字段。
//! 领域层不决定弹窗、颜色或按钮，这些展示行为由外层适配器处理。

use crate::{Revision, RuntimeEpoch};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ProxyAlreadyRunning,
    ProxyNotRunning,
    OperationInProgress,
    PortInUse,
    ConfigInvalid,
    RevisionConflict,
    CertificateNotReady,
    CertificateInvalid,
    Pkcs12PasswordInvalid,
    DpapiProtectFailed,
    DpapiUnprotectFailed,
    TlsHandshakeFailed,
    UpstreamConnectTimeout,
    UpstreamWriteTimeout,
    UpstreamReadTimeout,
    BodyTooLarge,
    HeaderLimitExceeded,
    BodyDecodeFailed,
    BodyEncodeFailed,
    JsonInvalid,
    RuleInvalid,
    /// Socket Document 规则执行被调用方显式取消。
    RuleExecutionCancelled,
    RuleConflictWarning,
    /// 协议包 ID 或 `SemVer` 不符合稳定身份约束。
    ProtocolPackageInvalid,
    /// Document Schema 的身份、字段声明或聚合结构无效。
    DocumentSchemaInvalid,
    /// 脚本或调用方访问了 Schema 未声明的字段。
    DocumentFieldUndeclared,
    /// 字段已声明，但当前 Frame 尚未给它赋值。
    DocumentFieldUnassigned,
    /// 写入值的类型与 Schema 声明不一致。
    DocumentFieldTypeMismatch,
    BreakpointNotFound,
    BreakpointAlreadyResolved,
    BreakpointClientDisconnected,
    BreakpointProxyStopped,
    ResourceExhausted,
    EventCursorExpired,
    ExportFailed,
    ImportFailed,
    DatabaseSchemaInvalid,
    InvalidStateTransition,
    InternalError,
}

impl ErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProxyAlreadyRunning => "PROXY_ALREADY_RUNNING",
            Self::ProxyNotRunning => "PROXY_NOT_RUNNING",
            Self::OperationInProgress => "OPERATION_IN_PROGRESS",
            Self::PortInUse => "PORT_IN_USE",
            Self::ConfigInvalid => "CONFIG_INVALID",
            Self::RevisionConflict => "REVISION_CONFLICT",
            Self::CertificateNotReady => "CERTIFICATE_NOT_READY",
            Self::CertificateInvalid => "CERTIFICATE_INVALID",
            Self::Pkcs12PasswordInvalid => "PKCS12_PASSWORD_INVALID",
            Self::DpapiProtectFailed => "DPAPI_PROTECT_FAILED",
            Self::DpapiUnprotectFailed => "DPAPI_UNPROTECT_FAILED",
            Self::TlsHandshakeFailed => "TLS_HANDSHAKE_FAILED",
            Self::UpstreamConnectTimeout => "UPSTREAM_CONNECT_TIMEOUT",
            Self::UpstreamWriteTimeout => "UPSTREAM_WRITE_TIMEOUT",
            Self::UpstreamReadTimeout => "UPSTREAM_READ_TIMEOUT",
            Self::BodyTooLarge => "BODY_TOO_LARGE",
            Self::HeaderLimitExceeded => "HEADER_LIMIT_EXCEEDED",
            Self::BodyDecodeFailed => "BODY_DECODE_FAILED",
            Self::BodyEncodeFailed => "BODY_ENCODE_FAILED",
            Self::JsonInvalid => "JSON_INVALID",
            Self::RuleInvalid => "RULE_INVALID",
            Self::RuleExecutionCancelled => "RULE_EXECUTION_CANCELLED",
            Self::RuleConflictWarning => "RULE_CONFLICT_WARNING",
            Self::ProtocolPackageInvalid => "PROTOCOL_PACKAGE_INVALID",
            Self::DocumentSchemaInvalid => "DOCUMENT_SCHEMA_INVALID",
            Self::DocumentFieldUndeclared => "DOCUMENT_FIELD_UNDECLARED",
            Self::DocumentFieldUnassigned => "DOCUMENT_FIELD_UNASSIGNED",
            Self::DocumentFieldTypeMismatch => "DOCUMENT_FIELD_TYPE_MISMATCH",
            Self::BreakpointNotFound => "BREAKPOINT_NOT_FOUND",
            Self::BreakpointAlreadyResolved => "BREAKPOINT_ALREADY_RESOLVED",
            Self::BreakpointClientDisconnected => "BREAKPOINT_CLIENT_DISCONNECTED",
            Self::BreakpointProxyStopped => "BREAKPOINT_PROXY_STOPPED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::EventCursorExpired => "EVENT_CURSOR_EXPIRED",
            Self::ExportFailed => "EXPORT_FAILED",
            Self::ImportFailed => "IMPORT_FAILED",
            Self::DatabaseSchemaInvalid => "DATABASE_SCHEMA_INVALID",
            Self::InvalidStateTransition => "INVALID_STATE_TRANSITION",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Error, PartialEq, Serialize, Type)]
#[error("{code}: {message}")]
pub struct DomainError {
    pub code: ErrorCode,
    pub message: String,
    pub field_errors: Box<BTreeMap<String, Vec<String>>>,
    pub retryable: bool,
    pub suggested_action: Option<String>,
    pub entity_id: Option<String>,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub actual_revision: Option<Revision>,
}

impl DomainError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field_errors: Box::default(),
            retryable: false,
            suggested_action: None,
            entity_id: None,
            runtime_epoch: None,
            actual_revision: None,
        }
    }

    #[must_use]
    pub fn with_field_error(
        mut self,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.field_errors
            .entry(field.into())
            .or_default()
            .push(message.into());
        self
    }

    #[must_use]
    pub const fn retryable(mut self, value: bool) -> Self {
        self.retryable = value;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // STATE-014, NFR-008
    #[test]
    fn error_codes_have_stable_wire_values() {
        assert_eq!(ErrorCode::RevisionConflict.as_str(), "REVISION_CONFLICT");
        assert_eq!(
            serde_json::to_string(&ErrorCode::BodyTooLarge).unwrap(),
            "\"BODY_TOO_LARGE\""
        );
        let cases = [
            (ErrorCode::ProxyAlreadyRunning, "PROXY_ALREADY_RUNNING"),
            (ErrorCode::ProxyNotRunning, "PROXY_NOT_RUNNING"),
            (ErrorCode::OperationInProgress, "OPERATION_IN_PROGRESS"),
            (ErrorCode::PortInUse, "PORT_IN_USE"),
            (ErrorCode::ConfigInvalid, "CONFIG_INVALID"),
            (ErrorCode::RevisionConflict, "REVISION_CONFLICT"),
            (ErrorCode::CertificateNotReady, "CERTIFICATE_NOT_READY"),
            (ErrorCode::CertificateInvalid, "CERTIFICATE_INVALID"),
            (ErrorCode::Pkcs12PasswordInvalid, "PKCS12_PASSWORD_INVALID"),
            (ErrorCode::DpapiProtectFailed, "DPAPI_PROTECT_FAILED"),
            (ErrorCode::DpapiUnprotectFailed, "DPAPI_UNPROTECT_FAILED"),
            (ErrorCode::TlsHandshakeFailed, "TLS_HANDSHAKE_FAILED"),
            (
                ErrorCode::UpstreamConnectTimeout,
                "UPSTREAM_CONNECT_TIMEOUT",
            ),
            (ErrorCode::UpstreamWriteTimeout, "UPSTREAM_WRITE_TIMEOUT"),
            (ErrorCode::UpstreamReadTimeout, "UPSTREAM_READ_TIMEOUT"),
            (ErrorCode::BodyTooLarge, "BODY_TOO_LARGE"),
            (ErrorCode::HeaderLimitExceeded, "HEADER_LIMIT_EXCEEDED"),
            (ErrorCode::BodyDecodeFailed, "BODY_DECODE_FAILED"),
            (ErrorCode::BodyEncodeFailed, "BODY_ENCODE_FAILED"),
            (ErrorCode::JsonInvalid, "JSON_INVALID"),
            (ErrorCode::RuleInvalid, "RULE_INVALID"),
            (
                ErrorCode::RuleExecutionCancelled,
                "RULE_EXECUTION_CANCELLED",
            ),
            (ErrorCode::RuleConflictWarning, "RULE_CONFLICT_WARNING"),
            (
                ErrorCode::ProtocolPackageInvalid,
                "PROTOCOL_PACKAGE_INVALID",
            ),
            (ErrorCode::DocumentSchemaInvalid, "DOCUMENT_SCHEMA_INVALID"),
            (
                ErrorCode::DocumentFieldUndeclared,
                "DOCUMENT_FIELD_UNDECLARED",
            ),
            (
                ErrorCode::DocumentFieldUnassigned,
                "DOCUMENT_FIELD_UNASSIGNED",
            ),
            (
                ErrorCode::DocumentFieldTypeMismatch,
                "DOCUMENT_FIELD_TYPE_MISMATCH",
            ),
            (ErrorCode::BreakpointNotFound, "BREAKPOINT_NOT_FOUND"),
            (
                ErrorCode::BreakpointAlreadyResolved,
                "BREAKPOINT_ALREADY_RESOLVED",
            ),
            (
                ErrorCode::BreakpointClientDisconnected,
                "BREAKPOINT_CLIENT_DISCONNECTED",
            ),
            (
                ErrorCode::BreakpointProxyStopped,
                "BREAKPOINT_PROXY_STOPPED",
            ),
            (ErrorCode::ResourceExhausted, "RESOURCE_EXHAUSTED"),
            (ErrorCode::EventCursorExpired, "EVENT_CURSOR_EXPIRED"),
            (ErrorCode::ExportFailed, "EXPORT_FAILED"),
            (ErrorCode::ImportFailed, "IMPORT_FAILED"),
            (ErrorCode::DatabaseSchemaInvalid, "DATABASE_SCHEMA_INVALID"),
            (ErrorCode::InternalError, "INTERNAL_ERROR"),
        ];
        for (code, expected) in cases {
            assert_eq!(code.as_str(), expected);
            assert_eq!(
                serde_json::to_string(&code).unwrap(),
                format!("\"{expected}\"")
            );
        }
    }
}
