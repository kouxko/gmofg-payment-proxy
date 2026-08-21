//! 已完成 Socket capture 的有界、非阻塞发布器。
//!
//! Listener 连接任务和 Rhai blocking worker 都只能调用 [`SocketCapturePublisher::publish`]。
//! 真正的 `SQLite` 写入由独立 drain 线程串行执行；队列已满、仓储失败或线程退出时 capture
//! 可以丢弃，但绝不能反向改变已经 write + flush 的网络结果。

use std::{
    fmt,
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    EventHub, SocketCaptureId, SocketCapturePayload, SocketCaptureRecord, SocketConnectionId,
    SocketDisplayDiagnostic, SocketDisplayFallbackReason, SocketDisplayResult, UiEventPayload,
};
use intercept_proxy_domain::{ListenerId, WorkspaceId};
use intercept_proxy_protocol_scripting::{DisplayFallbackReason, ProtocolDisplayResult};
use intercept_proxy_runtime::SocketConnectionIdentity;
use parking_lot::{Mutex, RwLock};

use crate::adapters::SocketCaptureRepositoryAdapter;
use crate::sqlite::socket_capture_coordination::SocketCaptureGeneration;

mod external_diagnostics;

const SOCKET_CAPTURE_QUEUE_CAPACITY: usize = 256;
const SOCKET_CAPTURE_QUEUE_MAX_LOGICAL_BYTES: u64 = 64 * 1024 * 1024;
const SOCKET_CAPTURE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(super) struct SocketCapturePublisher {
    inner: Arc<PublisherInner>,
}

struct PublisherInner {
    sender: Mutex<Option<SyncSender<QueuedCapture>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    worker_done: Mutex<Option<Receiver<()>>>,
    repository: Arc<SocketCaptureRepositoryAdapter>,
    events: Arc<RwLock<Arc<EventHub>>>,
    budget: Arc<QueueBudget>,
    queue_full_warned: Arc<AtomicBool>,
    disconnected_warned: Arc<AtomicBool>,
    #[cfg(test)]
    display_gate: Mutex<Option<TestDisplayGate>>,
    #[cfg(test)]
    completion_event_gate: Arc<Mutex<Option<TestDisplayGate>>>,
}

#[cfg(test)]
struct TestDisplayGate {
    entered: std::sync::mpsc::SyncSender<()>,
    release: Receiver<()>,
}

struct QueuedCapture {
    record: SocketCaptureRecord,
    generation: SocketCaptureGeneration,
    _reservation: QueueReservation,
}

/// `output_committed` 线性化点冻结的 Workspace 代次。
///
/// Display 可以晚于 clear/reset 完成，但最终持久化只能使用这张早期票，不能重新读取代次。
pub(super) struct SocketCapturePublishTicket {
    workspace_id: WorkspaceId,
    generation: SocketCaptureGeneration,
}

#[derive(Debug)]
struct QueueBudget {
    used: AtomicU64,
    limit: u64,
}

struct QueueReservation {
    budget: Arc<QueueBudget>,
    bytes: u64,
}

