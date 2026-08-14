//! Protocol-neutral Socket Relay and `LocalResponder` data plane.
//!
//! This module never parses HTTP and never knows Rhai, Document or rule types. Direct Relay keeps
//! forwarding TCP bytes unchanged; Scripted Relay and `LocalResponder` only coordinate the generic
//! bounded [`SocketFrameProcessor`] boundary. TLS termination/origination remains explicit in the
//! selected topology.

mod config;
mod connector;
pub(crate) mod frame_pump;
mod handler;
mod handler_support;
mod observer;
mod processing;
mod service;

pub use config::{
    SocketDownstreamSecurity, SocketDownstreamTlsConfig, SocketEndpoint,
    SocketLocalResponderConfig, SocketRelayConfig, SocketRelaySecurity, SocketTlsIdentity,
    SocketUpstreamConnectionTestResult, SocketUpstreamTlsConfig, SocketUpstreamTransport,
};
pub use observer::{
    BoundedSocketConnectionObserver, NoopSocketConnectionObserver, SocketConnectionEvent,
    SocketConnectionObserver, SocketConnectionTarget, SocketOpenedEvidence, SocketRejectionReason,
    SocketRelayBytes, SocketRelayDirection, SocketRelayFailure, SocketRelayMetricsSnapshot,
    SocketRelayRunContext, SocketRelayStage, SocketTlsEvidence, SocketTransportMode,
};
pub use processing::{
    FrameBoundary, LocalResponderProcessorFactory, ScriptedRelayProcessorFactory,
    SocketConnectionIdentity, SocketFrameProcessor, SocketFramePumpLimits, SocketPayloadDirection,
    SocketProcessingFailure, SocketProcessingFailureKind,
};
pub use service::SocketRelayService;

use observer::SocketRelayMetrics;

#[cfg(test)]
mod tests;
