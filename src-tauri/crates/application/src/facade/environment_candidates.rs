#![expect(
    dead_code,
    reason = "G034 seals mutation capabilities inside Application until G035 validation orchestration"
)]

use std::future::Future;

use super::Application;
use crate::environment_configuration::{
    EnvironmentApplyQueuedResult, EnvironmentApplyWork, EnvironmentCandidateCreateResult,
    EnvironmentCandidateEpoch, EnvironmentCandidateId, EnvironmentCandidateLifecycleError,
    EnvironmentCandidateMetrics, EnvironmentCandidatePublicSnapshot,
    EnvironmentCandidateStatusResult, EnvironmentConfigurationCandidateV1,
    EnvironmentConfirmationToken, EnvironmentDiagnostic, EnvironmentValidationLayerResult,
};

impl Application {
    pub fn environment_candidate_status(
        &self,
        candidate_id: &EnvironmentCandidateId,
    ) -> EnvironmentCandidateStatusResult {
        self.environment_candidates.status(candidate_id)
    }

    pub fn environment_candidate_metrics(&self) -> EnvironmentCandidateMetrics {
        self.environment_candidates.metrics()
    }

    pub(crate) fn environment_candidate_insert_validating(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        epoch: EnvironmentCandidateEpoch,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.environment_candidates
            .insert_validating(candidate, epoch)
    }

    pub(crate) fn environment_candidate_complete_preview_ready(
        &self,
        candidate_id: &EnvironmentCandidateId,
        snapshot: EnvironmentCandidatePublicSnapshot,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.environment_candidates
            .complete_preview_ready(candidate_id, snapshot)
    }

    pub(crate) fn environment_candidate_snapshot_from_validated_json(
        bytes: &[u8],
    ) -> Result<EnvironmentCandidatePublicSnapshot, serde_json::Error> {
        EnvironmentCandidatePublicSnapshot::from_validated_json(bytes)
    }

    pub(crate) fn environment_candidate_complete_validation_failed(
        &self,
        candidate_id: &EnvironmentCandidateId,
        validation_layers: Vec<EnvironmentValidationLayerResult>,
        diagnostics: Vec<EnvironmentDiagnostic>,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        self.environment_candidates.complete_validation_failed(
            candidate_id,
            validation_layers,
            diagnostics,
        )
    }

    pub(crate) fn environment_candidate_queue_apply(
        &self,
        candidate_id: &EnvironmentCandidateId,
        confirmation_token: &EnvironmentConfirmationToken,
    ) -> Result<EnvironmentApplyQueuedResult, EnvironmentCandidateLifecycleError> {
        self.environment_candidates
            .queue_apply(candidate_id, confirmation_token)
    }

    pub(crate) fn environment_candidate_cancel(
        &self,
        candidate_id: &EnvironmentCandidateId,
    ) -> crate::EnvironmentCancelResult {
        self.environment_candidates.cancel(candidate_id)
    }

    pub(crate) fn environment_candidate_claim_next_apply(
        &self,
    ) -> Result<Option<EnvironmentApplyWork>, EnvironmentCandidateLifecycleError> {
        self.environment_candidates.claim_next_apply()
    }

    pub(crate) fn environment_candidate_invalidate_if_epoch_changed(
        &self,
        candidate_id: &EnvironmentCandidateId,
        current_epoch: EnvironmentCandidateEpoch,
    ) -> bool {
        self.environment_candidates
            .invalidate_if_epoch_changed(candidate_id, current_epoch)
    }

    pub(crate) fn environment_candidate_begin_shutdown(
        &self,
    ) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.environment_candidates.begin_shutdown()
    }
}
