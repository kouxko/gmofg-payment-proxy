use std::future::Future;

use super::{
    registry::EnvironmentCandidateRegistry, types::EnvironmentCandidateStatus,
    validation::signal_validation_cancellation,
};
use crate::environment_configuration::{EnvironmentStatusCode, EnvironmentTerminalResult};

impl EnvironmentCandidateRegistry {
    pub(crate) fn begin_shutdown(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.begin_shutdown_inner(|| {})
    }

    #[cfg(test)]
    pub(crate) fn begin_shutdown_with_barrier(
        &self,
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.begin_shutdown_inner(move || {
            entered
                .send(())
                .expect("shutdown barrier observer remains available");
            release
                .recv()
                .expect("shutdown barrier controller releases the registry lock");
        })
    }

    fn begin_shutdown_inner<F>(
        &self,
        before_publish: F,
    ) -> impl Future<Output = ()> + Send + 'static + use<F>
    where
        F: FnOnce(),
    {
        let mut state = self.shared.state.lock();
        state.shutting_down = true;
        let cancellable = state
            .candidates
            .iter()
            .filter(|(_, entry)| {
                matches!(
                    entry.status,
                    EnvironmentCandidateStatus::Validating
                        | EnvironmentCandidateStatus::PreviewReady
                        | EnvironmentCandidateStatus::ApplyQueued
                )
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for candidate_id in cancellable {
            signal_validation_cancellation(
                &state.candidates[&candidate_id].validation_cancellation,
                EnvironmentStatusCode::CandidateCancelledByShutdown,
            );
            self.shared
                .publish_terminal(
                    &mut state,
                    &candidate_id,
                    EnvironmentCandidateStatus::CancelledByShutdown,
                    EnvironmentTerminalResult::cancelled_by_shutdown(),
                    None,
                    None,
                )
                .expect("typed shutdown terminal projection must serialize");
        }
        let draining = state
            .candidates
            .values()
            .filter(|entry| entry.status == EnvironmentCandidateStatus::ApplyInProgress)
            .count();
        before_publish();
        self.shared.drain_count.send_replace(draining);
        let mut receiver = self.shared.drain_count.subscribe();
        drop(state);
        async move {
            while *receiver.borrow_and_update() != 0 {
                if receiver.changed().await.is_err() {
                    return;
                }
            }
        }
    }
}
