//! Tokio/Hyper/rustls runtime for the GMO-FG interception proxy.
//!
//! The traits exported by this crate are deliberately application-neutral:
//! application use-cases implement [`PipelinePorts`], while transport and time
//! are injectable for deterministic tests.

#![allow(clippy::missing_errors_doc, clippy::too_many_lines)]

pub mod application_adapter;
pub mod codec;
pub mod fault;
pub mod message;
pub mod production_factory;
pub mod supervisor;
pub mod tls;
pub mod transport;

pub use application_adapter::{
    ApplicationProxyAdapter, ChannelRuntimeMetrics, RuntimeMetricsProvider, RuntimeMetricsSnapshot,
};
pub use fault::{FaultAction, ResponseDisposition};
pub use message::{Message, MessageLimits, RawHeader};
pub use production_factory::{
    RustlsRuntimeServiceFactory, TlsMaterialProvider, TlsMaterialSnapshot,
};
pub use supervisor::{
    Channel, ChannelConfig, ProxyConfig, ProxyState, ProxySupervisor, RuntimeServiceFactory,
    RuntimeSnapshot,
};
pub use transport::{
    AcceptedConnection, Clock, ConnectionContext, HandshakePolicy, PipelinePorts, SystemClock,
    TlsPeerIdentity, TokioListenerBinder, UpstreamConnector,
};

use std::fmt::Debug;
use std::io;

use thiserror::Error;

/// Stable proxy error classification (requirements §15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ProxyAlreadyRunning,
    OperationInProgress,
    PortInUse,
    ConfigInvalid,
    CertificateNotReady,
    CertificateInvalid,
    Pkcs12PasswordInvalid,
    DpapiUnprotectFailed,
    TlsHandshakeFailed,
    UpstreamConnectTimeout,
    UpstreamWriteTimeout,
    UpstreamReadTimeout,
    BodyTooLarge,
    HeaderLimitExceeded,
    ShiftJisDecodeFailed,
    ShiftJisEncodeFailed,
    JsonInvalid,
    IncorrectContentLength,
    TruncatedResponse,
    ClientDisconnected,
    ProxyStopped,
    Io,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProxyAlreadyRunning => "PROXY_ALREADY_RUNNING",
            Self::OperationInProgress => "OPERATION_IN_PROGRESS",
            Self::PortInUse => "PORT_IN_USE",
            Self::ConfigInvalid => "CONFIG_INVALID",
            Self::CertificateNotReady => "CERTIFICATE_NOT_READY",
            Self::CertificateInvalid => "CERTIFICATE_INVALID",
            Self::Pkcs12PasswordInvalid => "PKCS12_PASSWORD_INVALID",
            Self::DpapiUnprotectFailed => "DPAPI_UNPROTECT_FAILED",
            Self::TlsHandshakeFailed => "TLS_HANDSHAKE_FAILED",
            Self::UpstreamConnectTimeout => "UPSTREAM_CONNECT_TIMEOUT",
            Self::UpstreamWriteTimeout => "UPSTREAM_WRITE_TIMEOUT",
            Self::UpstreamReadTimeout => "UPSTREAM_READ_TIMEOUT",
            Self::BodyTooLarge => "BODY_TOO_LARGE",
            Self::HeaderLimitExceeded => "HEADER_LIMIT_EXCEEDED",
            Self::ShiftJisDecodeFailed => "SHIFT_JIS_DECODE_FAILED",
            Self::ShiftJisEncodeFailed => "SHIFT_JIS_ENCODE_FAILED",
            Self::JsonInvalid => "JSON_INVALID",
            Self::IncorrectContentLength => "INCORRECT_CONTENT_LENGTH",
            Self::TruncatedResponse => "TRUNCATED_RESPONSE",
            Self::ClientDisconnected => "BREAKPOINT_CLIENT_DISCONNECTED",
            Self::ProxyStopped => "BREAKPOINT_PROXY_STOPPED",
            Self::Io => "IO_ERROR",
            Self::Internal => "INTERNAL_ERROR",
        }
    }
}

#[derive(Debug, Error)]
#[error("{code}: {message}")]
pub struct ProxyError {
    pub code: &'static str,
    pub message: String,
}

impl ProxyError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str(),
            message: message.into(),
        }
    }

    pub fn io(context: &str, error: &io::Error) -> Self {
        Self::new(ErrorCode::Io, format!("{context}: {error}"))
    }
}

pub type Result<T> = std::result::Result<T, ProxyError>;
