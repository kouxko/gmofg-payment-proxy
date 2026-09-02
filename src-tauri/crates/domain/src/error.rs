//! 领域层统一错误模型。
//!
//! `ErrorCode` 是稳定机器码，中文 `message` 给人阅读，`field_errors` 精确指出错误字段。
//! 领域层不决定弹窗、颜色或按钮，这些展示行为由外层适配器处理。

use crate::{Revision, RuntimeEpoch};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use thiserror::Error;

macro_rules! define_error_codes {
    ($( $(#[$metadata:meta])* $variant:ident => $wire:literal, )+) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
        pub enum ErrorCode {
            $(
                $(#[$metadata])*
                #[serde(rename = $wire)]
                $variant,
            )+
        }

        impl ErrorCode {
            /// Every stable wire code, for generated unknown-boundary validators.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                }
            }
        }
    };
}

define_error_codes! {
    ProxyAlreadyRunning => "PROXY_ALREADY_RUNNING",
    ProxyNotRunning => "PROXY_NOT_RUNNING",
    OperationInProgress => "OPERATION_IN_PROGRESS",
    PortInUse => "PORT_IN_USE",
    ConfigInvalid => "CONFIG_INVALID",
    RevisionConflict => "REVISION_CONFLICT",
    CertificateNotReady => "CERTIFICATE_NOT_READY",
    CertificateInvalid => "CERTIFICATE_INVALID",
    Pkcs12PasswordInvalid => "PKCS12_PASSWORD_INVALID",
    DpapiProtectFailed => "DPAPI_PROTECT_FAILED",
    DpapiUnprotectFailed => "DPAPI_UNPROTECT_FAILED",
    TlsHandshakeFailed => "TLS_HANDSHAKE_FAILED",
    UpstreamConnectTimeout => "UPSTREAM_CONNECT_TIMEOUT",
    UpstreamWriteTimeout => "UPSTREAM_WRITE_TIMEOUT",
    UpstreamReadTimeout => "UPSTREAM_READ_TIMEOUT",
    BodyTooLarge => "BODY_TOO_LARGE",
    HeaderLimitExceeded => "HEADER_LIMIT_EXCEEDED",
    BodyDecodeFailed => "BODY_DECODE_FAILED",
    BodyEncodeFailed => "BODY_ENCODE_FAILED",
    JsonInvalid => "JSON_INVALID",
    RuleInvalid => "RULE_INVALID",
    /// 协议 Document 规则执行被调用方显式取消。
    RuleExecutionCancelled => "RULE_EXECUTION_CANCELLED",
    RuleConflictWarning => "RULE_CONFLICT_WARNING",
    /// 协议包 ID 或 `SemVer` 不符合稳定身份约束。
    ProtocolPackageInvalid => "PROTOCOL_PACKAGE_INVALID",
    /// Document Schema 的身份、字段声明或聚合结构无效。
    DocumentSchemaInvalid => "DOCUMENT_SCHEMA_INVALID",
    /// A number is not finite.
    DocumentNumberInvalid => "DOCUMENT_NUMBER_INVALID",
    /// An integer JSON literal exceeds JavaScript's exact integer range.
    DocumentUnsafeInteger => "DOCUMENT_UNSAFE_INTEGER",
    /// JSON Pointer syntax is invalid.
    DocumentPointerInvalid => "DOCUMENT_POINTER_INVALID",
    /// A requested Document path or array index does not exist.
    DocumentPathMissing => "DOCUMENT_PATH_MISSING",
    /// A Document path traverses or targets the wrong JSON type.
    DocumentPathTypeMismatch => "DOCUMENT_PATH_TYPE_MISMATCH",
    /// 脚本或调用方访问了 Schema 未声明的字段。
    DocumentFieldUndeclared => "DOCUMENT_FIELD_UNDECLARED",
    /// 字段已声明，但当前 Frame 尚未给它赋值。
    DocumentFieldUnassigned => "DOCUMENT_FIELD_UNASSIGNED",
    /// 写入值的类型与 Schema 声明不一致。
    DocumentFieldTypeMismatch => "DOCUMENT_FIELD_TYPE_MISMATCH",
    ResourceExhausted => "RESOURCE_EXHAUSTED",
    EventCursorExpired => "EVENT_CURSOR_EXPIRED",
    ExportFailed => "EXPORT_FAILED",
    ImportFailed => "IMPORT_FAILED",
    DatabaseSchemaInvalid => "DATABASE_SCHEMA_INVALID",
    InvalidStateTransition => "INVALID_STATE_TRANSITION",
    InternalError => "INTERNAL_ERROR",
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
        let mut wire_values = std::collections::BTreeSet::new();
        for code in ErrorCode::ALL {
            let expected = code.as_str();
            assert!(
                wire_values.insert(expected),
                "duplicate stable code {expected}"
            );
            assert_eq!(
                serde_json::to_string(code).unwrap(),
                format!("\"{expected}\"")
            );
        }
        assert_eq!(wire_values.len(), ErrorCode::ALL.len());
    }
}
