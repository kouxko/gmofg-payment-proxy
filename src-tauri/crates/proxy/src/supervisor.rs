//! Transactional multi-listener Tokio supervisor (`STATE-001` through `STATE-009`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::FutureExt;
use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::message::MessageLimits;
use crate::transport::{
    BoundListener, ConnectionAdmission, ConnectionService, ListenerBinder, PipelinePorts,
};
use crate::{ErrorCode, ProxyError, Result};

pub const DEFAULT_MAX_CONNECTIONS: usize = 500;

#[cfg(not(test))]
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(5);
#[cfg(test)]
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_millis(100);

/// Stable, product-neutral identifier for one configured proxy channel.
///
/// IDs are intentionally safe for logs, configuration keys and command-line
/// arguments: 1-64 ASCII characters, beginning and ending with an
/// alphanumeric character, with `-`, `_` and `.` allowed internally.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChannelId(String);

impl ChannelId {
    pub const MAX_LEN: usize = 64;

    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        Self::validate(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(value: &str) -> Result<()> {
        let valid_length = !value.is_empty() && value.len() <= Self::MAX_LEN;
        let mut chars = value.chars();
        let first = chars.next();
        let last = value.chars().next_back();
        let valid_edges = first.is_some_and(|character| character.is_ascii_alphanumeric())
            && last.is_some_and(|character| character.is_ascii_alphanumeric());
        let valid_characters = value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if valid_length && valid_edges && valid_characters {
            return Ok(());
        }
        Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            format!(
                "invalid channel ID {value:?}; expected 1-{} ASCII letters, digits, '-', '_' or '.', with alphanumeric edges",
                Self::MAX_LEN
            ),
        ))
    }
}

impl AsRef<str> for ChannelId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ChannelId {
    type Err = ProxyError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl TryFrom<String> for ChannelId {
    type Error = ProxyError;

    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl TryFrom<&str> for ChannelId {
    type Error = ProxyError;

    fn try_from(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ChannelId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub channel: ChannelId,
    pub enabled: bool,
    pub listen_addr: SocketAddr,
    pub upstream_url: String,
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub channels: Vec<ChannelConfig>,
    pub limits: MessageLimits,
    pub max_connections: usize,
    pub connect_timeout: Duration,
    pub write_timeout: Duration,
    pub read_timeout: Duration,
    pub rewrite_host: bool,
    pub leaf_sans: Vec<String>,
}

impl ProxyConfig {
    pub fn validate(&self) -> Result<()> {
        let enabled: Vec<_> = self
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .collect();
        if enabled.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "at least one proxy channel must be enabled",
            ));
        }
        if enabled
            .iter()
            .any(|channel| channel.upstream_url.trim().is_empty())
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "enabled channels require an upstream URL",
            ));
        }
        if self.connect_timeout.is_zero()
            || self.write_timeout.is_zero()
            || self.read_timeout.is_zero()
            || self.limits.max_body_bytes == 0
            || self.max_connections == 0
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "timeouts, body limit, and connection limit must be greater than zero",
            ));
        }
        let mut channel_ids = BTreeSet::new();
        if self
            .channels
            .iter()
            .any(|channel| !channel_ids.insert(&channel.channel))
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "each channel may appear at most once",
            ));
        }
        let mut listen_addresses = BTreeSet::new();
        if enabled.iter().any(|channel| {
            channel.listen_addr.port() != 0 && !listen_addresses.insert(channel.listen_addr)
        }) {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "enabled channels cannot use the same listen address",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub state: ProxyState,
    pub runtime_epoch: Option<Uuid>,
    pub listeners: BTreeMap<ChannelId, SocketAddr>,
    pub fault: Option<String>,
}

#[derive(Debug)]
struct Runtime {
    epoch: Uuid,
    cancellation: CancellationToken,
    listener_tasks: Vec<JoinHandle<()>>,
    watchdog: JoinHandle<()>,
    ports: Arc<dyn PipelinePorts>,
    stopping_notified: Arc<StoppingNotification>,
}

#[derive(Debug)]
struct PendingCleanup {
    epoch: Uuid,
    ports: Arc<dyn PipelinePorts>,
    stopping_notified: Arc<StoppingNotification>,
}

#[derive(Debug, Default)]
struct StoppingNotification {
    completed: AtomicBool,
    operation: Mutex<()>,
}

