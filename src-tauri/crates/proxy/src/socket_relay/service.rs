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
    LocalResponderProcessorFactory, NoopSocketConnectionObserver, ScriptedRelayProcessorFactory,
    SocketConnectionEvent, SocketConnectionObserver, SocketFramePumpLimits,
    SocketLocalResponderConfig, SocketRejectionReason, SocketRelayConfig, SocketRelayFailure,
    SocketRelayMetrics, SocketRelayMetricsSnapshot, SocketRelayRunContext, SocketRelayStage,
    SocketUpstreamConnectionTestResult, handler::SocketConnectionHandler,
};

const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

pub struct SocketRelayService {
    config: SocketListenerConfig,
    handler: Arc<SocketConnectionHandler>,
    lifecycle: Arc<SocketLifecycleAdapter>,
    metrics: Arc<SocketRelayMetrics>,
    run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
}

impl fmt::Debug for SocketRelayService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut output = formatter.debug_struct("SocketRelayService");
        output
            .field("bind_addr", &self.config.bind_addr())
            .field("maximum_connections", &self.config.maximum_connections());
        match &self.config {
            SocketListenerConfig::Relay(config) => output
                .field("topology", &"relay")
                .field("upstream", &config.upstream)
                .field("security", &config.security),
            SocketListenerConfig::LocalResponder(config) => output
                .field("topology", &"local_responder")
                .field("security", &config.security),
        };
        output.finish_non_exhaustive()
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
        let handler_config = config.clone();
        let handler_builder = move |observer, metrics, run| {
            SocketConnectionHandler::build_direct(handler_config, observer, metrics, run)
        };
        Self::build_inner(
            SocketListenerConfig::Relay(config),
            observer,
            handler_builder,
        )
    }

    /// 构造使用脚本 processor 的固定上游双向 Relay。
    pub fn build_scripted(
        config: SocketRelayConfig,
        factory: Arc<dyn ScriptedRelayProcessorFactory>,
        limits: SocketFramePumpLimits,
    ) -> Result<Self> {
        Self::build_scripted_with_observer(
            config,
            factory,
            limits,
            Arc::new(NoopSocketConnectionObserver),
        )
    }

    /// 构造带 observer 的 Scripted Relay；processor 每连接每方向各创建一次。
    pub fn build_scripted_with_observer(
        config: SocketRelayConfig,
        factory: Arc<dyn ScriptedRelayProcessorFactory>,
        limits: SocketFramePumpLimits,
        observer: Arc<dyn SocketConnectionObserver>,
    ) -> Result<Self> {
        let handler_factory = factory;
        let handler_config = config.clone();
        let handler_builder = move |observer, metrics, run| {
            SocketConnectionHandler::build_scripted(
                handler_config,
                Arc::clone(&handler_factory),
                limits,
                observer,
                metrics,
                run,
            )
        };
        Self::build_inner(
            SocketListenerConfig::Relay(config),
            observer,
            handler_builder,
        )
    }

    /// 构造不包含任何上游能力的本地应答 Listener。
    pub fn build_local_responder(
        config: SocketLocalResponderConfig,
        factory: Arc<dyn LocalResponderProcessorFactory>,
        limits: SocketFramePumpLimits,
    ) -> Result<Self> {
        Self::build_local_responder_with_observer(
            config,
            factory,
            limits,
            Arc::new(NoopSocketConnectionObserver),
        )
    }

    /// 构造带 observer 的本地应答 Listener。
    pub fn build_local_responder_with_observer(
        config: SocketLocalResponderConfig,
        factory: Arc<dyn LocalResponderProcessorFactory>,
        limits: SocketFramePumpLimits,
        observer: Arc<dyn SocketConnectionObserver>,
    ) -> Result<Self> {
        let handler_factory = factory;
        let handler_config = config.clone();
        let handler_builder = move |observer, metrics, run| {
            SocketConnectionHandler::build_local_responder(
                handler_config,
                Arc::clone(&handler_factory),
                limits,
                observer,
                metrics,
                run,
            )
        };
        Self::build_inner(
            SocketListenerConfig::LocalResponder(config),
            observer,
            handler_builder,
        )
    }

    fn build_inner<F>(
        config: SocketListenerConfig,
        observer: Arc<dyn SocketConnectionObserver>,
        handler_builder: F,
    ) -> Result<Self>
    where
        F: FnOnce(
            Arc<dyn SocketConnectionObserver>,
            Arc<SocketRelayMetrics>,
            Arc<std::sync::RwLock<SocketRelayRunContext>>,
        ) -> Result<SocketConnectionHandler>,
    {
        let metrics = Arc::new(SocketRelayMetrics::default());
        let events = Arc::new(SocketEventCoordinator::new(observer));
        let run = Arc::new(std::sync::RwLock::new(SocketRelayRunContext {
            listener_id: format!("socket-{}", config.bind_addr().port()),
            workspace_runtime_epoch: uuid::Uuid::nil(),
            listener_run_epoch: uuid::Uuid::nil(),
        }));
        let handler = Arc::new(handler_builder(
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
                bind_addr: self.config.bind_addr(),
                runtime_epoch: run.listener_run_epoch,
                listener_id,
                allowed_client_cidrs: self.config.allowed_client_cidrs().to_vec(),
                capacity: ListenerCapacity::new(usize::from(self.config.maximum_connections()))?,
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
            listener_id: format!("socket-{}", self.config.bind_addr().port()),
            workspace_runtime_epoch: run_id,
            listener_run_epoch: run_id,
        }
    }
}

