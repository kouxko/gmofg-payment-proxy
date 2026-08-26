use std::{
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    task::{Context, Poll, Waker},
    thread,
};

use super::support::*;
use crate::EnvironmentCandidatePolicy;

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    let mut context = Context::from_waker(Waker::noop());
    future.poll(&mut context)
}

#[test]
fn registry_owned_worker_claims_queued_applies_in_fifo_order() {
    let registry = EnvironmentCandidateRegistry::new(
        EnvironmentCandidatePolicy::new(4, 1, 2, 1, 32, 4_194_304)
            .expect("bounded test policy is valid"),
    );
    let first = admit_preview_ready(&registry, "FIFO First", 1);
    let second = admit_preview_ready(&registry, "FIFO Second", 1);
    let first_ack = registry
        .queue_apply(first.candidate_id(), &token_from_create(&first))
        .expect("first apply queues");
    let second_ack = registry
        .queue_apply(second.candidate_id(), &token_from_create(&second))
        .expect("second apply queues");

    let first_work = registry
        .claim_next_apply()
        .expect("FIFO claim succeeds")
        .expect("first work exists");

    assert_eq!(first_work.candidate_id(), first.candidate_id());
    assert_eq!(first_work.apply_task_id(), first_ack.apply_task_id());
    first_work
        .finish_failed_before_commit(EnvironmentStatusCode::CommitFailed)
        .expect("first guard terminalizes");

    let second_work = registry
        .claim_next_apply()
        .expect("FIFO claim succeeds")
        .expect("second work exists");
    assert_eq!(second_work.candidate_id(), second.candidate_id());
    assert_eq!(second_work.apply_task_id(), second_ack.apply_task_id());
}

#[test]
fn normal_cancel_wins_before_fifo_worker_claim() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Cancel Wins", 1);
    registry
        .queue_apply(ready.candidate_id(), &token_from_create(&ready))
        .expect("apply queues");

    assert_eq!(
        registry.cancel(ready.candidate_id()).status(),
        EnvironmentCancelStatus::Cancelled
    );
    assert!(
        registry
            .claim_next_apply()
            .expect("FIFO observation succeeds")
            .is_none(),
        "cancelled queued work is removed from the owned FIFO"
    );
}

#[test]
fn worker_claim_wins_before_normal_cancel() {
    let registry = registry();
    let (ready, work) = claim_apply(&registry, "Worker Wins");

    assert_eq!(
        registry.cancel(ready.candidate_id()).status(),
        EnvironmentCancelStatus::ApplyInProgressNotCancellable
    );

    work.finish_failed_before_commit(EnvironmentStatusCode::CommitFailed)
        .expect("owned worker drains to a terminal state");
}

#[test]
fn dropping_unfinished_apply_guard_terminalizes_commit_failed() {
    let registry = registry();
    let (ready, work) = claim_apply(&registry, "Dropped Guard");

    drop(work);

    let status = json(&registry.status(ready.candidate_id()));
    assert_eq!(status["status"], "failed_before_commit");
    assert_eq!(status["terminal_result"]["status_code"], "COMMIT_FAILED");
    assert_eq!(registry.metrics().active_apply_tasks(), 0);
    assert_eq!(registry.metrics().private_candidate_bytes(), 0);
}

#[test]
fn panic_unwind_drops_guard_and_terminalizes_commit_failed() {
    let registry = registry();
    let (ready, work) = claim_apply(&registry, "Panicked Guard");

    let panic = catch_unwind(AssertUnwindSafe(move || {
        let _owned = work;
        panic!("simulated worker panic");
    }));

    assert!(panic.is_err());
    let status = json(&registry.status(ready.candidate_id()));
    assert_eq!(status["status"], "failed_before_commit");
    assert_eq!(status["terminal_result"]["status_code"], "COMMIT_FAILED");
}

#[test]
fn explicit_failed_before_commit_guard_method_has_no_persisted_identifiers() {
    let registry = registry();
    let (ready, work) = claim_apply(&registry, "Failed Guard");

    work.finish_failed_before_commit(EnvironmentStatusCode::ProtectedMaterialPrepareFailed)
        .expect("guard records pre-commit failure");
    let terminal = json(&registry.status(ready.candidate_id()))["terminal_result"].clone();

    assert_eq!(terminal["result"], "failed_before_commit");
    assert_eq!(terminal["status_code"], "PROTECTED_MATERIAL_PREPARE_FAILED");
    assert!(terminal.get("workspace_id").is_none());
    assert!(terminal.get("revision").is_none());
    assert!(terminal.get("selected_workspace_id").is_none());
}