impl QueueBudget {
    fn reserve(self: &Arc<Self>, bytes: u64) -> Option<QueueReservation> {
        let mut current = self.used.load(Ordering::Acquire);
        loop {
            let next = current.checked_add(bytes)?;
            if next > self.limit {
                return None;
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(QueueReservation {
                        budget: Arc::clone(self),
                        bytes,
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for QueueReservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub(super) fn capture_display(result: ProtocolDisplayResult) -> SocketDisplayResult {
    match result {
        ProtocolDisplayResult::UntrustedHtml(html) => SocketDisplayResult::UntrustedHtml { html },
        ProtocolDisplayResult::HexFallback(reason) => {
            let (reason, diagnostic) = match reason {
                DisplayFallbackReason::EntryPointFailed => (
                    SocketDisplayFallbackReason::EntryPointFailed,
                    Some(SocketDisplayDiagnostic {
                        code: "DISPLAY_ENTRY_FAILED".to_owned(),
                        message: "Display 执行失败，已回退 Hex。".to_owned(),
                        external_package_call: None,
                    }),
                ),
                DisplayFallbackReason::ResourceLimitExceeded(limit) => (
                    SocketDisplayFallbackReason::ResourceLimitExceeded,
                    Some(SocketDisplayDiagnostic {
                        code: "DISPLAY_RESOURCE_LIMIT_EXCEEDED".to_owned(),
                        message: format!("Display 超出资源限制（{limit}），已回退 Hex。"),
                        external_package_call: None,
                    }),
                ),
            };
            SocketDisplayResult::HexFallback { reason, diagnostic }
        }
    }
}

pub(super) fn capture_resource_busy() -> SocketDisplayResult {
    SocketDisplayResult::HexFallback {
        reason: SocketDisplayFallbackReason::ResourceLimitExceeded,
        diagnostic: Some(SocketDisplayDiagnostic {
            code: "DISPLAY_RESOURCE_BUSY".to_owned(),
            message: "Display 后台资源繁忙，已回退 Hex。".to_owned(),
            external_package_call: None,
        }),
    }
}

impl SocketCapturePublisher {
    pub(super) fn new(
        repository: Arc<SocketCaptureRepositoryAdapter>,
        events: Arc<RwLock<Arc<EventHub>>>,
    ) -> Self {
        let (sender, receiver) = sync_channel::<QueuedCapture>(SOCKET_CAPTURE_QUEUE_CAPACITY);
        let (done_sender, done_receiver) = std::sync::mpsc::channel();
        let persistence_warned = Arc::new(AtomicBool::new(false));
        let worker_events = Arc::clone(&events);
        let worker_persistence_warned = Arc::clone(&persistence_warned);
        let worker_repository = Arc::clone(&repository);
        #[cfg(test)]
        let completion_event_gate = Arc::new(Mutex::new(None));
        #[cfg(test)]
        let worker_completion_event_gate = Arc::clone(&completion_event_gate);
        let worker = thread::Builder::new()
            .name("socket-capture-drain".to_owned())
            .spawn(move || {
                while let Ok(queued) = receiver.recv() {
                    // Capture 是已提交线路的旁路证据。持久化失败只能丢弃该条记录；这里不向
                    // connection task 回传错误，也不记录可能包含业务字段的 payload。
                    let runtime_epoch = queued.record.runtime_epoch;
                    let Some(_completion_permit) =
                        worker_repository.completion_if_current(&queued.generation)
                    else {
                        continue;
                    };
                    match worker_repository.record_if_current(&queued.record, &queued.generation) {
                        Ok(Some(row)) => {
                            #[cfg(test)]
                            wait_on_test_gate(&worker_completion_event_gate);
                            worker_events.read().publish(
                                Some(runtime_epoch),
                                row.completed_at,
                                Some(row.capture_id.to_string()),
                                None,
                                UiEventPayload::SocketCaptureCompleted(row),
                            );
                        }
                        Ok(None) => {}
                        Err(_) => publish_warning_once(
                            &worker_events,
                            &worker_persistence_warned,
                            "Socket 抓包写入失败；网络数据已正常提交，但本次抓包未保存。",
                        ),
                    }
                }
                let _ = done_sender.send(());
            });
        let (sender, worker, worker_done) = match worker {
            Ok(worker) => (Some(sender), Some(worker), Some(done_receiver)),
            Err(_) => (None, None, None),
        };
        if sender.is_none() {
            publish_warning_once(
                &events,
                &AtomicBool::new(false),
                "Socket 抓包后台任务启动失败；代理仍可运行，但抓包不会保存。",
            );
        }
        Self {
            inner: Arc::new(PublisherInner {
                sender: Mutex::new(sender),
                worker: Mutex::new(worker),
                worker_done: Mutex::new(worker_done),
                repository,
                events,
                budget: Arc::new(QueueBudget {
                    used: AtomicU64::new(0),
                    limit: SOCKET_CAPTURE_QUEUE_MAX_LOGICAL_BYTES,
                }),
                queue_full_warned: Arc::new(AtomicBool::new(false)),
                disconnected_warned: Arc::new(AtomicBool::new(false)),
                #[cfg(test)]
                display_gate: Mutex::new(None),
                #[cfg(test)]
                completion_event_gate,
            }),
        }
    }

    /// 尝试发布一条完整 capture，永不等待队列空间。
    fn ticket(&self, workspace_id: WorkspaceId) -> SocketCapturePublishTicket {
        SocketCapturePublishTicket {
            workspace_id,
            generation: self.inner.repository.generation_for(workspace_id),
        }
    }

    pub(super) fn publish(&self, record: SocketCaptureRecord, ticket: SocketCapturePublishTicket) {
        debug_assert_eq!(record.workspace_id, ticket.workspace_id);
        if record.workspace_id != ticket.workspace_id {
            return;
        }
        let Some(reservation) = self.inner.budget.reserve(record.logical_bytes()) else {
            publish_warning_once(
                &self.inner.events,
                &self.inner.queue_full_warned,
                "Socket 抓包队列已满；网络数据已正常提交，但部分抓包被丢弃。",
            );
            return;
        };
        let queued = QueuedCapture {
            record,
            generation: ticket.generation,
            _reservation: reservation,
        };
        let sender = self.inner.sender.lock();
        let Some(sender) = sender.as_ref() else {
            publish_warning_once(
                &self.inner.events,
                &self.inner.disconnected_warned,
                "Socket 抓包后台任务不可用；网络数据已正常提交，但抓包未保存。",
            );
            return;
        };
        match sender.try_send(queued) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => publish_warning_once(
                &self.inner.events,
                &self.inner.queue_full_warned,
                "Socket 抓包队列已满；网络数据已正常提交，但部分抓包被丢弃。",
            ),
            Err(TrySendError::Disconnected(_)) => publish_warning_once(
                &self.inner.events,
                &self.inner.disconnected_warned,
                "Socket 抓包后台任务不可用；网络数据已正常提交，但抓包未保存。",
            ),
        }
    }

    #[cfg(test)]
    fn close_and_drain(&self) -> bool {
        self.inner.close_and_drain(SOCKET_CAPTURE_SHUTDOWN_TIMEOUT)
    }

    #[cfg(test)]
    pub(super) fn block_next_display(
        &self,
        entered: std::sync::mpsc::SyncSender<()>,
        release: Receiver<()>,
    ) {
        *self.inner.display_gate.lock() = Some(TestDisplayGate { entered, release });
    }

    #[cfg(test)]
    fn wait_before_display(&self) {
        wait_on_test_gate(&self.inner.display_gate);
    }

    #[cfg(test)]
    fn block_next_completion_event(
        &self,
        entered: std::sync::mpsc::SyncSender<()>,
        release: Receiver<()>,
    ) {
        *self.inner.completion_event_gate.lock() = Some(TestDisplayGate { entered, release });
    }
}

#[cfg(test)]
fn wait_on_test_gate(gate: &Mutex<Option<TestDisplayGate>>) {
    let Some(gate) = gate.lock().take() else {
        return;
    };
    let _ = gate.entered.send(());
    let _ = gate.release.recv();
}

impl PublisherInner {
    fn close_and_drain(&self, timeout: Duration) -> bool {
        self.sender.lock().take();
        let completed = self
            .worker_done
            .lock()
            .take()
            .is_none_or(|done| done.recv_timeout(timeout).is_ok());
        if completed && let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        } else if !completed {
            // `JoinHandle` 没有超时 join；超过预算后显式放弃句柄，避免后续 Drop 再次阻塞。
            self.worker.lock().take();
        }
        completed
    }
}

impl Drop for PublisherInner {
    fn drop(&mut self) {
        let _ = self.close_and_drain(SOCKET_CAPTURE_SHUTDOWN_TIMEOUT);
    }
}

fn publish_warning_once(
    events: &RwLock<Arc<EventHub>>,
    warned: &AtomicBool,
    message: &'static str,
) {
    if warned.swap(true, Ordering::AcqRel) {
        return;
    }
    // 即使未来 EventHub 实现变化，旁路告警也不得让连接/Rhai worker unwind。
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let Some(events) = events.try_read() else {
            // 当前连接线程不等待锁；恢复标志，让后续丢弃有机会再次尝试发布一次告警。
            warned.store(false, Ordering::Release);
            return;
        };
        events.publish(
            None,
            Utc::now(),
            None,
            None,
            UiEventPayload::ResourceWarning {
                message: message.to_owned(),
            },
        );
    }));
}

impl fmt::Debug for SocketCapturePublisher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketCapturePublisher")
            .field("queue_capacity", &SOCKET_CAPTURE_QUEUE_CAPACITY)
            .field("queue_logical_bytes", &self.inner.budget.limit)
            .finish_non_exhaustive()
    }
}