impl StoppingNotification {
    fn is_complete(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
struct CancelOnDrop {
    token: CancellationToken,
    armed: bool,
}

impl CancelOnDrop {
    fn new(token: CancellationToken) -> Self {
        Self { token, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.token.cancel();
        }
    }
}

#[derive(Debug)]
struct Lifecycle {
    state: ProxyState,
    epoch: Option<Uuid>,
    listeners: BTreeMap<ChannelId, SocketAddr>,
    fault: Option<String>,
}

/// Start-time composition seam for upstream URLs, timeout policy and TLS
/// certificate snapshots. Implementations are called once per enabled channel
/// before the epoch becomes visible.
#[async_trait]
pub trait RuntimeServiceFactory: std::fmt::Debug + Send + Sync {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<ChannelId, ConnectionService>>;
}

#[derive(Debug, Clone)]
struct StaticRuntimeServiceFactory {
    service: ConnectionService,
}

#[async_trait]
impl RuntimeServiceFactory for StaticRuntimeServiceFactory {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<ChannelId, ConnectionService>> {
        let mut service = self.service.clone();
        service.limits = config.limits;
        service.write_timeout = config.write_timeout;
        service.read_timeout = config.read_timeout;
        service.admission = ConnectionAdmission::new(config.max_connections)?;
        Ok(config
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .map(|channel| (channel.channel.clone(), service.clone()))
            .collect())
    }
}

/// Owns all listener roots and guarantees all-or-nothing startup.
#[derive(Debug)]
pub struct ProxySupervisor {
    core: Arc<SupervisorCore>,
}

#[derive(Debug)]
struct SupervisorCore {
    binder: Arc<dyn ListenerBinder>,
    service_factory: Arc<dyn RuntimeServiceFactory>,
    operation: Mutex<()>,
    lifecycle: Arc<RwLock<Lifecycle>>,
    runtime: Mutex<Option<Runtime>>,
    pending_cleanup: Mutex<Option<PendingCleanup>>,
    active_cancellation: StdMutex<Option<CancellationToken>>,
}

impl ProxySupervisor {
    pub fn new(binder: Arc<dyn ListenerBinder>, service: ConnectionService) -> Self {
        Self::with_factory(binder, Arc::new(StaticRuntimeServiceFactory { service }))
    }

    /// Creates a supervisor whose complete channel transport is rebuilt at
    /// every start. The factory should load one immutable certificate/settings
    /// snapshot and construct the channel-specific connector from `config`.
    pub fn with_factory(
        binder: Arc<dyn ListenerBinder>,
        service_factory: Arc<dyn RuntimeServiceFactory>,
    ) -> Self {
        Self {
            core: Arc::new(SupervisorCore {
                binder,
                service_factory,
                operation: Mutex::new(()),
                lifecycle: Arc::new(RwLock::new(Lifecycle {
                    state: ProxyState::Stopped,
                    epoch: None,
                    listeners: BTreeMap::new(),
                    fault: None,
                })),
                runtime: Mutex::new(None),
                pending_cleanup: Mutex::new(None),
                active_cancellation: StdMutex::new(None),
            }),
        }
    }

    pub async fn snapshot(&self) -> RuntimeSnapshot {
        let lifecycle = self.core.lifecycle.read().await;
        snapshot(&lifecycle)
    }

    pub async fn start(&self, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        let core = Arc::clone(&self.core);
        tokio::spawn(async move { core.run_start(config).await })
            .await
            .map_err(|error| operation_join_error(&error))?
    }

    pub async fn stop(&self) -> Result<RuntimeSnapshot> {
        let core = Arc::clone(&self.core);
        tokio::spawn(async move { core.run_stop().await })
            .await
            .map_err(|error| operation_join_error(&error))?
    }

    pub async fn restart(&self, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        let core = Arc::clone(&self.core);
        tokio::spawn(async move { core.run_restart(config).await })
            .await
            .map_err(|error| operation_join_error(&error))?
    }
}

impl SupervisorCore {
    async fn run_start(self: Arc<Self>, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        self.run_guarded(self.start_inner(config)).await
    }

    async fn run_stop(self: Arc<Self>) -> Result<RuntimeSnapshot> {
        self.run_guarded(self.stop_inner()).await
    }

    async fn run_restart(self: Arc<Self>, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        self.run_guarded(async {
            self.stop_inner().await?;
            self.start_inner(config).await
        })
        .await
    }

    async fn run_guarded<F>(&self, operation: F) -> Result<RuntimeSnapshot>
    where
        F: std::future::Future<Output = Result<RuntimeSnapshot>>,
    {
        match AssertUnwindSafe(operation).catch_unwind().await {
            Ok(result) => result,
            Err(payload) => {
                let message = format!(
                    "proxy lifecycle operation panicked: {}",
                    panic_message(payload.as_ref())
                );
                let _ = self.cleanup_runtime().await;
                let mut lifecycle = self.lifecycle.write().await;
                lifecycle.state = ProxyState::Faulted;
                lifecycle.epoch = None;
                lifecycle.listeners.clear();
                lifecycle.fault = Some(message.clone());
                Err(ProxyError::new(ErrorCode::Internal, message))
            }
        }
    }

    async fn start_inner(&self, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        let _operation = self.operation.lock().await;
        config.validate()?;
        match self.lifecycle.read().await.state {
            ProxyState::Running => {
                return Err(ProxyError::new(
                    ErrorCode::ProxyAlreadyRunning,
                    "proxy is already running",
                ));
            }
            ProxyState::Starting
            | ProxyState::Stopping
            | ProxyState::Stopped
            | ProxyState::Faulted => {}
        }
        if let Err(error) = self.cleanup_runtime().await {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Faulted;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = Some(error.to_string());
            return Err(error);
        }
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Starting;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = None;
        }

        // Pre-bind every enabled listener before spawning any task. Dropping this
        // local vector rolls back all earlier binds if a later bind fails.
        let mut bound = Vec::<(ChannelId, Arc<dyn BoundListener>, SocketAddr)>::new();
        for channel in config.channels.iter().filter(|channel| channel.enabled) {
            let listener = match self.binder.bind(channel.listen_addr).await {
                Ok(listener) => listener,
                Err(error) => {
                    let code = if error.kind() == std::io::ErrorKind::AddrInUse {
                        ErrorCode::PortInUse
                    } else {
                        ErrorCode::Io
                    };
                    let proxy_error = ProxyError::new(
                        code,
                        format!("failed to bind {}: {error}", channel.listen_addr),
                    );
                    let mut lifecycle = self.lifecycle.write().await;
                    lifecycle.state = ProxyState::Faulted;
                    lifecycle.fault = Some(proxy_error.to_string());
                    return Err(proxy_error);
                }
            };
            let local_addr = match listener.local_addr() {
                Ok(address) => address,
                Err(error) => {
                    let proxy_error = ProxyError::io("read listener address", &error);
                    let mut lifecycle = self.lifecycle.write().await;
                    lifecycle.state = ProxyState::Faulted;
                    lifecycle.fault = Some(proxy_error.to_string());
                    return Err(proxy_error);
                }
            };
            bound.push((channel.channel.clone(), listener, local_addr));
        }

        // Build every channel service before publishing a new epoch. Any
        // certificate/upstream failure therefore rolls back all bound sockets.
        let mut services = match self.service_factory.build(&config).await {
            Ok(services) => services,
            Err(error) => {
                let mut lifecycle = self.lifecycle.write().await;
                lifecycle.state = ProxyState::Faulted;
                lifecycle.fault = Some(error.to_string());
                return Err(error);
            }
        };
        let mut prepared = Vec::with_capacity(bound.len());
        for (channel, listener, local_addr) in bound {
            let Some(service) = services.remove(&channel) else {
                let error = ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    format!("runtime factory omitted {channel} service"),
                );
                let mut lifecycle = self.lifecycle.write().await;
                lifecycle.state = ProxyState::Faulted;
                lifecycle.fault = Some(error.to_string());
                return Err(error);
            };
            prepared.push((channel, listener, local_addr, service));
        }

        let epoch = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        let mut start_guard = CancelOnDrop::new(cancellation.clone());
        *self
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancellation.clone());
        let (fatal_tx, mut fatal_rx) = mpsc::channel::<(ChannelId, ProxyError)>(prepared.len());
        let (listeners_ready_tx, listeners_ready_rx) = tokio::sync::watch::channel(false);
        let mut listener_tasks = Vec::with_capacity(prepared.len());
        let mut listener_addresses = BTreeMap::new();
        let fault_ports = Arc::clone(&prepared.first().expect("validated enabled channel").3.ports);
        let stopping_notified = Arc::new(StoppingNotification::default());
        for (channel, listener, local_addr, service) in prepared {
            listener_addresses.insert(channel.clone(), local_addr);
            let child_cancel = cancellation.child_token();
            let tx = fatal_tx.clone();
            let mut listeners_ready = listeners_ready_rx.clone();
            listener_tasks.push(tokio::spawn(async move {
                if !*listeners_ready.borrow() && listeners_ready.changed().await.is_err() {
                    return;
                }
                if child_cancel.is_cancelled() {
                    return;
                }
                let outcome = AssertUnwindSafe(service.run_listener(
                    listener,
                    channel.clone(),
                    epoch,
                    child_cancel.clone(),
                ))
                .catch_unwind()
                .await;
                if child_cancel.is_cancelled() {
                    return;
                }
                let error = match outcome {
                    Ok(Err(error)) => error,
                    Ok(Ok(())) => ProxyError::new(
                        ErrorCode::Internal,
                        format!("{channel} listener exited unexpectedly"),
                    ),
                    Err(payload) => ProxyError::new(
                        ErrorCode::Internal,
                        format!(
                            "{channel} listener panicked: {}",
                            panic_message(payload.as_ref())
                        ),
                    ),
                };
                if !child_cancel.is_cancelled() {
                    let _ = tx.send((channel, error)).await;
                }
            }));
        }
        drop(fatal_tx);

