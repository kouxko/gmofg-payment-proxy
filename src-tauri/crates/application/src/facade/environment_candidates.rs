#[cfg(test)]
use std::future::Future;

use async_trait::async_trait;

use super::Application;
use crate::ProxyWorkspace;
#[cfg(test)]
use crate::environment_configuration::EnvironmentCandidateEpoch;
use crate::environment_configuration::EnvironmentCandidateValidator;
use crate::environment_configuration::EnvironmentValidationReport;
use crate::environment_configuration::candidate_preview_snapshot;
use crate::environment_configuration::{
    EnvironmentAdmittedTarget, EnvironmentDomainProjectionPort, EnvironmentPreviewBaselinePort,
    EnvironmentPreviewBaselineRequest, EnvironmentProjectedCandidate,
    EnvironmentValidationCheckpoint,
};
use crate::environment_configuration::{
    EnvironmentCandidateCreateResult, EnvironmentCandidateId, EnvironmentCandidateLifecycleError,
    EnvironmentCandidateMetrics, EnvironmentCandidatePublicSnapshot,
    EnvironmentCandidateStatusResult, EnvironmentConfigurationCandidateV1,
};
use crate::{EnvironmentApplyBaselineCaptureRequest, EnvironmentCommitTarget, WorkspaceId};
use crate::{EnvironmentApplyQueuedResult, EnvironmentConfirmationToken};

impl Application {
    pub async fn environment_candidate_create(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        request_cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        let candidate_json = zeroize::Zeroizing::new(
            serde_json::to_vec(&candidate)
                .map_err(|_| EnvironmentCandidateLifecycleError::PrivateMaterialEncodingFailed)?,
        );
        let inserted = self
            .environment_candidates
            .insert_validating_owned(candidate)?;
        let candidate_id = inserted.candidate_id().clone();
        let validation = self
            .environment_candidate_run_validation(
                &candidate_id,
                &candidate_json,
                request_cancellation.clone(),
            )
            .await;
        if validation.is_err() || request_cancellation.is_cancelled() {
            let _ = self.environment_candidates.cancel(&candidate_id);
        }
        validation?;
        let result = self.environment_candidates.create_result(&candidate_id)?;
        if !request_cancellation.is_cancelled() {
            return Ok(result);
        }
        let _ = self.environment_candidates.cancel(&candidate_id);
        self.environment_candidates.create_result(&candidate_id)
    }

    pub(crate) async fn environment_candidate_run_validation(
        &self,
        candidate_id: &EnvironmentCandidateId,
        candidate_json: &[u8],
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<EnvironmentValidationReport, EnvironmentCandidateLifecycleError> {
        let candidate_cancellation = self
            .environment_candidates
            .validation_cancellation(candidate_id)?;
        let report = EnvironmentCandidateValidator::new(self.environment_validator.clone())
            .validate_for_candidate(
                candidate_id,
                candidate_json,
                cancellation,
                candidate_cancellation,
                self,
            )
            .await;
        self.environment_candidate_finish_validation(candidate_id, report.clone())?;
        Ok(report)
    }

    pub(crate) fn environment_candidate_finish_validation(
        &self,
        candidate_id: &EnvironmentCandidateId,
        report: EnvironmentValidationReport,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        match report.status_code() {
            None | Some(crate::EnvironmentStatusCode::CandidateCancelledByShutdown) => Ok(()),
            Some(crate::EnvironmentStatusCode::CandidateCancelled) => {
                let _ = self.environment_candidates.cancel(candidate_id);
                Ok(())
            }
            Some(_) => self
                .environment_candidates
                .complete_validation_report_failed(candidate_id, report),
        }
    }
    pub(crate) fn start_next_environment_apply(&self) {
        crate::environment_configuration::EnvironmentApplyWorker::new_with_mutation_gate(
            self.environment_candidates.clone(),
            self.mutation_gate.clone(),
            self.environment_apply_lease.clone(),
            self.environment_material_preparer.clone(),
            self.environment_commit.clone(),
        )
        .spawn_once();
    }

    pub fn environment_candidate_status(
        &self,
        candidate_id: &EnvironmentCandidateId,
    ) -> EnvironmentCandidateStatusResult {
        self.environment_candidates.status(candidate_id)
    }

    pub fn environment_candidate_metrics(&self) -> EnvironmentCandidateMetrics {
        self.environment_candidates.metrics()
    }

    #[cfg(test)]
    pub(crate) fn environment_candidate_insert_validating(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        epoch: EnvironmentCandidateEpoch,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.environment_candidates
            .insert_validating(candidate, epoch)
    }

    pub(crate) async fn environment_candidate_complete_preview_ready(
        &self,
        candidate_id: &EnvironmentCandidateId,
        snapshot: EnvironmentCandidatePublicSnapshot,
        workspace: ProxyWorkspace,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        let admitted_target = snapshot.admitted_target();
        let (target, persisted_workspace) = match admitted_target {
            EnvironmentAdmittedTarget::Existing {
                workspace_id,
                expected_revision,
            } => {
                let persisted = self
                    .workspaces
                    .get(WorkspaceId::from_uuid(workspace_id))
                    .await
                    .map_err(|_| EnvironmentCandidateLifecycleError::InvalidState)?;
                (
                    EnvironmentCommitTarget::Existing {
                        workspace_id,
                        expected_revision,
                    },
                    Some(persisted),
                )
            }
            EnvironmentAdmittedTarget::New { name } => (
                EnvironmentCommitTarget::New {
                    workspace_id: workspace.id.as_uuid(),
                    display_name: name,
                },
                None,
            ),
        };
        let environment_baseline_capture = self.environment_baseline_capture.clone();
        let capture_request = EnvironmentApplyBaselineCaptureRequest {
            target,
            persisted_workspace,
            candidate_workspace: workspace.clone(),
            schema_version: snapshot.schema_version(),
            validation_engine_version: snapshot.validation_engine_version(),
        };
        let captured = environment_baseline_capture.capture(capture_request).await;
        let baseline = captured.map_err(|_| EnvironmentCandidateLifecycleError::InvalidState)?;
        self.environment_candidates
            .attach_validated_apply_baseline(candidate_id, baseline)?;
        self.environment_candidates
            .complete_preview_ready(candidate_id, snapshot, workspace)
    }

    pub fn environment_candidate_queue_and_start_apply(
        &self,
        candidate_id: &EnvironmentCandidateId,
        confirmation_token: &EnvironmentConfirmationToken,
    ) -> Result<EnvironmentApplyQueuedResult, EnvironmentCandidateLifecycleError> {
        let queued = self
            .environment_candidates
            .queue_apply(candidate_id, confirmation_token)?;
        self.start_next_environment_apply();
        Ok(queued)
    }

    pub fn environment_candidate_cancel(
        &self,
        candidate_id: &EnvironmentCandidateId,
    ) -> crate::EnvironmentCancelResult {
        self.environment_candidates.cancel(candidate_id)
    }

    #[cfg(test)]
    pub(crate) fn environment_candidate_begin_shutdown(
        &self,
    ) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.environment_candidates.begin_shutdown()
    }