/// Factory 在 Listener start 时冻结的 capture 归属。
#[derive(Clone, Debug)]
pub(super) struct SocketCaptureContext {
    pub(super) workspace_id: WorkspaceId,
    pub(super) listener_id: ListenerId,
    pub(super) publisher: Option<SocketCapturePublisher>,
}

impl SocketCaptureContext {
    pub(super) fn ticket(&self) -> Option<SocketCapturePublishTicket> {
        self.publisher
            .as_ref()
            .map(|publisher| publisher.ticket(self.workspace_id))
    }

    #[cfg(test)]
    pub(super) fn wait_before_display(&self) {
        if let Some(publisher) = &self.publisher {
            publisher.wait_before_display();
        }
    }

    pub(super) fn record(
        &self,
        ticket: Option<SocketCapturePublishTicket>,
        connection: &SocketConnectionIdentity,
        occurred_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
        payload: SocketCapturePayload,
    ) {
        let (Some(publisher), Some(ticket)) = (&self.publisher, ticket) else {
            return;
        };
        let record = SocketCaptureRecord {
            capture_id: SocketCaptureId::new(),
            runtime_epoch: connection.runtime_epoch,
            workspace_id: self.workspace_id,
            listener_id: self.listener_id,
            // 当前 Socket 后端以一条已接纳连接作为 session 聚合边界；每个 Frame/exchange
            // 仍有独立 capture_id，LocalResponder 另有 exchange_id。
            session_id: connection.connection_id,
            connection_id: SocketConnectionId::from_uuid(connection.connection_id),
            peer_address: connection.peer_addr.to_string(),
            occurred_at,
            completed_at,
            payload,
        };
        debug_assert!(record.is_consistent());
        if record.is_consistent() {
            publisher.publish(record, ticket);
        }
    }
}

#[cfg(test)]
#[path = "socket_capture_publisher/tests.rs"]
mod tests;
