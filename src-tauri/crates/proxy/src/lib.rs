//! 基于 Tokio、Hyper 与 rustls 的可配置拦截代理运行时。
//!
//! 对外 trait 刻意保持应用无关：应用用例实现 [`PipelinePorts`]，运行时只负责监听、TLS、
//! HTTP/1 字节传输、取消与故障动作。绑定器、连接器和时间相关行为可替换，以便测试能
//! 确定性验证生命周期，而无需真实支付上游。

pub mod fault;
pub mod forward;
pub mod http;
pub(crate) mod listener;
pub mod message;
pub mod metrics;
pub mod reverse;
pub mod socket_relay;
pub mod supervisor;
pub mod tls;
pub mod traffic;
pub mod transport;

pub use fault::{FaultAction, ResponseDisposition};
pub use forward::{
    ForwardAuthenticationMode, ForwardProxyAuthenticator, ForwardProxyConfig, ForwardProxyService,
    MitmCertificateAuthority, MitmServerIdentity, NoAuthentication, absolute_uri_to_origin_form,
    strip_hop_by_hop_headers,
};
pub use http::{
    ConnectionAdmission, ConnectionService, HttpConnectionIdentity, HttpDirectionCapabilities,
    HttpObservationMetadata, HttpProtocolCapabilityFactory, NoopPipelinePorts, PipelinePorts,
    PlainHttpCapabilityFactory, RulesChain, UpstreamConnector,
};
pub use message::{Message, MessageLimits, RawHeader};
pub use metrics::{ChannelRuntimeMetrics, RuntimeMetricsProvider, RuntimeMetricsSnapshot};
pub use reverse::{
    ReverseClientIdentity, ReverseDownstreamTls, ReverseProxyConfig, ReverseProxyService,
    ReverseUpstreamTls, UpstreamConnectionTestResult, UpstreamScheme, UpstreamTlsHandshakeResult,
    UpstreamTransport,
};
pub use socket_relay::{
    BoundedSocketConnectionObserver, FrameBoundary, NoopSocketConnectionObserver,
    SocketConnectionEvent, SocketConnectionIdentity, SocketConnectionObserver,
    SocketConnectionTarget, SocketDirectionCapabilities, SocketDocumentFieldPreview,
    SocketDocumentPreview, SocketDownstreamSecurity, SocketDownstreamTlsConfig, SocketEndpoint,
    SocketJointEvaluation, SocketLocalRequestPreview, SocketLocalResponderConfig,
    SocketObservationMetadata, SocketOpenedEvidence, SocketPayloadDirection, SocketPipelineLimits,
    SocketProcessingFailure, SocketProcessingFailureKind, SocketProtocolCapabilityFactory,
    SocketRejectionReason, SocketRelayBytes, SocketRelayConfig, SocketRelayDirection,
    SocketRelayFailure, SocketRelayMetricsSnapshot, SocketRelayRunContext, SocketRelaySecurity,
    SocketRelayService, SocketRelayStage, SocketTlsEvidence, SocketTlsIdentity,
    SocketTransportMode, SocketUpstreamConnectionTestResult, SocketUpstreamTlsConfig,
    SocketUpstreamTransport,
};
pub use supervisor::{
    ChannelConfig, ChannelId, DEFAULT_MAX_CONNECTIONS, ProxyConfig, ProxyState, ProxySupervisor,
    RuntimeServiceFactory, RuntimeSnapshot,
};
pub use traffic::{JitterScope, TrafficDirection};
pub use transport::{
    AcceptedConnection, Clock, ConnectionContext, HandshakePolicy, SystemClock,
    TLS_HANDSHAKE_POLICY_TIMEOUT, TlsPeerIdentity, TokioListenerBinder, UpstreamSecurityEvidence,
    UpstreamTransportSecurity,
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
    KeychainUnprotectFailed,
    TlsHandshakeFailed,
    DownstreamTlsHandshakeFailed,
    UpstreamConnectTimeout,
    UpstreamWriteTimeout,
    UpstreamReadTimeout,
    SocketCapacityExhausted,
    SocketDnsFailed,
    SocketDnsTimeout,
    SocketConnectFailed,
    SocketConnectTimeout,
    SocketDownstreamTlsFailed,
    SocketDownstreamTlsTimeout,
    SocketUpstreamTlsFailed,
    SocketUpstreamTlsTimeout,
    SocketReadFailed,
    SocketReadTimeout,
    SocketWriteFailed,
    SocketWriteTimeout,
    SocketRelayCancelled,
    SocketConnectionTaskPanicked,
    BodyTooLarge,
    HeaderLimitExceeded,
    IncorrectContentLength,
    TruncatedResponse,
    FaultStreamAborted,
    FaultExecutionCancelled,
    ClientDisconnected,
    ProxyStopped,
    ExternalPackageCallFailed,
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
            Self::KeychainUnprotectFailed => "KEYCHAIN_UNPROTECT_FAILED",
            Self::TlsHandshakeFailed => "TLS_HANDSHAKE_FAILED",
            Self::DownstreamTlsHandshakeFailed => "DOWNSTREAM_TLS_HANDSHAKE_FAILED",
            Self::UpstreamConnectTimeout => "UPSTREAM_CONNECT_TIMEOUT",
            Self::UpstreamWriteTimeout => "UPSTREAM_WRITE_TIMEOUT",
            Self::UpstreamReadTimeout => "UPSTREAM_READ_TIMEOUT",
            Self::SocketCapacityExhausted => "SOCKET_CAPACITY_EXHAUSTED",
            Self::SocketDnsFailed => "SOCKET_DNS_FAILED",
            Self::SocketDnsTimeout => "SOCKET_DNS_TIMEOUT",
            Self::SocketConnectFailed => "SOCKET_CONNECT_FAILED",
            Self::SocketConnectTimeout => "SOCKET_CONNECT_TIMEOUT",
            Self::SocketDownstreamTlsFailed => "SOCKET_DOWNSTREAM_TLS_FAILED",
            Self::SocketDownstreamTlsTimeout => "SOCKET_DOWNSTREAM_TLS_TIMEOUT",
            Self::SocketUpstreamTlsFailed => "SOCKET_UPSTREAM_TLS_FAILED",
            Self::SocketUpstreamTlsTimeout => "SOCKET_UPSTREAM_TLS_TIMEOUT",
            Self::SocketReadFailed => "SOCKET_READ_FAILED",
            Self::SocketReadTimeout => "SOCKET_READ_TIMEOUT",
            Self::SocketWriteFailed => "SOCKET_WRITE_FAILED",
            Self::SocketWriteTimeout => "SOCKET_WRITE_TIMEOUT",
            Self::SocketRelayCancelled => "SOCKET_RELAY_CANCELLED",
            Self::SocketConnectionTaskPanicked => "SOCKET_CONNECTION_TASK_PANICKED",
            Self::BodyTooLarge => "BODY_TOO_LARGE",
            Self::HeaderLimitExceeded => "HEADER_LIMIT_EXCEEDED",
            Self::IncorrectContentLength => "INCORRECT_CONTENT_LENGTH",
            Self::TruncatedResponse => "TRUNCATED_RESPONSE",
            Self::FaultStreamAborted => "FAULT_STREAM_ABORTED",
            Self::FaultExecutionCancelled => "FAULT_EXECUTION_CANCELLED",
            Self::ClientDisconnected => "BREAKPOINT_CLIENT_DISCONNECTED",
            Self::ProxyStopped => "BREAKPOINT_PROXY_STOPPED",
            Self::ExternalPackageCallFailed => "EXTERNAL_PACKAGE_CALL_FAILED",
            Self::Io => "IO_ERROR",
            Self::Internal => "INTERNAL_ERROR",
        }
    }

    /// 把跨 trait 边界携带的稳定错误码还原为运行时分类。
    ///
    /// Exchange 的错误对象拥有 `String` code，而公开的 `ProxyError` 使用静态 code；集中
    /// 维护反向映射可避免各协议适配器把真实故障降级成 `INTERNAL_ERROR`。
    pub fn from_stable_str(value: &str) -> Option<Self> {
        [
            Self::ProxyAlreadyRunning,
            Self::OperationInProgress,
            Self::PortInUse,
            Self::ConfigInvalid,
            Self::CertificateNotReady,
            Self::CertificateInvalid,
            Self::Pkcs12PasswordInvalid,
            Self::DpapiUnprotectFailed,
            Self::KeychainUnprotectFailed,
            Self::TlsHandshakeFailed,
            Self::DownstreamTlsHandshakeFailed,
            Self::UpstreamConnectTimeout,
            Self::UpstreamWriteTimeout,
            Self::UpstreamReadTimeout,
            Self::SocketCapacityExhausted,
            Self::SocketDnsFailed,
            Self::SocketDnsTimeout,
            Self::SocketConnectFailed,
            Self::SocketConnectTimeout,
            Self::SocketDownstreamTlsFailed,
            Self::SocketDownstreamTlsTimeout,
            Self::SocketUpstreamTlsFailed,
            Self::SocketUpstreamTlsTimeout,
            Self::SocketReadFailed,
            Self::SocketReadTimeout,
            Self::SocketWriteFailed,
            Self::SocketWriteTimeout,
            Self::SocketRelayCancelled,
            Self::SocketConnectionTaskPanicked,
            Self::BodyTooLarge,
            Self::HeaderLimitExceeded,
            Self::IncorrectContentLength,
            Self::TruncatedResponse,
            Self::FaultStreamAborted,
            Self::FaultExecutionCancelled,
            Self::ClientDisconnected,
            Self::ProxyStopped,
            Self::ExternalPackageCallFailed,
            Self::Io,
            Self::Internal,
        ]
        .into_iter()
        .find(|code| code.as_str() == value)
    }
}

