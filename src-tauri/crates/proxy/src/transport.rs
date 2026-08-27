//! Protocol-neutral listener, relay and byte-stream primitives.

mod contracts;
mod listener;
pub(crate) mod relay;
#[path = "transport/io.rs"]
mod stream_io;

pub use contracts::{
    ConnectionContext, HandshakePolicy, TLS_HANDSHAKE_POLICY_TIMEOUT, UpstreamSecurityEvidence,
    UpstreamTransportSecurity,
};
// Compatibility for callers migrating to the canonical `http::ConnectionService` path.
pub use crate::ConnectionService;
pub(crate) use listener::TokioBoundListener;
pub use listener::{
    AcceptedConnection, BoundListener, Clock, ConnectionAcceptor, ListenerBinder, SystemClock,
    TlsPeerIdentity, TokioListenerBinder,
};
pub use stream_io::{BoxIo, IoStream};
