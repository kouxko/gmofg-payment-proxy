use std::sync::Arc;

use super::super::{
    state::{CandidateEntry, PrivateCandidateMaterial, RegistryShared, RegistryState},
    types::{
        EnvironmentCandidateCreateResult, EnvironmentCandidateEpoch, EnvironmentCandidateId,
        EnvironmentCandidateLifecycleError, EnvironmentCandidatePolicy, EnvironmentCandidateStatus,
    },
};
use super::EnvironmentCandidateRegistry;
use crate::environment_configuration::EnvironmentConfigurationCandidateV1;

impl EnvironmentCandidateRegistry {
    pub fn new(policy: EnvironmentCandidatePolicy) -> Self {
        let (drain_count, _) = tokio::sync::watch::channel(0);
        Self {
            shared: Arc::new(RegistryShared {
                policy,
                state: parking_lot::Mutex::new(RegistryState::default()),
                drain_count,
            }),
        }
    }

    #[cfg(test)]
    pub fn insert_validating(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        epoch: EnvironmentCandidateEpoch,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.insert_validating_with(candidate, |_| Ok(epoch))
    }

    pub(crate) fn insert_validating_owned(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        self.insert_validating_with(candidate, |state| {
            state.next_candidate_epoch = state
                .next_candidate_epoch
                .checked_add(1)
                .ok_or(EnvironmentCandidateLifecycleError::CandidateEpochExhausted)?;
            Ok(EnvironmentCandidateEpoch::new(state.next_candidate_epoch))
        })
    }

    fn insert_validating_with(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        allocate_epoch: impl FnOnce(
            &mut RegistryState,
        ) -> Result<
            EnvironmentCandidateEpoch,
            EnvironmentCandidateLifecycleError,
        >,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        let admitted_target = candidate.lifecycle_target();
        let internal_target_identity = admitted_target.capacity_identity();
        let material = PrivateCandidateMaterial::new(&candidate)?;
        let private_bytes = material.byte_len();
        drop(candidate);
        let mut state = self.shared.state.lock();
        if state.shutting_down {
            return Err(EnvironmentCandidateLifecycleError::ShutdownInProgress);
        }
        if state
            .candidates
            .values()
            .filter(|entry| entry.status.is_active())
            .count()
            >= self.shared.policy.candidate_capacity
        {
            return Err(EnvironmentCandidateLifecycleError::CandidateCapacityExceeded);
        }
        if state
            .candidates
            .values()
            .filter(|entry| {
                entry.status.is_active()
                    && entry.internal_target_identity == internal_target_identity
            })
            .count()
            >= self.shared.policy.per_target_capacity
        {
            return Err(EnvironmentCandidateLifecycleError::TargetCandidateAlreadyActive);
        }
        let epoch = allocate_epoch(&mut state)?;

        let id = EnvironmentCandidateId::generate();
        state.candidates.insert(
            id.clone(),
            CandidateEntry {
                id: id.clone(),
                internal_target_identity,
                admitted_target,
                epoch,
                status: EnvironmentCandidateStatus::Validating,
                material: Some(material),
                private_bytes,
                token: None,
                apply_task_id: None,
                target_key: None,
                baseline_public: None,
                validation_layers: Vec::new(),
                preview: None,
                terminal_result: None,
                errors: Vec::new(),
                terminal_public_bytes: 0,
                validated_apply_baseline: None,
                validation_cancellation: tokio_util::sync::CancellationToken::new(),
            },
        );
        Ok(EnvironmentCandidateCreateResult {
            candidate_id: id,
            confirmation_token: None,
            status: EnvironmentCandidateStatus::Validating,
            target_key: None,
            baseline_public: None,
            validation_layers: Vec::new(),
            preview: None,
            expires_on: "app_exit_or_invalidation",
            errors: Vec::new(),
        })
    }
}
