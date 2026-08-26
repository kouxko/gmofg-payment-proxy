use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::Arc,
};

use parking_lot::Mutex;
use tokio::sync::watch;
use zeroize::Zeroizing;

use super::{
    snapshot::{
        EnvironmentBaselinePublic, EnvironmentCandidatePreview, EnvironmentValidationLayerResult,
    },
    types::{
        EnvironmentApplyTaskId, EnvironmentCandidateEpoch, EnvironmentCandidateId,
        EnvironmentCandidateLifecycleError, EnvironmentCandidatePolicy, EnvironmentCandidateStatus,
        EnvironmentCandidateStatusResult, EnvironmentConfirmationToken,
    },
};
use crate::environment_configuration::{
    EnvironmentAdmittedTarget, EnvironmentConfigurationCandidateV1, EnvironmentDiagnostic,
    EnvironmentStatusCode, EnvironmentTerminalResult,
};

pub(super) struct PrivateCandidateMaterial {
    encoded_candidate: Zeroizing<Vec<u8>>,
}

impl PrivateCandidateMaterial {
    pub(super) fn new(
        candidate: &EnvironmentConfigurationCandidateV1,
    ) -> Result<Self, EnvironmentCandidateLifecycleError> {
        serde_json::to_vec(candidate)
            .map(Zeroizing::new)
            .map(|encoded_candidate| Self { encoded_candidate })
            .map_err(|_| EnvironmentCandidateLifecycleError::PrivateMaterialEncodingFailed)
    }

    pub(super) fn byte_len(&self) -> usize {
        self.encoded_candidate.len()
    }
}

pub(super) enum TokenState {
    Available(EnvironmentConfirmationToken),
    Consumed,
}

pub(super) struct CandidateEntry {
    pub(super) id: EnvironmentCandidateId,
    pub(super) internal_target_identity: String,
    pub(super) admitted_target: EnvironmentAdmittedTarget,
    pub(super) epoch: EnvironmentCandidateEpoch,
    pub(super) status: EnvironmentCandidateStatus,
    pub(super) material: Option<PrivateCandidateMaterial>,
    pub(super) private_bytes: usize,
    pub(super) token: Option<TokenState>,
    pub(super) apply_task_id: Option<EnvironmentApplyTaskId>,
    pub(super) target_key: Option<String>,
    pub(super) baseline_public: Option<EnvironmentBaselinePublic>,
    pub(super) validation_layers: Vec<EnvironmentValidationLayerResult>,
    pub(super) preview: Option<EnvironmentCandidatePreview>,
    pub(super) terminal_result: Option<EnvironmentTerminalResult>,
    pub(super) errors: Vec<EnvironmentDiagnostic>,
    pub(super) terminal_public_bytes: usize,
}

impl CandidateEntry {
    pub(super) fn public_status(&self) -> EnvironmentCandidateStatusResult {
        EnvironmentCandidateStatusResult {
            candidate_id: self.id.clone(),
            status: self.status,
            target_key: self.target_key.clone(),
            baseline_public: self.baseline_public.clone(),
            validation_layers: self.validation_layers.clone(),
            preview: self.preview.clone(),
            terminal_result: self.terminal_result.clone(),
            errors: self.errors.clone(),
        }
    }
}

#[derive(Default)]
pub(super) struct RegistryState {
    pub(super) candidates: HashMap<EnvironmentCandidateId, CandidateEntry>,
    pub(super) apply_queue: VecDeque<EnvironmentCandidateId>,
    pub(super) terminal_order: VecDeque<EnvironmentCandidateId>,
    pub(super) terminal_public_bytes: usize,
    pub(super) shutting_down: bool,
}

pub(super) struct RegistryShared {
    pub(super) policy: EnvironmentCandidatePolicy,
    pub(super) state: Mutex<RegistryState>,
    pub(super) drain_count: watch::Sender<usize>,
}

pub struct EnvironmentApplyWork {
    pub(super) shared: Arc<RegistryShared>,
    pub(super) candidate_id: EnvironmentCandidateId,
    pub(super) apply_task_id: EnvironmentApplyTaskId,
    pub(super) epoch: EnvironmentCandidateEpoch,
    pub(super) material: Option<PrivateCandidateMaterial>,
    pub(super) completed: bool,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "G034 owns apply work; G038 will consume its sealed completion methods"
    )
)]
impl EnvironmentApplyWork {
    pub fn candidate_id(&self) -> &EnvironmentCandidateId {
        &self.candidate_id
    }
    pub fn apply_task_id(&self) -> &EnvironmentApplyTaskId {
        &self.apply_task_id
    }
    pub const fn epoch(&self) -> EnvironmentCandidateEpoch {
        self.epoch
    }

    pub fn finish_failed_before_commit(
        mut self,
        status_code: EnvironmentStatusCode,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        self.finish(
            EnvironmentCandidateStatus::FailedBeforeCommit,
            EnvironmentTerminalResult::failed_before_commit(status_code),
        )
    }

    pub fn finish_rolled_back(
        mut self,
        status_code: EnvironmentStatusCode,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        self.finish(
            EnvironmentCandidateStatus::RolledBack,
            EnvironmentTerminalResult::rolled_back(status_code),
        )
    }

    fn finish(
        &mut self,
        status: EnvironmentCandidateStatus,
        result: EnvironmentTerminalResult,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        self.shared
            .finish_guard(&self.candidate_id, &self.apply_task_id, status, result)?;
        self.material = None;
        self.completed = true;
        Ok(())
    }
}

impl Drop for EnvironmentApplyWork {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let outcome = self.shared.finish_guard(
            &self.candidate_id,
            &self.apply_task_id,
            EnvironmentCandidateStatus::FailedBeforeCommit,
            EnvironmentTerminalResult::failed_before_commit(EnvironmentStatusCode::CommitFailed),
        );
        debug_assert!(
            outcome.is_ok(),
            "typed fallback terminalization must succeed"
        );
        self.material = None;
        self.completed = true;
    }
}

impl fmt::Debug for EnvironmentApplyWork {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentApplyWork")
            .field("candidate_id", &self.candidate_id)
            .field("apply_task_id", &self.apply_task_id)
            .field("epoch", &self.epoch())
            .field("private_material", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
