use std::sync::Arc;

use super::{EnvironmentCandidateLifecycleError, EnvironmentCandidateRegistry};
use crate::environment_configuration::{
    EnvironmentApplyLeaseOutcome, EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest,
    EnvironmentCommitFailure, EnvironmentCommitPort, EnvironmentCommitReceipt,
    EnvironmentCommitRollbackOutcome, EnvironmentProtectedMaterialPreparePort,
    EnvironmentSelectionPolicy, EnvironmentStatusCode,
};
use crate::{EventHub, UiEventPayload};

pub(crate) struct EnvironmentApplyWorker {
    registry: EnvironmentCandidateRegistry,
    mutation_gate: Arc<crate::facade::ApplicationMutationGate>,
    lease: Arc<dyn EnvironmentApplyLeasePort>,
    prepare: Arc<dyn EnvironmentProtectedMaterialPreparePort>,
    commit: Arc<dyn EnvironmentCommitPort>,
    events: Arc<EventHub>,
}

enum BeforeCommitPhase<T> {
    Ready(T),
    Terminalized,
}

fn map_before_commit<T, E>(result: Result<T, E>) -> BeforeCommitPhase<T> {
    match result {
        Ok(value) => BeforeCommitPhase::Ready(value),
        Err(_) => BeforeCommitPhase::Terminalized,
    }
}

impl EnvironmentApplyWorker {
    #[cfg(test)]
    pub(crate) fn new(
        registry: EnvironmentCandidateRegistry,
        lease: Arc<dyn EnvironmentApplyLeasePort>,
        prepare: Arc<dyn EnvironmentProtectedMaterialPreparePort>,
        commit: Arc<dyn EnvironmentCommitPort>,
        events: Arc<EventHub>,
    ) -> Self {
        Self::new_with_mutation_gate(
            registry,
            Arc::new(crate::facade::ApplicationMutationGate::default()),
            lease,
            prepare,
            commit,
            events,
        )
    }

    pub(crate) fn new_with_mutation_gate(
        registry: EnvironmentCandidateRegistry,
        mutation_gate: Arc<crate::facade::ApplicationMutationGate>,
        lease: Arc<dyn EnvironmentApplyLeasePort>,
        prepare: Arc<dyn EnvironmentProtectedMaterialPreparePort>,
        commit: Arc<dyn EnvironmentCommitPort>,
        events: Arc<EventHub>,
    ) -> Self {
        Self {
            registry,
            mutation_gate,
            lease,
            prepare,
            commit,
            events,
        }
    }

    /// The spawned task owns all work after dequeue. Dropping the caller's `JoinHandle` does not
    /// cancel this task; Application shutdown observes the registry drain instead.
    pub(crate) fn spawn_once(self) {
        tokio::spawn(async move {
            if let Err(error) = self.run_once().await {
                tracing::error!(%error, "environment apply worker terminated unexpectedly");
            }
        });
    }

    async fn run_once(self) -> Result<bool, EnvironmentCandidateLifecycleError> {
        let Some(mut work) = self.registry.claim_next_apply()? else {
            return Ok(false);
        };
        let mutation_gate = Arc::clone(&self.mutation_gate);
        let mutation_guard = mutation_gate.lock().await;
        let result = async {
            let validated_baseline = work.validated_apply_baseline().clone();
            let expected = validated_baseline.generations().clone();
            let lease_result = self
                .lease
                .acquire(EnvironmentApplyLeaseRequest {
                    candidate_id: work.candidate_id().clone(),
                    candidate_epoch: work.epoch(),
                    expected: expected.clone(),
                    validated_baseline,
                })
                .await;
            let BeforeCommitPhase::Ready(lease) = map_before_commit(lease_result) else {
                work.finish_failed_before_commit(EnvironmentStatusCode::ApplyLeaseUnavailable)?;
                return Ok(true);
            };

            match lease.outcome() {
                EnvironmentApplyLeaseOutcome::Acquired => {}
                EnvironmentApplyLeaseOutcome::PackageStale => {
                    work.finish_stale(EnvironmentStatusCode::CandidateStale)?;
                    return Ok(true);
                }
                EnvironmentApplyLeaseOutcome::GenerationMismatch => {
                    work.finish_failed_before_commit(EnvironmentStatusCode::ApplyLeaseMismatch)?;
                    return Ok(true);
                }
                EnvironmentApplyLeaseOutcome::RuntimeActive => {
                    work.finish_failed_before_commit(EnvironmentStatusCode::RuntimeActive)?;
                    return Ok(true);
                }
                EnvironmentApplyLeaseOutcome::AndroidOwnerMismatch => {
                    work.finish_failed_before_commit(
                        EnvironmentStatusCode::AndroidRuntimeOwnerActive,
                    )?;
                    return Ok(true);
                }
            }

            let staged = work.take_staged_material()?;
            let prepare_result = self.prepare.prepare(staged).await;
            let BeforeCommitPhase::Ready(prepared) = map_before_commit(prepare_result) else {
                work.finish_failed_before_commit(
                    EnvironmentStatusCode::ProtectedMaterialPrepareFailed,
                )?;
                return Ok(true);
            };
            let request = prepared.into_commit_request(
                lease.observed().clone(),
                EnvironmentSelectionPolicy::PreserveExistingSelectionOrSelectNewWhenNone,
            );
            let result = match self.commit.commit(request).await {
                Ok(result) => result,
                Err(EnvironmentCommitFailure::BeforeTransaction(_)) => {
                    work.finish_failed_before_commit(EnvironmentStatusCode::CommitFailed)?;
                    return Ok(true);
                }
                Err(failure @ EnvironmentCommitFailure::RolledBack { .. }) => {
                    let status = match failure.rollback_outcome() {
                        Some(EnvironmentCommitRollbackOutcome::BaselineMismatch) => {
                            EnvironmentStatusCode::CommitBaselineMismatch
                        }
                        Some(EnvironmentCommitRollbackOutcome::Failed) | None => {
                            EnvironmentStatusCode::CommitRolledBack
                        }
                    };
                    work.finish_rolled_back(status)?;
                    return Ok(true);
                }
            };
            self.events.publish(
                None,
                chrono::Utc::now(),
                Some(result.workspace_id.to_string()),
                Some(result.revision),
                UiEventPayload::SnapshotRequired {
                    reason: "environment_configuration_committed".into(),
                },
            );
            let receipt =
                EnvironmentCommitReceipt::after_commit(result, work.apply_task_id().clone());
            work.finish_committed(receipt)?;
            drop(lease);
            Ok(true)
        }
        .await;
        drop(mutation_guard);
        result
    }
}
