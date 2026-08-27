use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use tokio::{
    io::duplex,
    sync::{Mutex as AsyncMutex, Notify, mpsc},
};

use crate::{
    ErrorCode, ProxyError,
    listener::{
        CONNECTION_CHILD_TASK_PANICKED, ChildTaskError, LISTENER_SHUTDOWN_GRACE_EXCEEDED, sealed,
    },
    transport::{BoxIo, SystemClock, TokioBoundListener, TokioListenerBinder},
};

use super::*;

mod shutdown;

#[derive(Debug)]
struct FakeListener {
    local_addr: SocketAddr,
    receiver: AsyncMutex<mpsc::UnboundedReceiver<(BoxIo, SocketAddr)>>,
}

#[async_trait]
impl BoundListener for FakeListener {
    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok(self.local_addr)
    }

    async fn accept(&self) -> std::io::Result<(BoxIo, SocketAddr)> {
        self.receiver.lock().await.recv().await.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "test listener closed")
        })
    }
}

fn fake_listener() -> (
    Arc<dyn BoundListener>,
    mpsc::UnboundedSender<(BoxIo, SocketAddr)>,
) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (
        Arc::new(FakeListener {
            local_addr: "127.0.0.1:32000".parse().unwrap(),
            receiver: AsyncMutex::new(receiver),
        }),
        sender,
    )
}

fn send_connection(sender: &mpsc::UnboundedSender<(BoxIo, SocketAddr)>, peer: SocketAddr) {
    let (server, client) = duplex(64);
    sender.send((Box::new(server), peer)).unwrap();
    drop(client);
}

#[derive(Debug, Default)]
struct RecordingObserver {
    rejected: Mutex<Vec<(SocketAddr, ListenerRejection)>>,
    admitted: Mutex<Vec<ConnectionContext>>,
    terminal: Mutex<Vec<(ConnectionContext, TerminalConnectionOutcome)>>,
    changed: Notify,
}

impl RecordingObserver {
    async fn wait_until(&self, predicate: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let changed = self.changed.notified();
                if predicate() {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("observer condition became true");
    }
}

impl ConnectionLifecycleObserver for RecordingObserver {
    fn connection_rejected(&self, peer_addr: SocketAddr, reason: ListenerRejection) {
        lock(&self.rejected).push((peer_addr, reason));
        self.changed.notify_waiters();
    }

    fn connection_admitted(&self, context: &ConnectionContext) {
        lock(&self.admitted).push(context.clone());
        self.changed.notify_waiters();
    }

    fn connection_terminal(
        &self,
        context: &ConnectionContext,
        outcome: &TerminalConnectionOutcome,
    ) {
        lock(&self.terminal).push((context.clone(), outcome.clone()));
        self.changed.notify_waiters();
    }
}

#[derive(Debug)]
enum HandlerMode {
    Immediate,
    Block {
        entered: Arc<Notify>,
        calls: Arc<AtomicUsize>,
        cancelled: Arc<Notify>,
        finished: Arc<AtomicUsize>,
    },
    ErrorThenSuccess(AtomicUsize),
    PanicChildAfterSibling {
        calls: AtomicUsize,
        sibling_entered: Arc<Notify>,
        sibling_cancelled: Arc<Notify>,
    },
    PendingChild,
    PanicChildPendingPrimary,
    CleanupOnCancellation {
        entered: Arc<Notify>,
        cleaned: Arc<Notify>,
    },
    PendingPrimary,
}

#[derive(Debug)]
struct FakeHandler(HandlerMode);

struct NotifyOnDrop(Arc<Notify>);

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

impl sealed::Sealed for FakeHandler {}

#[async_trait]
impl ConnectionHandler for FakeHandler {
    async fn handle(
        &self,
        _io: BoxIo,
        _context: ConnectionContext,
        child_tasks: ConnectionTaskScope,
        cancellation: CancellationToken,
    ) -> PrimaryConnectionOutcome {
        match &self.0 {
            HandlerMode::Immediate => PrimaryConnectionOutcome::Success,
            HandlerMode::Block {
                entered,
                calls,
                cancelled,
                finished,
            } => {
                let _cancelled = NotifyOnDrop(Arc::clone(cancelled));
                calls.fetch_add(1, Ordering::SeqCst);
                entered.notify_waiters();
                cancellation.cancelled().await;
                finished.fetch_add(1, Ordering::SeqCst);
                PrimaryConnectionOutcome::Cancelled
            }
            HandlerMode::ErrorThenSuccess(calls) => {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    PrimaryConnectionOutcome::Failed(ProxyError::new(
                        ErrorCode::Io,
                        "ordinary connection failure",
                    ))
                } else {
                    PrimaryConnectionOutcome::Success
                }
            }
            HandlerMode::PanicChildAfterSibling {
                calls,
                sibling_entered,
                sibling_cancelled,
            } => {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    let _cancelled = NotifyOnDrop(Arc::clone(sibling_cancelled));
                    sibling_entered.notify_one();
                    cancellation.cancelled().await;
                    PrimaryConnectionOutcome::Cancelled
                } else {
                    child_tasks
                        .spawn_owned(async move {
                            panic!("owned child panic");
                            #[allow(unreachable_code)]
                            Ok::<(), ChildTaskError>(())
                        })
                        .unwrap();
                    PrimaryConnectionOutcome::Success
                }
            }
            HandlerMode::PendingChild => {
                child_tasks
                    .spawn_owned(std::future::pending::<
                        std::result::Result<(), ChildTaskError>,
                    >())
                    .unwrap();
                PrimaryConnectionOutcome::Success
            }
            HandlerMode::PanicChildPendingPrimary => {
                child_tasks
                    .spawn_owned(async move {
                        panic!("owned child panic while primary remains pending");
                        #[allow(unreachable_code)]
                        Ok::<(), ChildTaskError>(())
                    })
                    .unwrap();
                std::future::pending().await
            }
            HandlerMode::CleanupOnCancellation { entered, cleaned } => {
                entered.notify_one();
                cancellation.cancelled().await;
                cleaned.notify_one();
                PrimaryConnectionOutcome::Cancelled
            }
            HandlerMode::PendingPrimary => std::future::pending().await,
        }
    }
}