        let lifecycle = Arc::clone(&self.lifecycle);
        let watchdog_cancel = cancellation.clone();
        let watchdog_ports = Arc::clone(&fault_ports);
        let watchdog_stopping_notified = Arc::clone(&stopping_notified);
        let watchdog = tokio::spawn(async move {
            tokio::select! {
                () = watchdog_cancel.cancelled() => {}
                fault = fatal_rx.recv() => {
                    if let Some((channel, error)) = fault {
                        if let Err(stopping_error) = notify_runtime_stopping(
                            &watchdog_ports,
                            &watchdog_stopping_notified,
                            epoch,
                        ).await {
                            tracing::error!(
                                runtime_epoch = %epoch,
                                error = %stopping_error,
                                "runtime fault cleanup callback failed"
                            );
                        }
                        watchdog_cancel.cancel();
                        let mut lifecycle = lifecycle.write().await;
                        if lifecycle.epoch == Some(epoch) {
                            lifecycle.state = ProxyState::Faulted;
                            lifecycle.fault = Some(error.to_string());
                        }
                        drop(lifecycle);
                        notify_runtime_fault(&watchdog_ports, epoch, channel, &error).await;
                    }
                }
            }
        });

        {
            let mut runtime = self.runtime.lock().await;
            *runtime = Some(Runtime {
                epoch,
                cancellation,
                listener_tasks,
                watchdog,
                ports: fault_ports,
                stopping_notified,
            });
        }
        start_guard.disarm();
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Running;
            lifecycle.epoch = Some(epoch);
            lifecycle.listeners = listener_addresses;
        }
        let _ = listeners_ready_tx.send(true);
        Ok(self.snapshot_inner().await)
    }

    async fn stop_inner(&self) -> Result<RuntimeSnapshot> {
        let _operation = self.operation.lock().await;
        match self.lifecycle.read().await.state {
            ProxyState::Stopped => {
                return Ok(self.snapshot_inner().await);
            }
            ProxyState::Starting
            | ProxyState::Stopping
            | ProxyState::Running
            | ProxyState::Faulted => {}
        }
        self.lifecycle.write().await.state = ProxyState::Stopping;
        if let Err(error) = self.cleanup_runtime().await {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Faulted;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = Some(error.to_string());
            return Err(error);
        }
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Stopped;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = None;
        }
        Ok(self.snapshot_inner().await)
    }

    async fn snapshot_inner(&self) -> RuntimeSnapshot {
        let lifecycle = self.lifecycle.read().await;
        snapshot(&lifecycle)
    }

    async fn cleanup_runtime(&self) -> Result<()> {
        let pending = self.pending_cleanup.lock().await.take();
        if let Some(pending) = pending
            && let Err(error) =
                notify_runtime_stopping(&pending.ports, &pending.stopping_notified, pending.epoch)
                    .await
        {
            *self.pending_cleanup.lock().await = Some(pending);
            return Err(error);
        }

        let runtime = self.runtime.lock().await.take();
        let shutdown_result = if let Some(runtime) = runtime {
            let pending = PendingCleanup {
                epoch: runtime.epoch,
                ports: Arc::clone(&runtime.ports),
                stopping_notified: Arc::clone(&runtime.stopping_notified),
            };
            let result = shutdown_runtime(runtime).await;
            if result.is_err() && !pending.stopping_notified.is_complete() {
                *self.pending_cleanup.lock().await = Some(pending);
            }
            result
        } else {
            Ok(())
        };
        if let Some(cancellation) = self
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            cancellation.cancel();
        }
        shutdown_result
    }
}

