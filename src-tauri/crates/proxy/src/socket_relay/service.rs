use std::{collections::BTreeMap, fmt, net::SocketAddr, sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;

use crate::listener::{
    ConnectionLifecycleObserver, LISTENER_SHUTDOWN_GRACE_EXCEEDED, ListenerCapacity,
    ListenerConfig, ListenerRejection, ListenerSupervisor, TerminalConnectionOutcome,
};
use crate::transport::relay::RelayBytes;
use crate::transport::{ConnectionContext, SystemClock, TokioBoundListener, TokioListenerBinder};
use crate::{ChannelId, ErrorCode, Result};

use super::{
    NoopSocketConnectionObserver, SocketConnectionEvent, SocketConnectionObserver,
    SocketRejectionReason, SocketRelayBytes, SocketRelayConfig, SocketRelayFailure,
    SocketRelayMetrics, SocketRelayMetricsSnapshot, SocketRelayRunContext, SocketRelayStage,
    SocketUpstreamConnectionTestResult, handler::SocketConnectionHandler,
};

const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

pub struct SocketRelayService {
    config: SocketRelayConfig,
    handler: Arc<SocketConnectionHandler>,
    lifecycle: Arc<SocketLifecycleAdapter>,
    metrics: Arc<SocketRelayMetrics>,
    run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
}

impl fmt::Debug for SocketRelayService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketRelayService")
            .field("bind_addr", &self.config.bind_addr)
            .field("upstream", &self.config.upstream)
            .field("security", &self.config.security)
            .field("maximum_connections", &self.config.maximum_connections)
            .finish_non_exhaustive()
    }
}

impl SocketRelayService {
    pub fn build(config: SocketRelayConfig) -> Result<Self> {
        Self::build_with_observer(config, Arc::new(NoopSocketConnectionObserver))
    }

    pub fn build_with_observer(
        config: SocketRelayConfig,
        observer: Arc<dyn SocketConnectionObserver>,
    ) -> Result<Self> {
        let metrics = Arc::new(SocketRelayMetrics::default());
        let events = Arc::new(SocketEventCoordinator::new(observer));
        let run = Arc::new(std::sync::RwLock::new(SocketRelayRunContext {
            listener_id: format!("socket-{}", config.bind_addr.port()),
            workspace_runtime_epoch: uuid::Uuid::nil(),
            listener_run_epoch: uuid::Uuid::nil(),
        }));
        let handler = Arc::new(SocketConnectionHandler::build(
            config.clone(),
            events.clone(),
            Arc::clone(&metrics),
            Arc::clone(&run),
        )?);
        Ok(Self {
            config,
            handler,
            lifecycle: Arc::new(SocketLifecycleAdapter {
                events,
                metrics: Arc::clone(&metrics),
                run: Arc::clone(&run),
            }),
            metrics,
            run,
        })
    }

    pub async fn serve(&self, cancellation: CancellationToken) -> Result<()> {
        self.serve_with_run_id(uuid::Uuid::new_v4(), cancellation)
            .await
    }

    pub async fn serve_with_run_id(
        &self,
        run_id: uuid::Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let run = self.compatible_run_context(run_id);
        self.serve_with_context(run, cancellation).await
    }

    pub async fn serve_with_context(
        &self,
        run: SocketRelayRunContext,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let supervisor = self.supervisor(&run)?;
        supervisor
            .bind_and_run(cancellation)
            .await?
            .into_result("socket listener stopped after a fatal lifecycle failure")
    }

    pub async fn serve_listener(
        &self,
        listener: tokio::net::TcpListener,
        run_id: uuid::Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let run = self.compatible_run_context(run_id);
        self.serve_listener_with_context(listener, run, cancellation)
            .await
    }

    pub async fn serve_listener_with_context(
        &self,
        listener: tokio::net::TcpListener,
        run: SocketRelayRunContext,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let supervisor = self.supervisor(&run)?;
        supervisor
            .run_bound(Arc::new(TokioBoundListener(listener)), cancellation)
            .await?
            .into_result("socket listener stopped after a fatal lifecycle failure")
    }

    pub async fn metrics(&self) -> SocketRelayMetricsSnapshot {
        std::future::ready(
            self.metrics
                .snapshot(self.lifecycle.events.retained_diagnostic_evictions()),
        )
        .await
    }

    pub async fn test_upstream_connection(&self) -> Result<SocketUpstreamConnectionTestResult> {
        self.handler.test_upstream_connection().await
    }