fn supervisor(
    handler: Arc<FakeHandler>,
    observer: Arc<RecordingObserver>,
    capacity: usize,
    grace: Duration,
) -> ListenerSupervisor<FakeHandler> {
    ListenerSupervisor::new(
        ListenerConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            runtime_epoch: uuid::Uuid::new_v4(),
            listener_id: ChannelId::new("listener-contract").unwrap(),
            capacity: ListenerCapacity::new(capacity).unwrap(),
            shutdown_grace: grace,
        },
        Arc::new(TokioListenerBinder),
        Arc::new(SystemClock),
        handler,
        observer,
    )
    .unwrap()
}

#[tokio::test]
async fn capacity_is_per_listener_and_rejections_do_not_emit_admission() {
    let entered = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(Notify::new());
    let finished = Arc::new(AtomicUsize::new(0));
    let observer = Arc::new(RecordingObserver::default());
    let supervisor = Arc::new(supervisor(
        Arc::new(FakeHandler(HandlerMode::Block {
            entered: Arc::clone(&entered),
            calls: Arc::clone(&calls),
            cancelled,
            finished: Arc::clone(&finished),
        })),
        Arc::clone(&observer),
        1,
        Duration::from_millis(50),
    ));
    let (listener, sender) = fake_listener();
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let cancellation = cancellation.clone();
        async move { supervisor.run_bound(listener, cancellation).await }
    });
    send_connection(&sender, "10.1.1.1:1001".parse().unwrap());
    entered.notified().await;
    send_connection(&sender, "10.1.1.2:1002".parse().unwrap());
    observer
        .wait_until(|| lock(&observer.rejected).len() == 1)
        .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(lock(&observer.admitted).len(), 1);
    assert_eq!(
        lock(&observer.rejected).as_slice(),
        &[(
            "10.1.1.2:1002".parse().unwrap(),
            ListenerRejection::CapacityExhausted,
        )]
    );
    assert!(lock(&observer.terminal).is_empty());
    cancellation.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        ListenerRunOutcome::Stopped { .. }
    ));
    assert_eq!(finished.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ordinary_connection_error_is_isolated_and_ids_reach_one_terminal_event() {
    let observer = Arc::new(RecordingObserver::default());
    let supervisor = Arc::new(supervisor(
        Arc::new(FakeHandler(HandlerMode::ErrorThenSuccess(
            AtomicUsize::new(0),
        ))),
        Arc::clone(&observer),
        2,
        Duration::from_millis(50),
    ));
    let (listener, sender) = fake_listener();
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let cancellation = cancellation.clone();
        async move { supervisor.run_bound(listener, cancellation).await }
    });
    send_connection(&sender, "10.1.1.1:1001".parse().unwrap());
    send_connection(&sender, "10.1.1.2:1002".parse().unwrap());
    observer
        .wait_until(|| lock(&observer.terminal).len() == 2)
        .await;
    cancellation.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        ListenerRunOutcome::Stopped { .. }
    ));
    let admitted = lock(&observer.admitted);
    let terminal = lock(&observer.terminal);
    assert_eq!(admitted.len(), 2);
    assert_eq!(terminal.len(), 2);
    for context in admitted.iter() {
        let matches = terminal
            .iter()
            .filter(|(closed, _)| closed.connection_id == context.connection_id)
            .count();
        assert_eq!(matches, 1);
        assert_eq!(context.runtime_epoch, terminal[0].0.runtime_epoch);
        assert_eq!(context.channel, terminal[0].0.channel);
    }
}

