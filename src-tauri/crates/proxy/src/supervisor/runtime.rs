use super::{
    Arc, AtomicBool, BTreeMap, BoundListener, CancellationToken, ChannelId, ConnectionService,
    JoinHandle, Mutex, Ordering, PipelinePorts, ProxyState, SocketAddr, Uuid,
};

#[derive(Debug)]
pub(super) struct Runtime {
    pub(super) epoch: Uuid,
    pub(super) cancellation: CancellationToken,
    pub(super) listener_tasks: Vec<JoinHandle<()>>,
    pub(super) watchdog: JoinHandle<()>,
    pub(super) ports: Arc<dyn PipelinePorts>,
    pub(super) stopping_notified: Arc<StoppingNotification>,
}

pub(super) struct BoundChannel {
    pub(super) channel: ChannelId,
    pub(super) listener: Arc<dyn BoundListener>,
    pub(super) local_addr: SocketAddr,
}

pub(super) struct PreparedChannel {
    pub(super) channel: ChannelId,
    pub(super) listener: Arc<dyn BoundListener>,
    pub(super) local_addr: SocketAddr,
    pub(super) service: ConnectionService,
}

pub(super) struct StartedTasks {
    pub(super) listener_tasks: Vec<JoinHandle<()>>,
    pub(super) watchdog: JoinHandle<()>,
    pub(super) listener_addresses: BTreeMap<ChannelId, SocketAddr>,
    pub(super) ports: Arc<dyn PipelinePorts>,
    pub(super) stopping_notified: Arc<StoppingNotification>,
    pub(super) listeners_ready: tokio::sync::watch::Sender<bool>,
}

#[derive(Debug)]
pub(super) struct PendingCleanup {
    pub(super) epoch: Uuid,
    pub(super) ports: Arc<dyn PipelinePorts>,
    pub(super) stopping_notified: Arc<StoppingNotification>,
}

#[derive(Debug, Default)]
pub(super) struct StoppingNotification {
    pub(super) completed: AtomicBool,
    pub(super) operation: Mutex<()>,
}

impl StoppingNotification {
    pub(super) fn is_complete(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub(super) struct CancelOnDrop {
    token: CancellationToken,
    armed: bool,
}

impl CancelOnDrop {
    pub(super) fn new(token: CancellationToken) -> Self {
        Self { token, armed: true }
    }

    pub(super) fn disarm(&mut self) {
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
pub(super) struct Lifecycle {
    pub(super) state: ProxyState,
    pub(super) epoch: Option<Uuid>,
    pub(super) listeners: BTreeMap<ChannelId, SocketAddr>,
    pub(super) fault: Option<String>,
}
