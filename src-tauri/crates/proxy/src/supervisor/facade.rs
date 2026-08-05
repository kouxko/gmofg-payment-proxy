use super::{
    Arc, BTreeMap, CancellationToken, ConnectionService, Lifecycle, ListenerBinder, Mutex,
    PendingCleanup, ProxyConfig, ProxyState, Result, Runtime, RuntimeServiceFactory,
    RuntimeSnapshot, RwLock, StaticRuntimeServiceFactory, StdMutex, operation_join_error, snapshot,
};

/// Owns all listener roots and guarantees all-or-nothing startup.
#[derive(Debug)]
pub struct ProxySupervisor {
    pub(super) core: Arc<SupervisorCore>,
}

#[derive(Debug)]
pub(super) struct SupervisorCore {
    pub(super) binder: Arc<dyn ListenerBinder>,
    pub(super) service_factory: Arc<dyn RuntimeServiceFactory>,
    pub(super) operation: Mutex<()>,
    pub(super) lifecycle: Arc<RwLock<Lifecycle>>,
    pub(super) runtime: Mutex<Option<Runtime>>,
    pub(super) pending_cleanup: Mutex<Option<PendingCleanup>>,
    pub(super) active_cancellation: StdMutex<Option<CancellationToken>>,
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
