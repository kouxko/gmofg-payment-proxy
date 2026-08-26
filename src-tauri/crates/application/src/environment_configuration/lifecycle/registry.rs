use std::sync::Arc;

use super::{
    snapshot::{EnvironmentCandidatePublicSnapshot, EnvironmentValidationLayerResult},
    state::{
        CandidateEntry, EnvironmentApplyWork, PrivateCandidateMaterial, RegistryShared,
        RegistryState, TokenState,
    },
    types::{
        EnvironmentApplyQueuedResult, EnvironmentApplyTaskId, EnvironmentCancelResult,
        EnvironmentCancelStatus, EnvironmentCandidateCreateResult, EnvironmentCandidateEpoch,
        EnvironmentCandidateId, EnvironmentCandidateLifecycleError, EnvironmentCandidatePolicy,
        EnvironmentCandidateStatus, EnvironmentCandidateStatusResult, EnvironmentConfirmationToken,
    },
};
use crate::environment_configuration::{
    EnvironmentConfigurationCandidateV1, EnvironmentDiagnostic, EnvironmentStatusCode,
    EnvironmentTerminalResult,
};

pub struct EnvironmentCandidateRegistry {
    pub(super) shared: Arc<RegistryShared>,
}

impl Default for EnvironmentCandidateRegistry {
    fn default() -> Self {
        Self::new(EnvironmentCandidatePolicy::default())
    }
}

