use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};

use tokio::sync::Notify;

use super::{ChildTaskError, ConnectionTaskScope, ScopePhase};

async fn poll_once<F>(mut future: Pin<&mut F>) -> Poll<F::Output>
where
    F: Future,
{
    std::future::poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await
}

struct PollCountingPending {
    polls: Arc<AtomicUsize>,
    first_poll: Option<Arc<Notify>>,
}

impl Future for PollCountingPending {
    type Output = Result<(), ChildTaskError>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        if let Some(first_poll) = &self.first_poll {
            first_poll.notify_one();
        }
        Poll::Pending
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_spawn_barrier_accepts_and_drains_or_rejects_without_polling() {
    for _ in 0..64 {
        let scope = ConnectionTaskScope::new();
        let barrier = Arc::new(Barrier::new(2));
        let polls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());

        let spawn_scope = scope.clone();
        let spawn_barrier = Arc::clone(&barrier);
        let spawn_polls = Arc::clone(&polls);
        let spawn_release = Arc::clone(&release);
        let spawn = tokio::task::spawn_blocking(move || {
            spawn_barrier.wait();
            spawn_scope.spawn_owned(async move {
                spawn_polls.fetch_add(1, Ordering::SeqCst);
                spawn_release.notified().await;
                Ok(())
            })
        });

        barrier.wait();
        scope.close();
        let submitted = spawn.await.expect("spawn contender must join");
        let snapshot = scope.snapshot();
        assert_eq!(snapshot.phase, ScopePhase::Closed);

        match submitted {
            Ok(_) => {
                assert_eq!(snapshot.live_count, 1);
                release.notify_one();
            }
            Err(_) => assert_eq!(polls.load(Ordering::SeqCst), 0),
        }

        let rejected_polls = Arc::new(AtomicUsize::new(0));
        assert!(
            scope
                .spawn_owned(PollCountingPending {
                    polls: Arc::clone(&rejected_polls),
                    first_poll: None,
                })
                .is_err()
        );
        assert_eq!(rejected_polls.load(Ordering::SeqCst), 0);

        scope.drain().await;
        assert_eq!(scope.snapshot().live_count, 0);
    }
}

#[tokio::test]
async fn fast_completion_does_not_leave_an_abort_handle() {
    let scope = ConnectionTaskScope::new();
    scope
        .spawn_owned(async { Ok(()) })
        .expect("open scope accepts child");
    scope.close_and_drain().await;

    let snapshot = scope.snapshot();
    assert_eq!(snapshot.live_count, 0);
    assert_eq!(snapshot.aggregate.completed_count, 1);
}

#[tokio::test]
async fn cancellation_ignorant_pending_child_is_forced_to_abort_and_drain() {
    let scope = ConnectionTaskScope::new();
    let polls = Arc::new(AtomicUsize::new(0));
    let first_poll = Arc::new(Notify::new());
    let polled = first_poll.notified();
    let id = scope
        .spawn_owned(PollCountingPending {
            polls: Arc::clone(&polls),
            first_poll: Some(Arc::clone(&first_poll)),
        })
        .expect("open scope accepts child");
    polled.await;
    assert!(polls.load(Ordering::SeqCst) > 0);

    scope.close();
    let mut drain = Box::pin(scope.drain());
    assert!(matches!(poll_once(drain.as_mut()).await, Poll::Pending));
    drop(drain);
    let aborted = scope.abort_live();
    assert_eq!(aborted, vec![id]);
    scope.drain().await;
    assert_eq!(scope.snapshot().live_count, 0);
}

#[tokio::test]
async fn ordinary_errors_use_lowest_registered_id_not_completion_order() {
    let scope = ConnectionTaskScope::new();
    let release_low = Arc::new(Notify::new());
    let low_release = Arc::clone(&release_low);
    let low = scope
        .spawn_owned(async move {
            low_release.notified().await;
            Err(ChildTaskError::new("LOW", "low id"))
        })
        .expect("first child accepted");
    let high = scope
        .spawn_owned(async { Err(ChildTaskError::new("HIGH", "high id")) })
        .expect("second child accepted");
    assert!(low < high);
    scope.wait_for_completed_count(1).await;
    release_low.notify_one();
    let aggregate = scope.close_and_drain().await;

    assert_eq!(aggregate.completed_count, 2);
    assert_eq!(
        aggregate.lowest_error.as_ref().map(|entry| entry.0),
        Some(low)
    );
    assert_eq!(
        aggregate.lowest_error.as_ref().map(|entry| entry.1.code),
        Some("LOW")
    );
}

#[tokio::test]
async fn panic_is_recorded_independently_of_lower_id_error() {
    let scope = ConnectionTaskScope::new();
    let low = scope
        .spawn_owned(async { Err(ChildTaskError::new("LOW", "ordinary")) })
        .expect("error child accepted");
    scope
        .spawn_owned(async {
            panic!("child panic");
            #[allow(unreachable_code)]
            Ok(())
        })
        .expect("panic child accepted");
    let aggregate = scope.close_and_drain().await;

    assert!(aggregate.panic_seen);
    assert_eq!(
        aggregate.lowest_error.as_ref().map(|entry| entry.0),
        Some(low)
    );
    assert_eq!(aggregate.completed_count, 2);
}

#[tokio::test]
async fn many_sequential_children_return_live_state_to_constant_size() {
    let scope = ConnectionTaskScope::new();
    let state_size = std::mem::size_of_val(&scope.snapshot().aggregate);

    for expected in 1..=2_000 {
        scope
            .spawn_owned(async { Ok(()) })
            .expect("open scope accepts child");
        scope.wait_for_completed_count(expected).await;
        let snapshot = scope.snapshot();
        assert_eq!(snapshot.live_count, 0);
        assert_eq!(std::mem::size_of_val(&snapshot.aggregate), state_size);
    }

    let aggregate = scope.close_and_drain().await;
    assert_eq!(aggregate.completed_count, 2_000);
    assert!(aggregate.lowest_error.is_none());
}

#[tokio::test]
async fn forced_abort_targets_only_current_live_children() {
    let scope = ConnectionTaskScope::new();
    for _ in 0..128 {
        scope
            .spawn_owned(async { Ok(()) })
            .expect("completed child accepted");
    }
    scope.wait_for_completed_count(128).await;

    let first = scope
        .spawn_owned(std::future::pending())
        .expect("first pending child accepted");
    let second = scope
        .spawn_owned(std::future::pending())
        .expect("second pending child accepted");
    assert_eq!(scope.snapshot().live_count, 2);

    scope.close();
    assert_eq!(scope.abort_live(), vec![first, second]);
    scope.drain().await;
    let snapshot = scope.snapshot();
    assert_eq!(snapshot.live_count, 0);
    assert_eq!(snapshot.aggregate.completed_count, 128);
}