    /// Stops accepting environment apply work and waits for the application-owned worker to drain.
    pub async fn environment_candidate_shutdown_and_drain(&self) {
        self.environment_candidates.begin_shutdown().await;
    }
}

#[async_trait]
impl EnvironmentDomainProjectionPort for Application {
    async fn project_environment_candidate(
        &self,
        candidate: crate::EnvironmentConfigurationCandidateV1,
        checkpoint: &dyn EnvironmentValidationCheckpoint,
    ) -> crate::AppResult<EnvironmentProjectedCandidate> {
        let target = candidate.lifecycle_target();
        let workspace_snapshot = match target {
            EnvironmentAdmittedTarget::New { .. } => None,
            EnvironmentAdmittedTarget::Existing { .. } => Some(self.workspaces.snapshot().await?),
        };
        let persisted_workspace = match (&target, &workspace_snapshot) {
            (EnvironmentAdmittedTarget::New { .. }, _) => None,
            (EnvironmentAdmittedTarget::Existing { workspace_id, .. }, Some(snapshot)) => Some(
                find_target_workspace(&snapshot.details, *workspace_id, checkpoint)?,
            ),
            (EnvironmentAdmittedTarget::Existing { .. }, None) => unreachable!(),
        };
        let workspace_scope = workspace_snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.details.as_slice());
        if checkpoint.checkpoint() {
            return Err(validation_interrupted());
        }
        EnvironmentProjectedCandidate::project_with_workspace_scope_and_checkpoint(
            candidate,
            persisted_workspace,
            workspace_scope,
            self.environment_identity_allocator.port(),
            checkpoint,
        )
    }
}

fn validation_interrupted() -> crate::AppError {
    crate::AppError::new(
        "VALIDATION_INTERRUPTED",
        "environment validation interrupted",
    )
}

fn find_target_workspace<'a>(
    workspaces: &'a [crate::ProxyWorkspace],
    workspace_id: uuid::Uuid,
    checkpoint: &dyn EnvironmentValidationCheckpoint,
) -> crate::AppResult<&'a crate::ProxyWorkspace> {
    for workspace in workspaces {
        if checkpoint.checkpoint() {
            return Err(validation_interrupted());
        }
        if workspace.id.as_uuid() == workspace_id {
            return Ok(workspace);
        }
    }
    Err(crate::AppError::new(
        "VALIDATION_LAYER_FAILED",
        "target Workspace does not exist",
    ))
}

#[async_trait]
impl EnvironmentPreviewBaselinePort for Application {
    fn domain_projection_port(&self) -> Option<&dyn EnvironmentDomainProjectionPort> {
        Some(self)
    }

    async fn validate_preview_baseline(
        &self,
        request: EnvironmentPreviewBaselineRequest<'_>,
    ) -> crate::AppResult<()> {
        let projected = request.projected_candidate().ok_or_else(preview_failure)?;
        let candidate = projected.candidate();
        let target = candidate.lifecycle_target();
        match &target {
            EnvironmentAdmittedTarget::New { name } => {
                let collision = self
                    .workspaces
                    .list()
                    .await?
                    .into_iter()
                    .any(|workspace| workspace.name.trim() == name);
                if collision {
                    return Err(crate::AppError::new(
                        "WORKSPACE_NAME_COLLISION",
                        "Workspace name already exists",
                    ));
                }
            }
            EnvironmentAdmittedTarget::Existing { .. } => {}
        }
        let workspace = projected.workspace().clone();
        let snapshot = candidate_preview_snapshot(candidate, request.prior_layers(), &workspace)?;
        self.environment_candidate_complete_preview_ready(
            request.candidate_id(),
            snapshot,
            workspace,
        )
        .await
        .map_err(|_| preview_failure())?;
        Ok(())
    }
}

fn preview_failure() -> crate::AppError {
    crate::AppError::new(
        "VALIDATION_LAYER_FAILED",
        "environment preview validation failed",
    )
}