impl std::fmt::Debug for EnvironmentCandidateRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentCandidateRegistry")
            .field("policy", &self.shared.policy)
            .field("metrics", &self.metrics())
            .finish_non_exhaustive()
    }
}

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

    pub fn insert_validating(
        &self,
        candidate: EnvironmentConfigurationCandidateV1,
        epoch: EnvironmentCandidateEpoch,
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

    pub(crate) fn complete_preview_ready(
        &self,
        candidate_id: &EnvironmentCandidateId,
        snapshot: EnvironmentCandidatePublicSnapshot,
    ) -> Result<EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
        let snapshot_target = snapshot.admitted_target();
        let mut state = self.shared.state.lock();
        if state.shutting_down {
            return Err(EnvironmentCandidateLifecycleError::ShutdownInProgress);
        }
        let entry = state
            .candidates
            .get_mut(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        if entry.status != EnvironmentCandidateStatus::Validating {
            return Err(EnvironmentCandidateLifecycleError::InvalidState);
        }
        if entry.admitted_target != snapshot_target {
            return Err(EnvironmentCandidateLifecycleError::ValidatedTargetMismatch);
        }
        let (target_key, baseline_public, validation_layers, preview) = snapshot.into_parts();
        let token = EnvironmentConfirmationToken::generate();
        entry.status = EnvironmentCandidateStatus::PreviewReady;
        entry.token = Some(TokenState::Available(token.clone()));
        entry.target_key = Some(target_key.clone());
        entry.baseline_public = Some(baseline_public.clone());
        entry.validation_layers.clone_from(&validation_layers);
        entry.preview = Some(preview.clone());
        Ok(EnvironmentCandidateCreateResult {
            candidate_id: entry.id.clone(),
            confirmation_token: Some(token),
            status: entry.status,
            target_key: Some(target_key),
            baseline_public: Some(baseline_public),
            validation_layers,
            preview: Some(preview),
            expires_on: "app_exit_or_invalidation",
            errors: Vec::new(),
        })
    }

    pub(crate) fn complete_validation_failed(
        &self,
        candidate_id: &EnvironmentCandidateId,
        validation_layers: Vec<EnvironmentValidationLayerResult>,
        diagnostics: Vec<EnvironmentDiagnostic>,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        let mut state = self.shared.state.lock();
        let entry = state
            .candidates
            .get(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        if entry.status != EnvironmentCandidateStatus::Validating {
            return Err(EnvironmentCandidateLifecycleError::InvalidState);
        }
        self.shared.publish_terminal(
            &mut state,
            candidate_id,
            EnvironmentCandidateStatus::ValidationFailed,
            EnvironmentTerminalResult::validation_failed(
                EnvironmentStatusCode::ValidationLayerFailed,
            ),
            Some(validation_layers),
            Some(diagnostics),
        )
    }

    pub fn queue_apply(
        &self,
        candidate_id: &EnvironmentCandidateId,
        confirmation_token: &EnvironmentConfirmationToken,
    ) -> Result<EnvironmentApplyQueuedResult, EnvironmentCandidateLifecycleError> {
        let mut state = self.shared.state.lock();
        if state.shutting_down {
            return Err(EnvironmentCandidateLifecycleError::ShutdownInProgress);
        }
        let entry = state
            .candidates
            .get(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        if matches!(entry.token, Some(TokenState::Consumed)) {
            return Err(EnvironmentCandidateLifecycleError::TokenConsumed);
        }
        if entry.status != EnvironmentCandidateStatus::PreviewReady {
            return Err(EnvironmentCandidateLifecycleError::InvalidState);
        }
        match entry.token.as_ref() {
            None => return Err(EnvironmentCandidateLifecycleError::ConfirmationTokenMissing),
            Some(TokenState::Consumed) => {
                return Err(EnvironmentCandidateLifecycleError::TokenConsumed);
            }
            Some(TokenState::Available(actual)) if actual != confirmation_token => {
                return Err(EnvironmentCandidateLifecycleError::ConfirmationTokenInvalid);
            }
            Some(TokenState::Available(_)) => {}
        }
        if state
            .candidates
            .values()
            .filter(|entry| entry.status.is_apply_active())
            .count()
            >= self.shared.policy.global_apply_capacity
        {
            return Err(EnvironmentCandidateLifecycleError::ApplyAlreadyActive);
        }
        let target_identity = entry.internal_target_identity.clone();
        if state
            .candidates
            .values()
            .filter(|entry| {
                entry.status.is_apply_active() && entry.internal_target_identity == target_identity
            })
            .count()
            >= self.shared.policy.target_apply_capacity
        {
            return Err(EnvironmentCandidateLifecycleError::ApplyAlreadyActive);
        }

        let apply_task_id = EnvironmentApplyTaskId::generate();
        let entry = state
            .candidates
            .get_mut(candidate_id)
            .expect("candidate was inspected under the same lock");
        entry.status = EnvironmentCandidateStatus::ApplyQueued;
        entry.token = Some(TokenState::Consumed);
        entry.apply_task_id = Some(apply_task_id.clone());
        state.apply_queue.push_back(candidate_id.clone());
        Ok(EnvironmentApplyQueuedResult {
            candidate_id: candidate_id.clone(),
            apply_task_id,
            status: EnvironmentCandidateStatus::ApplyQueued,
            errors: Vec::new(),
        })
    }

    pub(crate) fn claim_next_apply(
        &self,
    ) -> Result<Option<EnvironmentApplyWork>, EnvironmentCandidateLifecycleError> {
        let mut state = self.shared.state.lock();
        while let Some(candidate_id) = state.apply_queue.pop_front() {
            let Some(entry) = state.candidates.get_mut(&candidate_id) else {
                continue;
            };
            if entry.status != EnvironmentCandidateStatus::ApplyQueued {
                continue;
            }
            let material = entry
                .material
                .take()
                .ok_or(EnvironmentCandidateLifecycleError::InvalidState)?;
            let apply_task_id = entry
                .apply_task_id
                .clone()
                .ok_or(EnvironmentCandidateLifecycleError::InvalidState)?;
            entry.status = EnvironmentCandidateStatus::ApplyInProgress;
            return Ok(Some(EnvironmentApplyWork {
                shared: Arc::clone(&self.shared),
                candidate_id,
                apply_task_id,
                epoch: entry.epoch,
                material: Some(material),
                completed: false,
            }));
        }
        Ok(None)
    }

    pub(crate) fn invalidate_if_epoch_changed(
        &self,
        candidate_id: &EnvironmentCandidateId,
        current_epoch: EnvironmentCandidateEpoch,
    ) -> bool {
        let mut state = self.shared.state.lock();
        let invalid = state.candidates.get(candidate_id).is_some_and(|entry| {
            entry.status == EnvironmentCandidateStatus::PreviewReady && entry.epoch != current_epoch
        });
        if invalid {
            self.shared
                .publish_terminal(
                    &mut state,
                    candidate_id,
                    EnvironmentCandidateStatus::Stale,
                    EnvironmentTerminalResult::stale(),
                    None,
                    None,
                )
                .expect("typed stale terminal projection must serialize");
        }
        invalid
    }

    pub fn cancel(&self, candidate_id: &EnvironmentCandidateId) -> EnvironmentCancelResult {
        let mut state = self.shared.state.lock();
        let Some(status) = state.candidates.get(candidate_id).map(|entry| entry.status) else {
            return EnvironmentCancelResult::new(
                candidate_id.clone(),
                EnvironmentCancelStatus::NotFoundOrTerminal,
                false,
            );
        };
        match status {
            EnvironmentCandidateStatus::Validating
            | EnvironmentCandidateStatus::PreviewReady
            | EnvironmentCandidateStatus::ApplyQueued => {
                self.shared
                    .publish_terminal(
                        &mut state,
                        candidate_id,
                        EnvironmentCandidateStatus::Cancelled,
                        EnvironmentTerminalResult::cancelled(),
                        None,
                        None,
                    )
                    .expect("typed cancelled terminal projection must serialize");
                EnvironmentCancelResult::new(
                    candidate_id.clone(),
                    EnvironmentCancelStatus::Cancelled,
                    true,
                )
            }
            EnvironmentCandidateStatus::ApplyInProgress => EnvironmentCancelResult::new(
                candidate_id.clone(),
                EnvironmentCancelStatus::ApplyInProgressNotCancellable,
                false,
            ),
            _ => EnvironmentCancelResult::new(
                candidate_id.clone(),
                EnvironmentCancelStatus::NotFoundOrTerminal,
                true,
            ),
        }
    }
}

impl RegistryShared {
    pub(super) fn finish_guard(
        &self,
        candidate_id: &EnvironmentCandidateId,
        apply_task_id: &EnvironmentApplyTaskId,
        status: EnvironmentCandidateStatus,
        result: EnvironmentTerminalResult,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        let mut state = self.state.lock();
        let entry = state
            .candidates
            .get(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        if entry.status != EnvironmentCandidateStatus::ApplyInProgress
            || entry.apply_task_id.as_ref() != Some(apply_task_id)
        {
            return Err(EnvironmentCandidateLifecycleError::InvalidState);
        }
        self.publish_terminal(&mut state, candidate_id, status, result, None, None)?;
        let draining = state
            .candidates
            .values()
            .filter(|entry| entry.status == EnvironmentCandidateStatus::ApplyInProgress)
            .count();
        self.drain_count.send_replace(draining);
        drop(state);
        Ok(())
    }

    pub(super) fn publish_terminal(
        &self,
        state: &mut RegistryState,
        candidate_id: &EnvironmentCandidateId,
        status: EnvironmentCandidateStatus,
        terminal_result: EnvironmentTerminalResult,
        validation_layers: Option<Vec<EnvironmentValidationLayerResult>>,
        errors: Option<Vec<EnvironmentDiagnostic>>,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        let entry = state
            .candidates
            .get(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        let projection = EnvironmentCandidateStatusResult {
            candidate_id: entry.id.clone(),
            status,
            target_key: entry.target_key.clone(),
            baseline_public: entry.baseline_public.clone(),
            validation_layers: validation_layers.unwrap_or_else(|| entry.validation_layers.clone()),
            preview: entry.preview.clone(),
            terminal_result: Some(terminal_result.clone()),
            errors: errors.unwrap_or_else(|| entry.errors.clone()),
        };
        let public_bytes = serde_json::to_vec(&projection)
            .map_err(|_| EnvironmentCandidateLifecycleError::TerminalProjectionEncodingFailed)?
            .len();

        let entry = state
            .candidates
            .get_mut(candidate_id)
            .expect("candidate was checked under the same lock");
        entry.status = status;
        entry.material = None;
        entry.private_bytes = 0;
        if !matches!(entry.token, Some(TokenState::Consumed)) {
            entry.token = None;
        }
        entry.apply_task_id = None;
        entry.validation_layers = projection.validation_layers;
        entry.terminal_result = Some(terminal_result);
        entry.errors = projection.errors;
        entry.terminal_public_bytes = public_bytes;
        state.terminal_public_bytes += public_bytes;
        state.terminal_order.push_back(candidate_id.clone());

        while state.terminal_order.len() > self.policy.retained_terminal_count
            || state.terminal_public_bytes > self.policy.retained_terminal_bytes
        {
            let Some(oldest) = state.terminal_order.pop_front() else {
                break;
            };
            if let Some(evicted) = state.candidates.remove(&oldest) {
                state.terminal_public_bytes -= evicted.terminal_public_bytes;
            }
        }
        Ok(())
    }
}