#[derive(Debug, Error)]
#[error("{code}: {message}")]
pub struct ProxyError {
    pub code: &'static str,
    pub message: String,
    pub external_package_call: Option<Box<intercept_proxy_exchange::ExternalPackageCallFailure>>,
}

impl ProxyError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str(),
            message: message.into(),
            external_package_call: None,
        }
    }

    #[must_use]
    pub fn with_external_package_call(
        mut self,
        failure: Option<Box<intercept_proxy_exchange::ExternalPackageCallFailure>>,
    ) -> Self {
        self.external_package_call = failure;
        self
    }

    pub fn io(context: &str, error: &io::Error) -> Self {
        Self::new(ErrorCode::Io, format!("{context}: {error}"))
    }
}

pub type Result<T> = std::result::Result<T, ProxyError>;

#[cfg(test)]
mod tests {
    use super::ErrorCode;

    #[test]
    fn keychain_unprotect_error_code_is_stable() {
        assert_eq!(
            ErrorCode::KeychainUnprotectFailed.as_str(),
            "KEYCHAIN_UNPROTECT_FAILED"
        );
    }

    #[test]
    fn stable_error_codes_round_trip_across_exchange_boundaries() {
        for code in [
            ErrorCode::ClientDisconnected,
            ErrorCode::UpstreamConnectTimeout,
            ErrorCode::SocketReadFailed,
        ] {
            assert_eq!(ErrorCode::from_stable_str(code.as_str()), Some(code));
        }
        assert_eq!(ErrorCode::from_stable_str("UNKNOWN"), None);
    }
}