#[derive(Clone, Debug)]
enum SocketListenerConfig {
    Relay(SocketRelayConfig),
    LocalResponder(SocketLocalResponderConfig),
}

impl SocketListenerConfig {
    fn bind_addr(&self) -> SocketAddr {
        match self {
            Self::Relay(config) => config.bind_addr,
            Self::LocalResponder(config) => config.bind_addr,
        }
    }

    fn allowed_client_cidrs(&self) -> &[String] {
        match self {
            Self::Relay(config) => &config.allowed_client_cidrs,
            Self::LocalResponder(config) => &config.allowed_client_cidrs,
        }
    }

    fn maximum_connections(&self) -> u16 {
        match self {
            Self::Relay(config) => config.maximum_connections,
            Self::LocalResponder(config) => config.maximum_connections,
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
        let Some(tracked) = self.events.take_unclosed(context.connection_id) else {
            return;
        };
        let bytes =
            self.metrics
                .closed(context.connection_id, tracked.opened, RelayBytes::default());
        let run = self
            .run
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.events.record(SocketConnectionEvent::Closed {
            run,
            connection_id: context.connection_id,
            target: tracked.target,
            opened: tracked.opened,
            bytes,
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
    unclosed: std::sync::Mutex<BTreeMap<uuid::Uuid, TrackedSocketConnection>>,
}

#[derive(Clone, Debug)]
struct TrackedSocketConnection {
    opened: bool,
    target: super::SocketConnectionTarget,
}

impl SocketEventCoordinator {
    fn new(observer: Arc<dyn SocketConnectionObserver>) -> Self {
        Self {
            observer,
            unclosed: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    fn take_unclosed(&self, connection_id: uuid::Uuid) -> Option<TrackedSocketConnection> {
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
            SocketConnectionEvent::Admitted {
                connection_id,
                target,
                ..
            } => {
                self.unclosed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(
                        *connection_id,
                        TrackedSocketConnection {
                            opened: false,
                            target: target.clone(),
                        },
                    );
            }
            SocketConnectionEvent::Opened { connection_id, .. } => {
                if let Some(opened) = self
                    .unclosed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get_mut(connection_id)
                {
                    opened.opened = true;
                }
            }
            SocketConnectionEvent::Closed { connection_id, .. } => {
                self.take_unclosed(*connection_id);
            }
            SocketConnectionEvent::Rejected { .. }
            | SocketConnectionEvent::RequestParsed { .. } => {}
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
