//! Transactional two-listener Tokio supervisor (`STATE-001` through `STATE-009`).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Transaction,
    Dll,
}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub channel: Channel,
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
        let transaction_count = self
            .channels
            .iter()
            .filter(|channel| channel.channel == Channel::Transaction)
            .count();
        let dll_count = self
            .channels
            .iter()
            .filter(|channel| channel.channel == Channel::Dll)
            .count();
        if transaction_count > 1 || dll_count > 1 {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "each channel may appear at most once",
            ));
        }
        if enabled.len() == 2
            && enabled[0].listen_addr.port() != 0
            && enabled[0].listen_addr == enabled[1].listen_addr
        {
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
    pub listeners: BTreeMap<Channel, SocketAddr>,
    pub fault: Option<String>,
}

#[derive(Debug)]
struct Runtime {
    epoch: Uuid,
    cancellation: CancellationToken,
    listener_tasks: Vec<JoinHandle<()>>,
    watchdog: JoinHandle<()>,
    ports: Arc<dyn PipelinePorts>,
    stopping_notified: Arc<AtomicBool>,
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
    listeners: BTreeMap<Channel, SocketAddr>,
    fault: Option<String>,
}

/// Start-time composition seam for upstream URLs, timeout policy and TLS
/// certificate snapshots. Implementations are called once per enabled channel
/// before the epoch becomes visible.
#[async_trait]
pub trait RuntimeServiceFactory: std::fmt::Debug + Send + Sync {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<Channel, ConnectionService>>;
}

#[derive(Debug, Clone)]
struct StaticRuntimeServiceFactory {
    service: ConnectionService,
}

#[async_trait]
impl RuntimeServiceFactory for StaticRuntimeServiceFactory {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<Channel, ConnectionService>> {
        let mut service = self.service.clone();
        service.limits = config.limits;
        service.read_timeout = config.read_timeout;
        service.admission = ConnectionAdmission::new(config.max_connections)?;
        Ok(config
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .map(|channel| (channel.channel, service.clone()))
            .collect())
    }
}

/// Owns all listener roots and guarantees all-or-nothing startup.
#[derive(Debug)]
pub struct ProxySupervisor {
    binder: Arc<dyn ListenerBinder>,
    service_factory: Arc<dyn RuntimeServiceFactory>,
    operation: Mutex<()>,
    lifecycle: Arc<RwLock<Lifecycle>>,
    runtime: Mutex<Option<Runtime>>,
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
            active_cancellation: StdMutex::new(None),
        }
    }

    pub async fn snapshot(&self) -> RuntimeSnapshot {
        let lifecycle = self.lifecycle.read().await;
        snapshot(&lifecycle)
    }

    pub async fn start(&self, config: ProxyConfig) -> Result<RuntimeSnapshot> {
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
        self.cleanup_runtime().await;
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Starting;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = None;
        }

        // Pre-bind every enabled listener before spawning any task. Dropping this
        // local vector rolls back all earlier binds if a later bind fails.
        let mut bound = Vec::<(Channel, Arc<dyn BoundListener>, SocketAddr)>::new();
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
            bound.push((channel.channel, listener, local_addr));
        }

        // Build both channel services before publishing a new epoch. Any
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
                    format!("runtime factory omitted {channel:?} service"),
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
        let (fatal_tx, mut fatal_rx) = mpsc::channel::<(Channel, ProxyError)>(prepared.len());
        let mut listener_tasks = Vec::with_capacity(prepared.len());
        let mut listener_addresses = BTreeMap::new();
        let fault_ports = Arc::clone(&prepared.first().expect("validated enabled channel").3.ports);
        let stopping_notified = Arc::new(AtomicBool::new(false));
        for (channel, listener, local_addr, service) in prepared {
            listener_addresses.insert(channel, local_addr);
            let child_cancel = cancellation.child_token();
            let tx = fatal_tx.clone();
            listener_tasks.push(tokio::spawn(async move {
                if let Err(error) = service
                    .run_listener(listener, channel, epoch, child_cancel.clone())
                    .await
                    && !child_cancel.is_cancelled()
                {
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
                        if !watchdog_stopping_notified.swap(true, Ordering::AcqRel) {
                            watchdog_ports.runtime_stopping(epoch).await;
                        }
                        watchdog_cancel.cancel();
                        watchdog_ports.runtime_fault(epoch, channel, &error).await;
                        let mut lifecycle = lifecycle.write().await;
                        if lifecycle.epoch == Some(epoch) {
                            lifecycle.state = ProxyState::Faulted;
                            lifecycle.fault = Some(error.to_string());
                        }
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
        Ok(self.snapshot().await)
    }

    pub async fn stop(&self) -> Result<RuntimeSnapshot> {
        let _operation = self.operation.lock().await;
        match self.lifecycle.read().await.state {
            ProxyState::Stopped => {
                return Ok(self.snapshot().await);
            }
            ProxyState::Starting
            | ProxyState::Stopping
            | ProxyState::Running
            | ProxyState::Faulted => {}
        }
        self.lifecycle.write().await.state = ProxyState::Stopping;
        self.cleanup_runtime().await;
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Stopped;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = None;
        }
        Ok(self.snapshot().await)
    }

    pub async fn restart(&self, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        self.stop().await?;
        self.start(config).await
    }

    async fn cleanup_runtime(&self) {
        let runtime = self.runtime.lock().await.take();
        if let Some(runtime) = runtime {
            let mut cancellation_guard = CancelOnDrop::new(runtime.cancellation.clone());
            tracing::debug!(runtime_epoch = %runtime.epoch, "stopping proxy runtime");
            if !runtime.stopping_notified.swap(true, Ordering::AcqRel) {
                runtime.ports.runtime_stopping(runtime.epoch).await;
            }
            runtime.cancellation.cancel();
            cancellation_guard.disarm();
            for task in runtime.listener_tasks {
                let _ = task.await;
            }
            let _ = runtime.watchdog.await;
        }
        if let Some(cancellation) = self
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            cancellation.cancel();
        }
    }
}

impl Drop for ProxySupervisor {
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

fn snapshot(lifecycle: &Lifecycle) -> RuntimeSnapshot {
    RuntimeSnapshot {
        state: lifecycle.state,
        runtime_epoch: lifecycle.epoch,
        listeners: lifecycle.listeners.clone(),
        fault: lifecycle.fault.clone(),
    }
}
