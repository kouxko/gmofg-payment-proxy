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

use crate::transport::relay::{RelayBytes, RelayIoBytes, RelayProgress};
use crate::{ErrorCode, ProxyError, Result};

mod local_request;

pub use local_request::{
    SocketDocumentFieldPreview, SocketDocumentPreview, SocketLocalRequestPreview,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SocketRelayBytes {
    /// 从 App 侧实际读取、准备发往处理端/上游的 origin 字节数。
    pub client_to_server_read: u64,
    /// 实际提交到固定上游的输出字节数；LocalResponder 始终为零。
    pub client_to_server: u64,
    /// 从固定上游实际读取的 origin 字节数；LocalResponder 始终为零。
    pub server_to_client_read: u64,
    /// 实际提交到 App 侧的输出字节数。
    pub server_to_client: u64,
}

impl From<RelayBytes> for SocketRelayBytes {
    fn from(bytes: RelayBytes) -> Self {
        Self {
            client_to_server_read: 0,
            client_to_server: bytes.client_to_server,
            server_to_client_read: 0,
            server_to_client: bytes.server_to_client,
        }
    }
}

impl From<RelayIoBytes> for SocketRelayBytes {
    fn from(bytes: RelayIoBytes) -> Self {
        Self {
            client_to_server_read: bytes.read.client_to_server,
            client_to_server: bytes.written.client_to_server,
            server_to_client_read: bytes.read.server_to_client,
            server_to_client: bytes.written.server_to_client,
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
    pub workspace_id: String,
    pub listener_id: String,
    pub workspace_runtime_epoch: Uuid,
    pub listener_run_epoch: Uuid,
}

/// 被接纳连接的实际处理目标。
///
/// `LocalResponder` 没有上游地址。使用枚举而不是空字符串，能够阻止观察层和 UI 把
/// 本地应答误展示成一次上游连接。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketConnectionTarget {
    Relay(String),
    LocalResponder,
}

/// 数据面准备完成后可被审计的、与模式一致的连接证据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketOpenedEvidence {
    Relay {
        resolved_address: SocketAddr,
        downstream_tls_peer: Option<String>,
        upstream_tls: Option<SocketTlsEvidence>,
    },
    LocalResponder {
        downstream_tls_peer: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketRelayStage {
    Admission,
    DownstreamTls,
    Dns,
    Connect,
    UpstreamTls,
    RelayRead,
    /// 已读取字节正在等待处理器确认完整 Frame 边界。
    FrameInspect,
    /// 完整 Frame 的 Decode 阶段。
    Decode,
    /// Document 规则阶段。
    Rule,
    /// 下一跳字节的 Encode 阶段。
    Encode,
    /// 完整 Frame 正由处理器生成下一跳输出。
    FrameProcess,
    RelayWrite,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketRelayDirection {
    Downstream,
    Upstream,
    ClientToServer,
    ServerToClient,
    /// `LocalResponder` 的一次 App request → local response 串行交换。
    LocalExchange,
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
        target: SocketConnectionTarget,
        mode: SocketTransportMode,
        at: SystemTime,
    },
    Opened {
        run: SocketRelayRunContext,
        connection_id: Uuid,
        evidence: SocketOpenedEvidence,
        at: SystemTime,
    },
    /// `LocalResponder` 已完成 request Frame 与可选 Decode，但尚未生成或提交 response。
    RequestParsed {
        run: SocketRelayRunContext,
        connection_id: Uuid,
        preview: SocketLocalRequestPreview,
        at: SystemTime,
    },
    Closed {
        run: SocketRelayRunContext,
        connection_id: Uuid,
        target: SocketConnectionTarget,
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
    max_logical_bytes: usize,
    retained: Mutex<VecDeque<SocketConnectionEvent>>,
    retained_logical_bytes: Mutex<usize>,
    dropped: AtomicU64,
}

impl BoundedSocketConnectionObserver {
    pub fn new(capacity: usize) -> Result<Self> {
        Self::with_limits(capacity, usize::MAX)
    }

    /// 创建同时受事件条数和逻辑字节数约束的内存观察队列。
    pub fn with_limits(capacity: usize, max_logical_bytes: usize) -> Result<Self> {
        if capacity == 0 || max_logical_bytes == 0 {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "socket diagnostic limits must be greater than zero",
            ));
        }
        Ok(Self {
            capacity,
            max_logical_bytes,
            retained: Mutex::new(VecDeque::with_capacity(capacity)),
            retained_logical_bytes: Mutex::new(0),
            dropped: AtomicU64::new(0),
        })
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
        let event_bytes = event.logical_bytes();
        let mut retained = self
            .retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut retained_bytes = self
            .retained_logical_bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while retained.len() == self.capacity
            || retained_bytes.saturating_add(event_bytes) > self.max_logical_bytes
        {
            let Some(removed) = retained.pop_front() else {
                break;
            };
            *retained_bytes = retained_bytes.saturating_sub(removed.logical_bytes());
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        if event_bytes > self.max_logical_bytes {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        *retained_bytes = retained_bytes.saturating_add(event_bytes);
        retained.push_back(event);
    }

    fn begin_run(&self) {
        self.retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        *self
            .retained_logical_bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = 0;
        self.dropped.store(0, Ordering::Relaxed);
    }

    fn retained_diagnostic_evictions(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl SocketConnectionEvent {
    fn logical_bytes(&self) -> usize {
        match self {
            Self::RequestParsed { preview, .. } => preview.logical_bytes(),
            // 连接生命周期事件只保留短标识和固定统计。使用稳定上界避免为了内存计量
            // 序列化数据面事件，也不会低估真正占大头的 request preview。
            Self::Rejected { .. }
            | Self::Admitted { .. }
            | Self::Opened { .. }
            | Self::Closed { .. } => 512,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SocketRelayMetricsSnapshot {
    pub active_connections: u64,
    pub admitted_connections: u64,
    pub rejected_connections: u64,
    pub client_to_server_read_bytes: u64,
    pub client_to_server_bytes: u64,
    pub server_to_client_read_bytes: u64,
    pub server_to_client_bytes: u64,
    pub retained_diagnostic_evictions: u64,
}

#[derive(Debug, Default)]
pub(crate) struct SocketRelayMetrics {
    active_connections: AtomicU64,
    admitted_connections: AtomicU64,
    rejected_connections: AtomicU64,
    client_to_server_read_bytes: AtomicU64,
    client_to_server_bytes: AtomicU64,
    server_to_client_read_bytes: AtomicU64,
    server_to_client_bytes: AtomicU64,
    connections: Mutex<std::collections::BTreeMap<Uuid, (bool, std::sync::Arc<RelayProgress>)>>,
}

impl SocketRelayMetrics {
    pub(crate) fn reset(&self) {
        self.active_connections.store(0, Ordering::Relaxed);
        self.admitted_connections.store(0, Ordering::Relaxed);
        self.rejected_connections.store(0, Ordering::Relaxed);
        self.client_to_server_read_bytes.store(0, Ordering::Relaxed);
        self.client_to_server_bytes.store(0, Ordering::Relaxed);
        self.server_to_client_read_bytes.store(0, Ordering::Relaxed);
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
    ) -> SocketRelayBytes {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tracked = connections.remove(&connection_id).map_or(
            RelayIoBytes {
                read: RelayBytes::default(),
                written: bytes,
            },
            |(_, progress)| progress.io_snapshot(),
        );
        if opened {
            self.active_connections
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                    Some(active.saturating_sub(1))
                })
                .ok();
        }
        self.client_to_server_read_bytes
            .fetch_add(tracked.read.client_to_server, Ordering::Relaxed);
        self.client_to_server_bytes
            .fetch_add(tracked.written.client_to_server, Ordering::Relaxed);
        self.server_to_client_read_bytes
            .fetch_add(tracked.read.server_to_client, Ordering::Relaxed);
        self.server_to_client_bytes
            .fetch_add(tracked.written.server_to_client, Ordering::Relaxed);
        SocketRelayBytes::from(tracked)
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
                .fold(RelayIoBytes::default(), |mut total, (_, progress)| {
                    let current = progress.io_snapshot();
                    total.read.client_to_server = total
                        .read
                        .client_to_server
                        .saturating_add(current.read.client_to_server);
                    total.read.server_to_client = total
                        .read
                        .server_to_client
                        .saturating_add(current.read.server_to_client);
                    total.written.client_to_server = total
                        .written
                        .client_to_server
                        .saturating_add(current.written.client_to_server);
                    total.written.server_to_client = total
                        .written
                        .server_to_client
                        .saturating_add(current.written.server_to_client);
                    total
                });
        SocketRelayMetricsSnapshot {
            active_connections: self.active_connections.load(Ordering::Relaxed),
            admitted_connections: self.admitted_connections.load(Ordering::Relaxed),
            rejected_connections: self.rejected_connections.load(Ordering::Relaxed),
            client_to_server_read_bytes: self
                .client_to_server_read_bytes
                .load(Ordering::Relaxed)
                .saturating_add(active_bytes.read.client_to_server),
            client_to_server_bytes: self
                .client_to_server_bytes
                .load(Ordering::Relaxed)
                .saturating_add(active_bytes.written.client_to_server),
            server_to_client_read_bytes: self
                .server_to_client_read_bytes
                .load(Ordering::Relaxed)
                .saturating_add(active_bytes.read.server_to_client),
            server_to_client_bytes: self
                .server_to_client_bytes
                .load(Ordering::Relaxed)
                .saturating_add(active_bytes.written.server_to_client),
            retained_diagnostic_evictions,
        }
    }
}

#[cfg(test)]
mod tests;