#[tokio::test]
async fn child_panic_faults_listener_and_cancels_sibling() {
    let sibling_entered = Arc::new(Notify::new());
    let sibling_cancelled = Arc::new(Notify::new());
    let observer = Arc::new(RecordingObserver::default());
    let supervisor = Arc::new(supervisor(
        Arc::new(FakeHandler(HandlerMode::PanicChildAfterSibling {
            calls: AtomicUsize::new(0),
            sibling_entered: Arc::clone(&sibling_entered),
            sibling_cancelled: Arc::clone(&sibling_cancelled),
        })),
        Arc::clone(&observer),
        2,
        Duration::from_millis(50),
    ));
    let (listener, sender) = fake_listener();
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        async move { supervisor.run_bound(listener, cancellation).await }
    });
    send_connection(&sender, "10.1.1.1:1001".parse().unwrap());
    tokio::time::timeout(Duration::from_secs(2), sibling_entered.notified())
        .await
        .expect("sibling connection entered");
    send_connection(&sender, "10.1.1.2:1002".parse().unwrap());
    tokio::time::timeout(Duration::from_secs(2), sibling_cancelled.notified())
        .await
        .expect("sibling connection cancelled");
    assert!(matches!(
        task.await.unwrap().unwrap(),
        ListenerRunOutcome::Faulted {
            code,
            ..
        } if code == CONNECTION_CHILD_TASK_PANICKED
    ));
    assert_eq!(lock(&observer.terminal).len(), 2);
}

#[tokio::test]
async fn child_panic_interrupts_pending_primary_and_faults_listener() {
    let observer = Arc::new(RecordingObserver::default());
    let supervisor = Arc::new(supervisor(
        Arc::new(FakeHandler(HandlerMode::PanicChildPendingPrimary)),
        Arc::clone(&observer),
        1,
        Duration::from_millis(50),
    ));
    let (listener, sender) = fake_listener();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        async move {
            supervisor
                .run_bound(listener, CancellationToken::new())
                .await
        }
    });
    send_connection(&sender, "10.1.1.1:1001".parse().unwrap());

    let outcome = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("child panic must not wait for the pending primary")
        .unwrap()
        .unwrap();
    assert!(matches!(
        outcome,
        ListenerRunOutcome::Faulted {
            code: CONNECTION_CHILD_TASK_PANICKED,
            ..
        }
    ));
}

#[tokio::test]
async fn forced_abort_after_grace_emits_one_terminal_and_faults_listener() {
    let observer = Arc::new(RecordingObserver::default());
    let supervisor = Arc::new(supervisor(
        Arc::new(FakeHandler(HandlerMode::PendingChild)),
        Arc::clone(&observer),
        1,
        Duration::from_millis(20),
    ));
    let (listener, sender) = fake_listener();
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let cancellation = cancellation.clone();
        async move { supervisor.run_bound(listener, cancellation).await }
    });
    send_connection(&sender, "10.1.1.1:1001".parse().unwrap());
    observer
        .wait_until(|| lock(&observer.admitted).len() == 1)
        .await;
    cancellation.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        ListenerRunOutcome::Faulted {
            code,
            ..
        } if code == LISTENER_SHUTDOWN_GRACE_EXCEEDED
    ));
    let terminal = lock(&observer.terminal);
    assert_eq!(terminal.len(), 1);
    assert_eq!(
        terminal[0].1,
        TerminalConnectionOutcome::ShutdownGraceExceeded
    );
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
