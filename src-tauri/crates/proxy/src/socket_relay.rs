//! Protocol-neutral fixed-target Socket Relay data plane.
//!
//! This module never parses HTTP or application payloads. Transparent mode relays TCP bytes
//! unchanged, including end-to-end TLS. Bridge modes terminate and/or originate TLS explicitly.

mod config;
mod connector;
mod handler;
mod observer;
mod service;

pub use config::{
    SocketDownstreamTlsConfig, SocketEndpoint, SocketRelayConfig, SocketRelaySecurity,
    SocketTlsIdentity, SocketUpstreamConnectionTestResult, SocketUpstreamTlsConfig,
    SocketUpstreamTransport,
};
pub use observer::{
    BoundedSocketConnectionObserver, NoopSocketConnectionObserver, SocketConnectionEvent,
    SocketConnectionObserver, SocketRejectionReason, SocketRelayBytes, SocketRelayDirection,
    SocketRelayFailure, SocketRelayMetricsSnapshot, SocketRelayRunContext, SocketRelayStage,
    SocketTlsEvidence, SocketTransportMode,
};
pub use service::SocketRelayService;

use observer::SocketRelayMetrics;

#[cfg(test)]
mod tests;
