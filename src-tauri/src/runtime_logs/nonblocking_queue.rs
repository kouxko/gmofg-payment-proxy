//! 运行期观测专用的有界、非阻塞生产者队列。
//!
//! tracing Layer 只能调用 [`BoundedSender::try_send`]：队列锁竞争、容量耗尽或
//! consumer 已关闭时立即返回，由调用者记录丢弃计数。独立 owner 明确关闭 sender
//! 并 join consumer，避免把后台线程的生命周期遗失在 subscriber 中。

use std::{
    fmt, io,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicUsize, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread::JoinHandle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueDropReason {
    Full,
    BytesFull,
    Disconnected,
    Contended,
}

pub(super) trait BoundedMessage {
    fn logical_bytes(&self) -> usize;
}

/// Shared byte admission for all runtime-observation queues.
///
/// Count-bounded channels alone are unsafe because one tracing event may contain a multi-megabyte
/// HTTP body or Socket frame. Reservation happens before channel admission and the envelope releases
/// it on every receive, send failure, disconnect, or panic path.
#[derive(Debug)]
pub(super) struct QueueByteBudget {
    limit: usize,
    used: AtomicUsize,
}

impl QueueByteBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            used: AtomicUsize::new(0),
        }
    }

    pub(super) const fn limit(&self) -> usize {
        self.limit
    }

    fn reserve(self: &Arc<Self>, bytes: usize) -> Option<QueueReservation> {
        if bytes > self.limit {
            return None;
        }
        let mut used = self.used.load(Ordering::Relaxed);
        loop {
            let next = used.checked_add(bytes)?;
            if next > self.limit {
                return None;
            }
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => {
                    return Some(QueueReservation {
                        bytes,
                        budget: Arc::clone(self),
                    });
                }
                Err(actual) => used = actual,
            }
        }
    }

    #[cfg(test)]
    fn used_bytes(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
struct QueueReservation {
    bytes: usize,
    budget: Arc<QueueByteBudget>,
}

impl Drop for QueueReservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[derive(Debug)]
pub(super) struct Budgeted<T> {
    pub(super) message: T,
    _reservation: QueueReservation,
}

pub(super) struct BoundedSender<T> {
    sender: Arc<Mutex<Option<SyncSender<Budgeted<T>>>>>,
    budget: Arc<QueueByteBudget>,
}

impl<T> Clone for BoundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: Arc::clone(&self.sender),
            budget: Arc::clone(&self.budget),
        }
    }
}

impl<T> fmt::Debug for BoundedSender<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedSender")
            .finish_non_exhaustive()
    }
}

impl<T: BoundedMessage> BoundedSender<T> {
    pub(super) fn try_send(&self, message: T) -> Result<(), QueueDropReason> {
        let Some(reservation) = self.budget.reserve(message.logical_bytes()) else {
            return Err(QueueDropReason::BytesFull);
        };
        let guard = match self.sender.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(QueueDropReason::Contended),
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
        };
        let Some(sender) = guard.as_ref() else {
            return Err(QueueDropReason::Disconnected);
        };
        sender
            .try_send(Budgeted {
                message,
                _reservation: reservation,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => QueueDropReason::Full,
                TrySendError::Disconnected(_) => QueueDropReason::Disconnected,
            })
    }

    #[cfg(test)]
    pub(super) fn from_sync_sender(
        sender: SyncSender<Budgeted<T>>,
        budget: Arc<QueueByteBudget>,
    ) -> Self {
        Self {
            sender: Arc::new(Mutex::new(Some(sender))),
            budget,
        }
    }
}

#[derive(Debug)]
pub(super) struct ConsumerOwner<T> {
    sender: Arc<Mutex<Option<SyncSender<Budgeted<T>>>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl<T> ConsumerOwner<T> {
    /// Stops admission, drains already accepted messages and joins the consumer.
    pub(super) fn shutdown(&self) -> io::Result<()> {
        self.sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(join) = self
            .join
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return Ok(());
        };
        join.join()
            .map_err(|_| io::Error::other("runtime observation consumer panicked"))
    }
}

impl<T> Drop for ConsumerOwner<T> {
    fn drop(&mut self) {
        // RAII 只覆盖初始化失败或测试提前返回；生产正常路径仍显式调用 shutdown。
        let _ = self.shutdown();
    }
}

pub(super) fn spawn_bounded_consumer<T, F>(
    name: &str,
    capacity: usize,
    budget: Arc<QueueByteBudget>,
    mut consume: F,
) -> io::Result<(BoundedSender<T>, ConsumerOwner<T>)>
where
    T: BoundedMessage + Send + 'static,
    F: FnMut(T) + Send + 'static,
{
    if capacity == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runtime observation queue capacity must be positive",
        ));
    }
    let (sender, receiver) = sync_channel::<Budgeted<T>>(capacity);
    let sender = Arc::new(Mutex::new(Some(sender)));
    let join = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            while let Ok(message) = receiver.recv() {
                consume(message.message);
            }
        })?;
    Ok((
        BoundedSender {
            sender: Arc::clone(&sender),
            budget,
        },
        ConsumerOwner {
            sender,
            join: Mutex::new(Some(join)),
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc::sync_channel};

    use super::{BoundedMessage, BoundedSender, QueueByteBudget, QueueDropReason};

    impl BoundedMessage for &'static str {
        fn logical_bytes(&self) -> usize {
            self.len()
        }
    }

    #[test]
    fn try_send_reports_contended_instead_of_waiting_for_sender_lock() {
        let (sender, _receiver) = sync_channel(1);
        let sender = BoundedSender::from_sync_sender(sender, Arc::new(QueueByteBudget::new(64)));
        let held = sender.sender.lock().expect("sender lock");

        let result = sender.try_send("observation");

        assert_eq!(result, Err(QueueDropReason::Contended));
        drop(held);
    }

    #[test]
    fn byte_budget_rejects_oversized_messages_and_releases_failed_sends() {
        let (channel, receiver) = sync_channel(1);
        let budget = Arc::new(QueueByteBudget::new(4));
        let sender = BoundedSender::from_sync_sender(channel, Arc::clone(&budget));

        assert_eq!(sender.try_send("12345"), Err(QueueDropReason::BytesFull));
        assert_eq!(budget.used_bytes(), 0);
        sender.try_send("1234").unwrap();
        assert_eq!(budget.used_bytes(), 4);
        assert_eq!(sender.try_send("1"), Err(QueueDropReason::BytesFull));
        drop(receiver.recv().unwrap());
        assert_eq!(budget.used_bytes(), 0);
    }
}
