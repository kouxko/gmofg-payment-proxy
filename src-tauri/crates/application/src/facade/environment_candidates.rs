use super::Application;
#[cfg(test)]
use crate::environment_configuration::EnvironmentCandidateEpoch;
use crate::{
    EnvironmentApplyQueuedResult, EnvironmentCancelResult, EnvironmentCandidateCreateResult,
    EnvironmentCandidateId, EnvironmentCandidateLifecycleError, EnvironmentCandidateMetrics,
    EnvironmentCandidateStatusResult, EnvironmentConfigurationCandidateV1,
    EnvironmentConfirmationToken,
};
#[cfg(test)]
use std::future::Future;

impl Application {
    pub async fn environment_candidate_create(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.environment
            .environment_candidate_create(candidate, cancellation)
            .await
    }
    pub fn environment_candidate_status(
        &self,
        id: &EnvironmentCandidateId,
    ) -> EnvironmentCandidateStatusResult {
        self.environment.environment_candidate_status(id)
    }
    pub fn environment_candidate_metrics(&self) -> EnvironmentCandidateMetrics {
        self.environment.environment_candidate_metrics()
    }
    pub fn environment_candidate_queue_and_start_apply(
        &self,
        id: &EnvironmentCandidateId,
        token: &EnvironmentConfirmationToken,
    ) -> Result<EnvironmentApplyQueuedResult, EnvironmentCandidateLifecycleError> {
        self.environment
            .environment_candidate_queue_and_start_apply(id, token)
    }
    pub fn environment_candidate_cancel(
        &self,
        id: &EnvironmentCandidateId,
    ) -> EnvironmentCancelResult {
        self.environment.environment_candidate_cancel(id)
    }
    pub async fn environment_candidate_shutdown_and_drain(&self) {
        self.environment
            .environment_candidate_shutdown_and_drain()
            .await;
    }
    #[cfg(test)]
    pub(crate) fn environment_candidate_insert_validating(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        epoch: EnvironmentCandidateEpoch,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.environment
            .environment_candidate_insert_validating(candidate, epoch)
    }
    #[cfg(test)]
    pub(crate) fn environment_candidate_begin_shutdown(
        &self,
    ) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.environment.environment_candidate_begin_shutdown()
    }
    #[cfg(test)]
    pub(crate) fn environment_candidate_finish_validation(
        &self,
        id: &EnvironmentCandidateId,
        report: crate::EnvironmentValidationReport,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        self.environment
            .environment_candidate_finish_validation(id, report)
    }
    #[cfg(test)]
    pub(crate) async fn environment_candidate_complete_preview_ready(
        &self,
        id: &EnvironmentCandidateId,
        snapshot: crate::EnvironmentCandidatePublicSnapshot,
        workspace: crate::ProxyWorkspace,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.environment
            .environment_candidate_complete_preview_ready(id, snapshot, workspace)
            .await
    }
    #[cfg(test)]
    pub(crate) fn environment_configuration_service(
        &self,
    ) -> &crate::environment_configuration::EnvironmentConfigurationService {
        &self.environment
    }
}