    fn supervisor(
        &self,
        run: &SocketRelayRunContext,
    ) -> Result<ListenerSupervisor<SocketConnectionHandler>> {
        self.metrics.reset();
        self.lifecycle.events.begin_run();
        *self
            .run
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = run.clone();
        let listener_id = ChannelId::new(run.listener_id.clone())?;
        ListenerSupervisor::new(
            ListenerConfig {
                bind_addr: self.config.bind_addr,
                runtime_epoch: run.listener_run_epoch,
                listener_id,
                allowed_client_cidrs: self.config.allowed_client_cidrs.clone(),
                capacity: ListenerCapacity::new(usize::from(self.config.maximum_connections))?,
                shutdown_grace: DEFAULT_SHUTDOWN_GRACE,
            },
            Arc::new(TokioListenerBinder),
            Arc::new(SystemClock),
            Arc::clone(&self.handler),
            self.lifecycle.clone(),
        )
    }

    fn compatible_run_context(&self, run_id: uuid::Uuid) -> SocketRelayRunContext {
        SocketRelayRunContext {
            listener_id: format!("socket-{}", self.config.bind_addr.port()),
            workspace_runtime_epoch: run_id,
            listener_run_epoch: run_id,
        }
    }
}

#[derive(Debug)]
struct SocketLifecycleAdapter {
    events: Arc<SocketEventCoordinator>,
    metrics: Arc<SocketRelayMetrics>,
    run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
}

impl ConnectionLifecycleObserver for SocketLifecycleAdapter {
    fn connection_rejected(&self, peer: SocketAddr, reason: ListenerRejection) {
        let run = self
            .run
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let (reason, code) = match reason {
            ListenerRejection::NetworkDenied => (
                SocketRejectionReason::Cidr,
                ErrorCode::SocketCidrDenied.as_str(),
            ),
            ListenerRejection::CapacityExhausted => (
                SocketRejectionReason::Capacity,
                ErrorCode::SocketCapacityExhausted.as_str(),
            ),
        };
        self.metrics.rejected();
        self.events.record(SocketConnectionEvent::Rejected {
            run,
            peer,
            reason,
            code,
        });
    }

    fn connection_terminal(
        &self,
        context: &ConnectionContext,
        outcome: &TerminalConnectionOutcome,
    ) {
        let Some(opened) = self.events.take_unclosed(context.connection_id) else {
            return;
        };
        let bytes = self
            .metrics
            .closed(context.connection_id, opened, RelayBytes::default());
        let run = self
            .run
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.events.record(SocketConnectionEvent::Closed {
            run,
            connection_id: context.connection_id,
            opened,
            bytes: SocketRelayBytes::from(bytes),
            failure: terminal_failure(outcome),
            at: std::time::SystemTime::now(),
        });
    }
}

fn terminal_failure(outcome: &TerminalConnectionOutcome) -> Option<SocketRelayFailure> {
    let code = match outcome {
        TerminalConnectionOutcome::Success => return None,
        TerminalConnectionOutcome::Cancelled => ErrorCode::SocketRelayCancelled.as_str(),
        TerminalConnectionOutcome::Failed { code, .. } => code,
        TerminalConnectionOutcome::ChildTaskPanicked => {
            ErrorCode::SocketConnectionTaskPanicked.as_str()
        }
        TerminalConnectionOutcome::ShutdownGraceExceeded => LISTENER_SHUTDOWN_GRACE_EXCEEDED,
    };
    Some(SocketRelayFailure {
        stage: SocketRelayStage::Shutdown,
        direction: None,
        code,
    })
}

#[derive(Debug)]
struct SocketEventCoordinator {
    observer: Arc<dyn SocketConnectionObserver>,
    unclosed: std::sync::Mutex<BTreeMap<uuid::Uuid, bool>>,
}

impl SocketEventCoordinator {
    fn new(observer: Arc<dyn SocketConnectionObserver>) -> Self {
        Self {
            observer,
            unclosed: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    fn take_unclosed(&self, connection_id: uuid::Uuid) -> Option<bool> {
        self.unclosed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&connection_id)
    }

    fn begin_run(&self) {
        self.unclosed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.observer.begin_run();
    }

    fn retained_diagnostic_evictions(&self) -> u64 {
        self.observer.retained_diagnostic_evictions()
    }
}

impl SocketConnectionObserver for SocketEventCoordinator {
    fn record(&self, event: SocketConnectionEvent) {
        match &event {
            SocketConnectionEvent::Admitted { connection_id, .. } => {
                self.unclosed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(*connection_id, false);
            }
            SocketConnectionEvent::Opened { connection_id, .. } => {
                if let Some(opened) = self
                    .unclosed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get_mut(connection_id)
                {
                    *opened = true;
                }
            }
            SocketConnectionEvent::Closed { connection_id, .. } => {
                self.take_unclosed(*connection_id);
            }
            SocketConnectionEvent::Rejected { .. } => {}
        }
        self.observer.record(event);
    }

    fn begin_run(&self) {
        Self::begin_run(self);
    }

    fn retained_diagnostic_evictions(&self) -> u64 {
        Self::retained_diagnostic_evictions(self)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
