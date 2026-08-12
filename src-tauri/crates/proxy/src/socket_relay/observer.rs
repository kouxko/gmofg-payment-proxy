use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use uuid::Uuid;

use crate::transport::relay::{RelayBytes, RelayProgress};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SocketRelayBytes {
    pub client_to_server: u64,
    pub server_to_client: u64,
}

impl From<RelayBytes> for SocketRelayBytes {
    fn from(bytes: RelayBytes) -> Self {
        Self {
            client_to_server: bytes.client_to_server,
            server_to_client: bytes.server_to_client,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketTransportMode {
    Transparent,
    TcpToTls,
    TlsToTcp,
    TlsToTls,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketTlsEvidence {
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer_subject: String,
    pub peer_sha256_fingerprint: String,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketRelayRunContext {
    pub listener_id: String,
    pub workspace_runtime_epoch: Uuid,
    pub listener_run_epoch: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketRelayStage {
    Admission,
    DownstreamTls,
    Dns,
    Connect,
    UpstreamTls,
    RelayRead,
    RelayWrite,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketRelayDirection {
    Downstream,
    Upstream,
    ClientToServer,
    ServerToClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketRejectionReason {
    Cidr,
    Capacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketRelayFailure {
    pub stage: SocketRelayStage,
    pub direction: Option<SocketRelayDirection>,
    pub code: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketConnectionEvent {
    Rejected {
        run: SocketRelayRunContext,
        peer: SocketAddr,
        reason: SocketRejectionReason,
        code: &'static str,
    },
    Admitted {
        run: SocketRelayRunContext,
        connection_id: Uuid,
        peer: SocketAddr,
        target: String,
        mode: SocketTransportMode,
        at: SystemTime,
    },
    Opened {
        run: SocketRelayRunContext,
        connection_id: Uuid,
        resolved_address: SocketAddr,
        downstream_tls_peer: Option<String>,
        upstream_tls: Option<SocketTlsEvidence>,
        at: SystemTime,
    },
    Closed {
        run: SocketRelayRunContext,
        connection_id: Uuid,
        opened: bool,
        bytes: SocketRelayBytes,
        failure: Option<SocketRelayFailure>,
        at: SystemTime,
    },
}

/// Synchronous data-plane event port. Implementations must remain bounded and must not perform
/// network or disk I/O inline.
pub trait SocketConnectionObserver: std::fmt::Debug + Send + Sync {
    fn record(&self, event: SocketConnectionEvent);

    fn begin_run(&self) {}

    fn retained_diagnostic_evictions(&self) -> u64 {
        0
    }
}

#[derive(Debug, Default)]
pub struct NoopSocketConnectionObserver;

impl SocketConnectionObserver for NoopSocketConnectionObserver {
    fn record(&self, _event: SocketConnectionEvent) {}
}

#[derive(Debug)]
pub struct BoundedSocketConnectionObserver {
    capacity: usize,
    retained: Mutex<VecDeque<SocketConnectionEvent>>,
    dropped: AtomicU64,
}

impl BoundedSocketConnectionObserver {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            retained: Mutex::new(VecDeque::with_capacity(capacity.max(1))),
            dropped: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<SocketConnectionEvent> {
        self.retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

impl SocketConnectionObserver for BoundedSocketConnectionObserver {
    fn record(&self, event: SocketConnectionEvent) {
        let mut retained = self
            .retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if retained.len() == self.capacity {
            retained.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        retained.push_back(event);
    }

    fn begin_run(&self) {
        self.retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.dropped.store(0, Ordering::Relaxed);
    }

    fn retained_diagnostic_evictions(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SocketRelayMetricsSnapshot {
    pub active_connections: u64,
    pub admitted_connections: u64,
    pub rejected_connections: u64,
    pub client_to_server_bytes: u64,
    pub server_to_client_bytes: u64,
    pub retained_diagnostic_evictions: u64,
}

#[derive(Debug, Default)]
pub(crate) struct SocketRelayMetrics {
    active_connections: AtomicU64,
    admitted_connections: AtomicU64,
    rejected_connections: AtomicU64,
    client_to_server_bytes: AtomicU64,
    server_to_client_bytes: AtomicU64,
    connections: Mutex<std::collections::BTreeMap<Uuid, (bool, std::sync::Arc<RelayProgress>)>>,
}

impl SocketRelayMetrics {
    pub(crate) fn reset(&self) {
        self.active_connections.store(0, Ordering::Relaxed);
        self.admitted_connections.store(0, Ordering::Relaxed);
        self.rejected_connections.store(0, Ordering::Relaxed);
        self.client_to_server_bytes.store(0, Ordering::Relaxed);
        self.server_to_client_bytes.store(0, Ordering::Relaxed);
        self.connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(crate) fn admitted(&self, connection_id: Uuid, progress: std::sync::Arc<RelayProgress>) {
        self.admitted_connections.fetch_add(1, Ordering::Relaxed);
        self.connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(connection_id, (false, progress));
    }

    pub(crate) fn rejected(&self) {
        self.rejected_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn opened(&self, connection_id: Uuid) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        if let Some((opened, _)) = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&connection_id)
        {
            *opened = true;
        }
    }

    pub(crate) fn closed(
        &self,
        connection_id: Uuid,
        opened: bool,
        bytes: RelayBytes,
    ) -> RelayBytes {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tracked = connections
            .remove(&connection_id)
            .map_or(bytes, |(_, progress)| progress.snapshot());
        if opened {
            self.active_connections
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                    Some(active.saturating_sub(1))
                })
                .ok();
        }
        self.client_to_server_bytes
            .fetch_add(tracked.client_to_server, Ordering::Relaxed);
        self.server_to_client_bytes
            .fetch_add(tracked.server_to_client, Ordering::Relaxed);
        tracked
    }

    pub(crate) fn snapshot(
        &self,
        retained_diagnostic_evictions: u64,
    ) -> SocketRelayMetricsSnapshot {
        let connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let active_bytes =
            connections
                .values()
                .fold(RelayBytes::default(), |mut total, (_, progress)| {
                    let current = progress.snapshot();
                    total.client_to_server = total
                        .client_to_server
                        .saturating_add(current.client_to_server);
                    total.server_to_client = total
                        .server_to_client
                        .saturating_add(current.server_to_client);
                    total
                });
        SocketRelayMetricsSnapshot {
            active_connections: self.active_connections.load(Ordering::Relaxed),
            admitted_connections: self.admitted_connections.load(Ordering::Relaxed),
            rejected_connections: self.rejected_connections.load(Ordering::Relaxed),
            client_to_server_bytes: self
                .client_to_server_bytes
                .load(Ordering::Relaxed)
                .saturating_add(active_bytes.client_to_server),
            server_to_client_bytes: self
                .server_to_client_bytes
                .load(Ordering::Relaxed)
                .saturating_add(active_bytes.server_to_client),
            retained_diagnostic_evictions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejected(index: u16) -> SocketConnectionEvent {
        SocketConnectionEvent::Rejected {
            run: SocketRelayRunContext {
                listener_id: "listener-1".into(),
                workspace_runtime_epoch: Uuid::nil(),
                listener_run_epoch: Uuid::nil(),
            },
            peer: SocketAddr::from(([127, 0, 0, 1], index)),
            reason: SocketRejectionReason::Capacity,
            code: "SOCKET_CAPACITY_EXHAUSTED",
        }
    }

    #[test]
    fn bounded_retention_evicts_oldest_without_blocking_and_counts_drops() {
        let observer = BoundedSocketConnectionObserver::new(2);
        observer.record(rejected(1));
        observer.record(rejected(2));
        observer.record(rejected(3));

        assert_eq!(observer.snapshot(), vec![rejected(2), rejected(3)]);
        assert_eq!(observer.retained_diagnostic_evictions(), 1);

        observer.begin_run();
        assert!(observer.snapshot().is_empty());
        assert_eq!(observer.retained_diagnostic_evictions(), 0);
    }
}