impl Drop for ProxySupervisor {
    fn drop(&mut self) {
        if let Some(cancellation) = self
            .core
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            cancellation.cancel();
        }
    }
}

impl Drop for SupervisorCore {
    fn drop(&mut self) {
        if let Some(cancellation) = self
            .active_cancellation
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            cancellation.cancel();
        }
    }
}

async fn shutdown_runtime(mut runtime: Runtime) -> Result<()> {
    let mut cancellation_guard = CancelOnDrop::new(runtime.cancellation.clone());
    tracing::debug!(runtime_epoch = %runtime.epoch, "stopping proxy runtime");
    let stopping_result =
        notify_runtime_stopping(&runtime.ports, &runtime.stopping_notified, runtime.epoch).await;
    runtime.cancellation.cancel();
    cancellation_guard.disarm();

    let joined = tokio::time::timeout(SHUTDOWN_GRACE_PERIOD, async {
        for task in &mut runtime.listener_tasks {
            let _ = task.await;
        }
        let _ = (&mut runtime.watchdog).await;
    })
    .await;
    let join_result = if joined.is_err() {
        tracing::warn!(
            runtime_epoch = %runtime.epoch,
            "proxy runtime exceeded shutdown grace period; aborting remaining tasks"
        );
        for task in &runtime.listener_tasks {
            task.abort();
        }
        runtime.watchdog.abort();
        for task in runtime.listener_tasks {
            let _ = task.await;
        }
        let _ = runtime.watchdog.await;
        Err(ProxyError::new(
            ErrorCode::Internal,
            "proxy runtime exceeded shutdown grace period",
        ))
    } else {
        Ok(())
    };
    stopping_result.and(join_result)
}

async fn notify_runtime_stopping(
    ports: &Arc<dyn PipelinePorts>,
    stopping_notified: &StoppingNotification,
    epoch: Uuid,
) -> Result<()> {
    if stopping_notified.is_complete() {
        return Ok(());
    }
    let _operation = stopping_notified.operation.lock().await;
    if stopping_notified.is_complete() {
        return Ok(());
    }
    let outcome = tokio::time::timeout(
        SHUTDOWN_GRACE_PERIOD,
        AssertUnwindSafe(ports.runtime_stopping(epoch)).catch_unwind(),
    )
    .await;
    let result = match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(payload)) => Err(ProxyError::new(
            ErrorCode::Internal,
            format!(
                "runtime_stopping callback panicked: {}",
                panic_message(payload.as_ref())
            ),
        )),
        Err(_) => Err(ProxyError::new(
            ErrorCode::Internal,
            "runtime_stopping callback exceeded shutdown grace period",
        )),
    };
    if result.is_ok() {
        stopping_notified.completed.store(true, Ordering::Release);
    }
    result
}

