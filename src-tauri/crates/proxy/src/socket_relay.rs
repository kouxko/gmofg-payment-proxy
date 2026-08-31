//! Protocol-neutral Socket Relay and `LocalResponder` data plane.
//!
//! This module never parses HTTP and never knows JavaScript, Document or rule types. Direct Relay keeps
//! forwarding TCP bytes unchanged; Scripted Relay and `LocalResponder` only coordinate the generic
//! bounded [`SocketDirectionCapabilities`] boundary. TLS termination/origination remains explicit in the
//! selected topology.

mod config;
mod connector;
mod handler;
mod handler_support;
mod observer;
mod processing;
mod protocol_exchange;
mod raw_exchange;
mod service;
mod upstream_tls;

pub use config::{
    SocketDownstreamSecurity, SocketDownstreamTlsConfig, SocketEndpoint,
    SocketLocalResponderConfig, SocketRelayConfig, SocketRelaySecurity, SocketTlsIdentity,
    SocketUpstreamConnectionTestResult, SocketUpstreamTlsConfig, SocketUpstreamTransport,
};
pub use observer::{
    BoundedSocketConnectionObserver, NoopSocketConnectionObserver, SocketConnectionEvent,
    SocketConnectionObserver, SocketConnectionTarget, SocketDocumentFieldPreview,
    SocketDocumentPreview, SocketLocalRequestPreview, SocketOpenedEvidence, SocketRejectionReason,
    SocketRelayBytes, SocketRelayDirection, SocketRelayFailure, SocketRelayMetricsSnapshot,
    SocketRelayRunContext, SocketRelayStage, SocketTlsEvidence, SocketTransportMode,
};
pub use processing::{
    FrameBoundary, JointConditionEvaluation, JointRuleConditionEvaluation,
    SocketConnectionIdentity, SocketDirectionCapabilities, SocketJointEvaluation,
    SocketObservationMetadata, SocketPayloadDirection, SocketPipelineLimits,
    SocketProcessingFailure, SocketProcessingFailureKind, SocketProtocolCapabilityFactory,
};
pub use service::SocketRelayService;

use observer::SocketRelayMetrics;

#[cfg(test)]
mod tests;