#[test]
fn explicit_rolled_back_guard_method_has_no_persisted_identifiers() {
    let registry = registry();
    let (ready, work) = claim_apply(&registry, "Rollback Guard");

    work.finish_rolled_back(EnvironmentStatusCode::CommitRolledBack)
        .expect("guard records rollback");
    let terminal = json(&registry.status(ready.candidate_id()))["terminal_result"].clone();

    assert_eq!(terminal["result"], "rolled_back");
    assert_eq!(terminal["status_code"], "COMMIT_ROLLED_BACK");
    assert!(terminal.get("workspace_id").is_none());
    assert!(terminal.get("revision").is_none());
    assert!(terminal.get("selected_workspace_id").is_none());
}

#[test]
fn shutdown_cancels_queued_work_and_drain_is_immediately_ready() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Shutdown Queued", 1);
    registry
        .queue_apply(ready.candidate_id(), &token_from_create(&ready))
        .expect("apply queues");

    let mut drain = Box::pin(registry.begin_shutdown());

    assert_eq!(poll_once(drain.as_mut()), Poll::Ready(()));
    assert_eq!(
        registry.status(ready.candidate_id()).status(),
        EnvironmentCandidateStatus::CancelledByShutdown
    );
    assert_eq!(registry.metrics().active_apply_tasks(), 0);
}

#[test]
fn shutdown_drain_is_pending_until_in_progress_guard_finishes() {
    let registry = registry();
    let (ready, work) = claim_apply(&registry, "Shutdown In Progress");

    let mut drain = Box::pin(registry.begin_shutdown());

    assert_eq!(poll_once(drain.as_mut()), Poll::Pending);
    assert_eq!(
        registry.status(ready.candidate_id()).status(),
        EnvironmentCandidateStatus::ApplyInProgress
    );
    work.finish_failed_before_commit(EnvironmentStatusCode::ShutdownInProgress)
        .expect("worker drains to terminal");
    assert_eq!(poll_once(drain.as_mut()), Poll::Ready(()));
}

#[test]
fn shutdown_drain_is_released_when_in_progress_guard_is_dropped() {
    let registry = registry();
    let (_ready, work) = claim_apply(&registry, "Shutdown Drop");
    let mut drain = Box::pin(registry.begin_shutdown());
    assert_eq!(poll_once(drain.as_mut()), Poll::Pending);

    drop(work);

    assert_eq!(poll_once(drain.as_mut()), Poll::Ready(()));
}

#[test]
fn shutdown_publishes_drain_state_under_lock_and_two_workers_finish_in_reverse_order() {
    let registry = std::sync::Arc::new(EnvironmentCandidateRegistry::new(
        EnvironmentCandidatePolicy::new(4, 1, 2, 1, 32, 4_194_304)
            .expect("two-worker shutdown policy is valid"),
    ));
    let first = admit_preview_ready(&registry, "Drain First", 1);
    let second = admit_preview_ready(&registry, "Drain Second", 1);
    registry
        .queue_apply(first.candidate_id(), &token_from_create(&first))
        .expect("first apply queues");
    registry
        .queue_apply(second.candidate_id(), &token_from_create(&second))
        .expect("second apply queues");
    let first_work = registry
        .claim_next_apply()
        .expect("first claim observes FIFO")
        .expect("first work exists");
    let second_work = registry
        .claim_next_apply()
        .expect("second claim observes FIFO")
        .expect("second work exists");

    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(0);
    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);
    let shutdown_registry = std::sync::Arc::clone(&registry);
    let shutdown = thread::spawn(move || {
        shutdown_registry.begin_shutdown_with_barrier(entered_sender, release_receiver)
    });
    entered_receiver
        .recv()
        .expect("shutdown holds the state lock before publishing drain count");

    let (worker_started_sender, worker_started_receiver) = std::sync::mpsc::sync_channel(0);
    let second_completion = thread::spawn(move || {
        worker_started_sender
            .send(())
            .expect("test observes the reverse-order worker");
        second_work.finish_failed_before_commit(EnvironmentStatusCode::ShutdownInProgress)
    });
    worker_started_receiver
        .recv()
        .expect("second worker attempts completion while shutdown owns the lock");
    release_sender
        .send(())
        .expect("shutdown may publish its synchronized count");

    let mut drain = Box::pin(shutdown.join().expect("shutdown thread returns its drain"));
    second_completion
        .join()
        .expect("second worker thread does not panic")
        .expect("second worker terminalizes");
    assert_eq!(poll_once(drain.as_mut()), Poll::Pending);

    first_work
        .finish_failed_before_commit(EnvironmentStatusCode::ShutdownInProgress)
        .expect("first worker terminalizes last");
    assert_eq!(poll_once(drain.as_mut()), Poll::Ready(()));
}