async fn notify_runtime_fault(
    ports: &Arc<dyn PipelinePorts>,
    epoch: Uuid,
    channel: ChannelId,
    error: &ProxyError,
) {
    let outcome = tokio::time::timeout(
        SHUTDOWN_GRACE_PERIOD,
        AssertUnwindSafe(ports.runtime_fault(epoch, channel, error)).catch_unwind(),
    )
    .await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(payload)) => tracing::error!(
            runtime_epoch = %epoch,
            panic = %panic_message(payload.as_ref()),
            "runtime_fault callback panicked"
        ),
        Err(_) => tracing::warn!(
            runtime_epoch = %epoch,
            "runtime_fault callback exceeded shutdown grace period"
        ),
    }
}

fn operation_join_error(error: &tokio::task::JoinError) -> ProxyError {
    ProxyError::new(
        ErrorCode::Internal,
        format!("proxy lifecycle task failed: {error}"),
    )
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload")
}

fn snapshot(lifecycle: &Lifecycle) -> RuntimeSnapshot {
    RuntimeSnapshot {
        state: lifecycle.state,
        runtime_epoch: lifecycle.epoch,
        listeners: lifecycle.listeners.clone(),
        fault: lifecycle.fault.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::io;
    use std::sync::atomic::AtomicUsize;

    use crate::fault::FaultAction;
    use crate::transport::{
        AcceptedConnection, BoxIo, ConnectionAcceptor, ForwardRequest, HandshakePolicy,
        NoopPipelinePorts, SystemClock, TokioListenerBinder, UpstreamConnector,
    };

    use super::*;

    #[derive(Debug)]
    struct PlaintextAcceptor;

    #[async_trait]
    impl ConnectionAcceptor for PlaintextAcceptor {
        async fn accept(
            &self,
            io: BoxIo,
            _context: &crate::transport::ConnectionContext,
        ) -> Result<AcceptedConnection> {
            Ok(AcceptedConnection { io, tls_peer: None })
        }
    }

    #[derive(Debug)]
    struct UnusedUpstream;

    #[async_trait]
    impl UpstreamConnector for UnusedUpstream {
        async fn send(
            &self,
            _request: ForwardRequest,
            _actions: &[FaultAction],
            _informational: Option<&crate::transport::InformationalResponseSink>,
            _cancellation: &CancellationToken,
        ) -> Result<crate::transport::UpstreamExchange> {
            unreachable!("the synthetic listeners never accept a connection")
        }
    }

    fn test_service(ports: Arc<dyn PipelinePorts>) -> ConnectionService {
        ConnectionService {
            acceptor: Arc::new(PlaintextAcceptor),
            upstream: Arc::new(UnusedUpstream),
            ports,
            clock: Arc::new(SystemClock),
            admission: ConnectionAdmission::new(8).unwrap(),
            limits: MessageLimits::default(),
            read_timeout: Duration::from_secs(1),
            write_timeout: Duration::from_secs(1),
        }
    }

    fn channel_id(value: &str) -> ChannelId {
        ChannelId::new(value).expect("valid test channel ID")
    }

    fn test_config() -> ProxyConfig {
        ProxyConfig {
            channels: vec![
                ChannelConfig {
                    channel: channel_id("alpha"),
                    enabled: true,
                    listen_addr: "127.0.0.1:0".parse().unwrap(),
                    upstream_url: "http://alpha.test/".into(),
                },
                ChannelConfig {
                    channel: channel_id("beta"),
                    enabled: true,
                    listen_addr: "127.0.0.1:0".parse().unwrap(),
                    upstream_url: "http://beta.test/".into(),
                },
            ],
            limits: MessageLimits::default(),
            max_connections: 8,
            connect_timeout: Duration::from_secs(1),
            write_timeout: Duration::from_secs(1),
            read_timeout: Duration::from_secs(1),
            rewrite_host: true,
            leaf_sans: vec!["localhost".into()],
        }
    }

    #[test]
    fn channel_id_accepts_safe_product_neutral_values() {
        for value in ["alpha", "beta-v2", "gamma_3", "region.eu"] {
            assert_eq!(channel_id(value).as_str(), value);
        }
        for value in ["", "-alpha", "alpha-", "alpha/beta", "日本語"] {
            let error = ChannelId::new(value).expect_err("unsafe channel ID is rejected");
            assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
        }
    }

    #[test]
    fn channel_id_serde_round_trip_preserves_validation() {
        let original = channel_id("region.eu-2");
        let json = serde_json::to_string(&original).expect("serialize channel ID");
        assert_eq!(json, "\"region.eu-2\"");
        assert_eq!(
            serde_json::from_str::<ChannelId>(&json).expect("deserialize channel ID"),
            original
        );
        assert!(serde_json::from_str::<ChannelId>("\"alpha/beta\"").is_err());
    }

    #[test]
    fn config_rejects_duplicate_ids_and_nonzero_listen_addresses() {
        let mut duplicate_id = test_config();
        duplicate_id.channels.push(ChannelConfig {
            channel: channel_id("alpha"),
            enabled: false,
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            upstream_url: String::new(),
        });
        assert_eq!(
            duplicate_id.validate().unwrap_err().code,
            ErrorCode::ConfigInvalid.as_str()
        );

        let mut duplicate_address = test_config();
        duplicate_address.channels[0].listen_addr = "127.0.0.1:18080".parse().unwrap();
        duplicate_address.channels.push(ChannelConfig {
            channel: channel_id("gamma"),
            enabled: true,
            listen_addr: "127.0.0.1:18080".parse().unwrap(),
            upstream_url: "http://gamma.test/".into(),
        });
        assert_eq!(
            duplicate_address.validate().unwrap_err().code,
            ErrorCode::ConfigInvalid.as_str()
        );
    }

    #[tokio::test]
    async fn starts_listens_and_stops_three_channels() {
        let supervisor = ProxySupervisor::new(
            Arc::new(TokioListenerBinder),
            test_service(Arc::new(NoopPipelinePorts)),
        );
        let mut config = test_config();
        config.channels.push(ChannelConfig {
            channel: channel_id("gamma"),
            enabled: true,
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            upstream_url: "http://gamma.test/".into(),
        });

        let started = supervisor
            .start(config)
            .await
            .expect("start three channels");
        assert_eq!(started.state, ProxyState::Running);
        assert_eq!(started.listeners.len(), 3);
        for expected in ["alpha", "beta", "gamma"] {
            let address = started
                .listeners
                .get(&channel_id(expected))
                .expect("listener address exists");
            assert_ne!(address.port(), 0);
            tokio::net::TcpStream::connect(address)
                .await
                .expect("configured listener accepts connections");
        }

        let stopped = supervisor.stop().await.expect("stop three channels");
        assert_eq!(stopped.state, ProxyState::Stopped);
        assert!(stopped.listeners.is_empty());
    }

    #[derive(Debug)]
    struct BlockingFailFactory {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl RuntimeServiceFactory for BlockingFailFactory {
        async fn build(
            &self,
            _config: &ProxyConfig,
        ) -> Result<BTreeMap<ChannelId, ConnectionService>> {
            self.entered.notify_one();
            self.release.notified().await;
            Err(ProxyError::new(
                ErrorCode::CertificateNotReady,
                "injected startup failure",
            ))
        }
    }

    #[tokio::test]
    async fn aborted_start_waiter_does_not_strand_starting_state() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let supervisor = Arc::new(ProxySupervisor::with_factory(
            Arc::new(TokioListenerBinder),
            Arc::new(BlockingFailFactory {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
            }),
        ));
        let waiter = {
            let supervisor = Arc::clone(&supervisor);
            tokio::spawn(async move { supervisor.start(test_config()).await })
        };
        entered.notified().await;
        waiter.abort();
        release.notify_one();

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if supervisor.snapshot().await.state == ProxyState::Faulted {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached start operation reaches a stable terminal state");
    }

    #[derive(Debug)]
    struct BlockingStoppingPorts {
        entered: tokio::sync::Notify,
        release: tokio::sync::Notify,
    }

    #[derive(Debug, Clone, Copy)]
    enum StoppingFailure {
        Panic,
        Timeout,
    }

    #[derive(Debug)]
    struct FailingStoppingPorts(StoppingFailure);

    #[derive(Debug)]
    struct RetryingStoppingPorts {
        attempts: AtomicUsize,
        failures_before_success: usize,
        failure: StoppingFailure,
    }

    impl HandshakePolicy for FailingStoppingPorts {}

    #[async_trait]
    impl PipelinePorts for FailingStoppingPorts {
        async fn runtime_stopping(&self, _epoch: Uuid) {
            match self.0 {
                StoppingFailure::Panic => panic!("injected runtime_stopping panic"),
                StoppingFailure::Timeout => pending::<()>().await,
            }
        }
    }

    impl HandshakePolicy for RetryingStoppingPorts {}

    #[async_trait]
    impl PipelinePorts for RetryingStoppingPorts {
        async fn runtime_stopping(&self, _epoch: Uuid) {
            let attempt = self.attempts.fetch_add(1, Ordering::AcqRel);
            if attempt < self.failures_before_success {
                match self.failure {
                    StoppingFailure::Panic => panic!("injected retryable runtime_stopping panic"),
                    StoppingFailure::Timeout => pending::<()>().await,
                }
            }
        }
    }

    impl HandshakePolicy for BlockingStoppingPorts {}

    #[async_trait]
    impl PipelinePorts for BlockingStoppingPorts {
        async fn runtime_stopping(&self, _epoch: Uuid) {
            self.entered.notify_one();
            self.release.notified().await;
        }
    }

    #[tokio::test]
    async fn aborted_stop_waiter_does_not_strand_stopping_state() {
        let ports = Arc::new(BlockingStoppingPorts {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let supervisor = Arc::new(ProxySupervisor::new(
            Arc::new(TokioListenerBinder),
            test_service(ports.clone()),
        ));
        let epoch = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        let listener_cancel = cancellation.clone();
        let watchdog_cancel = cancellation.clone();
        *supervisor.core.runtime.lock().await = Some(Runtime {
            epoch,
            cancellation: cancellation.clone(),
            listener_tasks: vec![tokio::spawn(async move {
                listener_cancel.cancelled().await;
            })],
            watchdog: tokio::spawn(async move {
                watchdog_cancel.cancelled().await;
            }),
            ports: ports.clone(),
            stopping_notified: Arc::new(StoppingNotification::default()),
        });
        *supervisor
            .core
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancellation);
        {
            let mut lifecycle = supervisor.core.lifecycle.write().await;
            lifecycle.state = ProxyState::Running;
            lifecycle.epoch = Some(epoch);
        }

        let waiter = {
            let supervisor = Arc::clone(&supervisor);
            tokio::spawn(async move { supervisor.stop().await })
        };
        ports.entered.notified().await;
        waiter.abort();
        ports.release.notify_one();

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if supervisor.snapshot().await.state == ProxyState::Stopped {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached stop operation reaches Stopped");
    }

    #[tokio::test]
    async fn stopping_callback_failure_is_reported_and_keeps_faulted_state() {
        for failure in [StoppingFailure::Panic, StoppingFailure::Timeout] {
            let ports = Arc::new(FailingStoppingPorts(failure));
            let supervisor =
                ProxySupervisor::new(Arc::new(TokioListenerBinder), test_service(ports.clone()));
            let epoch = Uuid::new_v4();
            let cancellation = CancellationToken::new();
            let listener_cancel = cancellation.clone();
            let watchdog_cancel = cancellation.clone();
            *supervisor.core.runtime.lock().await = Some(Runtime {
                epoch,
                cancellation: cancellation.clone(),
                listener_tasks: vec![tokio::spawn(async move {
                    listener_cancel.cancelled().await;
                })],
                watchdog: tokio::spawn(async move {
                    watchdog_cancel.cancelled().await;
                }),
                ports,
                stopping_notified: Arc::new(StoppingNotification::default()),
            });
            *supervisor
                .core
                .active_cancellation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancellation);
            {
                let mut lifecycle = supervisor.core.lifecycle.write().await;
                lifecycle.state = ProxyState::Running;
                lifecycle.epoch = Some(epoch);
            }

            let error = supervisor
                .stop()
                .await
                .expect_err("failed cleanup callback must fail stop");
            assert_eq!(error.code, ErrorCode::Internal.as_str());
            let snapshot = supervisor.snapshot().await;
            assert_eq!(snapshot.state, ProxyState::Faulted);
            assert!(snapshot.runtime_epoch.is_none());
            assert!(
                snapshot
                    .fault
                    .as_deref()
                    .is_some_and(|fault| fault.contains("runtime_stopping callback"))
            );
        }
    }

    async fn install_synthetic_runtime(
        supervisor: &ProxySupervisor,
        ports: Arc<dyn PipelinePorts>,
    ) -> Uuid {
        let epoch = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        let listener_cancel = cancellation.clone();
        let watchdog_cancel = cancellation.clone();
        *supervisor.core.runtime.lock().await = Some(Runtime {
            epoch,
            cancellation: cancellation.clone(),
            listener_tasks: vec![tokio::spawn(async move {
                listener_cancel.cancelled().await;
            })],
            watchdog: tokio::spawn(async move {
                watchdog_cancel.cancelled().await;
            }),
            ports,
            stopping_notified: Arc::new(StoppingNotification::default()),
        });
        *supervisor
            .core
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancellation);
        {
            let mut lifecycle = supervisor.core.lifecycle.write().await;
            lifecycle.state = ProxyState::Running;
            lifecycle.epoch = Some(epoch);
        }
        epoch
    }

    #[tokio::test]
    async fn failed_stopping_callback_is_retried_until_later_stop_succeeds() {
        for failure in [StoppingFailure::Panic, StoppingFailure::Timeout] {
            let ports = Arc::new(RetryingStoppingPorts {
                attempts: AtomicUsize::new(0),
                failures_before_success: 2,
                failure,
            });
            let supervisor =
                ProxySupervisor::new(Arc::new(TokioListenerBinder), test_service(ports.clone()));
            install_synthetic_runtime(&supervisor, ports.clone()).await;

            for expected_attempts in [1, 2] {
                supervisor
                    .stop()
                    .await
                    .expect_err("pending cleanup must keep stop faulted until callback succeeds");
                assert_eq!(
                    supervisor.snapshot().await.state,
                    ProxyState::Faulted,
                    "failed retry must not publish Stopped"
                );
                assert_eq!(
                    ports.attempts.load(Ordering::Acquire),
                    expected_attempts,
                    "each stop retries the pending cleanup exactly once"
                );
            }

            let stopped = supervisor
                .stop()
                .await
                .expect("third cleanup callback attempt succeeds");
            assert_eq!(stopped.state, ProxyState::Stopped);
            assert_eq!(ports.attempts.load(Ordering::Acquire), 3);
        }
    }

    #[derive(Debug)]
    struct CountingBinder {
        binds: AtomicUsize,
    }

    #[async_trait]
    impl ListenerBinder for CountingBinder {
        async fn bind(&self, address: SocketAddr) -> io::Result<Arc<dyn BoundListener>> {
            self.binds.fetch_add(1, Ordering::AcqRel);
            TokioListenerBinder.bind(address).await
        }
    }

    #[tokio::test]
    async fn failed_stopping_callback_blocks_start_until_cleanup_retry_succeeds() {
        let ports = Arc::new(RetryingStoppingPorts {
            attempts: AtomicUsize::new(0),
            failures_before_success: 2,
            failure: StoppingFailure::Panic,
        });
        let binder = Arc::new(CountingBinder {
            binds: AtomicUsize::new(0),
        });
        let supervisor = ProxySupervisor::new(binder.clone(), test_service(ports.clone()));
        let old_epoch = install_synthetic_runtime(&supervisor, ports.clone()).await;

        supervisor
            .stop()
            .await
            .expect_err("initial cleanup callback fails");
        supervisor
            .start(test_config())
            .await
            .expect_err("start must retry and remain blocked by pending cleanup");
        let blocked = supervisor.snapshot().await;
        assert_eq!(blocked.state, ProxyState::Faulted);
        assert!(blocked.runtime_epoch.is_none());
        assert_eq!(
            binder.binds.load(Ordering::Acquire),
            0,
            "start cannot bind a new epoch before pending cleanup succeeds"
        );
        assert_eq!(ports.attempts.load(Ordering::Acquire), 2);

        let running = supervisor
            .start(test_config())
            .await
            .expect("start may proceed only after cleanup retry succeeds");
        assert_eq!(running.state, ProxyState::Running);
        assert_ne!(running.runtime_epoch, Some(old_epoch));
        assert_eq!(binder.binds.load(Ordering::Acquire), 2);
        assert_eq!(ports.attempts.load(Ordering::Acquire), 3);
        supervisor.stop().await.unwrap();
    }

    #[derive(Debug)]
    struct PendingListener {
        address: SocketAddr,
        accept_dropped: Arc<AtomicBool>,
    }

    struct AcceptGuard(Arc<AtomicBool>);

    impl Drop for AcceptGuard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[async_trait]
    impl BoundListener for PendingListener {
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok(self.address)
        }

        async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)> {
            let _guard = AcceptGuard(Arc::clone(&self.accept_dropped));
            pending().await
        }
    }

    #[derive(Debug)]
    struct PanicListener(SocketAddr);

    #[async_trait]
    impl BoundListener for PanicListener {
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok(self.0)
        }

        async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)> {
            panic!("injected listener panic");
        }
    }

    #[derive(Debug)]
    struct PanicBinder {
        binds: AtomicUsize,
        sibling_accept_dropped: Arc<AtomicBool>,
    }

    #[async_trait]
    impl ListenerBinder for PanicBinder {
        async fn bind(&self, _address: SocketAddr) -> io::Result<Arc<dyn BoundListener>> {
            let index = self.binds.fetch_add(1, Ordering::Relaxed);
            if index == 0 {
                Ok(Arc::new(PendingListener {
                    address: "127.0.0.1:31001".parse().unwrap(),
                    accept_dropped: Arc::clone(&self.sibling_accept_dropped),
                }))
            } else {
                Ok(Arc::new(PanicListener("127.0.0.1:31002".parse().unwrap())))
            }
        }
    }

    #[tokio::test]
    async fn listener_panic_faults_epoch_and_cancels_sibling() {
        let sibling_accept_dropped = Arc::new(AtomicBool::new(false));
        let supervisor = ProxySupervisor::new(
            Arc::new(PanicBinder {
                binds: AtomicUsize::new(0),
                sibling_accept_dropped: Arc::clone(&sibling_accept_dropped),
            }),
            test_service(Arc::new(NoopPipelinePorts)),
        );
        assert_eq!(
            supervisor.start(test_config()).await.unwrap().state,
            ProxyState::Running
        );

        let faulted = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = supervisor.snapshot().await;
                if snapshot.state == ProxyState::Faulted {
                    break snapshot;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("watchdog observes listener panic");
        assert!(
            faulted
                .fault
                .as_deref()
                .is_some_and(|fault| fault.contains("listener panicked"))
        );
        assert!(
            sibling_accept_dropped.load(Ordering::Acquire),
            "fault cancellation drops the sibling accept future"
        );
        supervisor.stop().await.unwrap();
    }

    struct TaskDropGuard(Arc<AtomicBool>);

    impl Drop for TaskDropGuard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn shutdown_aborts_tasks_that_ignore_cancellation_after_grace_period() {
        let listener_dropped = Arc::new(AtomicBool::new(false));
        let watchdog_dropped = Arc::new(AtomicBool::new(false));
        let listener_started = Arc::new(tokio::sync::Notify::new());
        let watchdog_started = Arc::new(tokio::sync::Notify::new());
        let listener_task = {
            let dropped = Arc::clone(&listener_dropped);
            let started = Arc::clone(&listener_started);
            tokio::spawn(async move {
                let _guard = TaskDropGuard(dropped);
                started.notify_one();
                pending::<()>().await;
            })
        };
        let watchdog = {
            let dropped = Arc::clone(&watchdog_dropped);
            let started = Arc::clone(&watchdog_started);
            tokio::spawn(async move {
                let _guard = TaskDropGuard(dropped);
                started.notify_one();
                pending::<()>().await;
            })
        };
        listener_started.notified().await;
        watchdog_started.notified().await;

        let cancellation = CancellationToken::new();
        let error = shutdown_runtime(Runtime {
            epoch: Uuid::new_v4(),
            cancellation,
            listener_tasks: vec![listener_task],
            watchdog,
            ports: Arc::new(NoopPipelinePorts),
            stopping_notified: Arc::new(StoppingNotification::default()),
        })
        .await
        .expect_err("forced abort is a shutdown failure");

        assert_eq!(error.code, ErrorCode::Internal.as_str());
        assert!(listener_dropped.load(Ordering::Acquire));
        assert!(watchdog_dropped.load(Ordering::Acquire));
    }
}
