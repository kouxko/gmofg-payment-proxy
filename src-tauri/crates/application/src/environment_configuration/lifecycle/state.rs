use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::Arc,
};

use parking_lot::Mutex;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use super::{
    snapshot::{
        EnvironmentBaselinePublic, EnvironmentCandidatePreview, EnvironmentValidationLayerResult,
    },
    types::{
        EnvironmentApplyTaskId, EnvironmentCandidateCreateResult, EnvironmentCandidateEpoch,
        EnvironmentCandidateId, EnvironmentCandidateLifecycleError, EnvironmentCandidatePolicy,
        EnvironmentCandidateStatus, EnvironmentCandidateStatusResult, EnvironmentConfirmationToken,
    },
};
use crate::ProxyWorkspace;
use crate::environment_configuration::{
    EnvironmentAdmittedTarget, EnvironmentCommitReceipt, EnvironmentConfigurationCandidateV1,
    EnvironmentDiagnostic, EnvironmentStatusCode, EnvironmentTerminalResult,
    EnvironmentValidatedApplyBaseline, StagedProtectedMaterialHandle,
};

pub(super) struct PrivateCandidateMaterial {
    encoded_candidate: Zeroizing<Vec<u8>>,
    validated_workspace: Option<ProxyWorkspace>,
}

impl PrivateCandidateMaterial {
    pub(super) fn new(
        candidate: &EnvironmentConfigurationCandidateV1,
    ) -> Result<Self, EnvironmentCandidateLifecycleError> {
        serde_json::to_vec(candidate)
            .map(Zeroizing::new)
            .map(|encoded_candidate| Self {
                encoded_candidate,
                validated_workspace: None,
            })
            .map_err(|_| EnvironmentCandidateLifecycleError::PrivateMaterialEncodingFailed)
    }

    pub(super) fn byte_len(&self) -> usize {
        self.encoded_candidate.len()
    }

    pub(super) fn set_validated_workspace(&mut self, workspace: ProxyWorkspace) {
        self.validated_workspace = Some(workspace);
    }

    fn into_staged(mut self) -> Option<StagedProtectedMaterialHandle> {
        self.validated_workspace.take().map(|workspace| {
            StagedProtectedMaterialHandle::from_candidate_json(
                std::mem::take(&mut self.encoded_candidate),
                workspace,
            )
        })
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
    pub(super) validated_apply_baseline: Option<EnvironmentValidatedApplyBaseline>,
    pub(super) validation_cancellation: CancellationToken,
}

impl CandidateEntry {
    pub(super) fn public_create_result(&self) -> EnvironmentCandidateCreateResult {
        EnvironmentCandidateCreateResult {
            candidate_id: self.id.clone(),
            confirmation_token: match &self.token {
                Some(TokenState::Available(token)) => Some(token.clone()),
                Some(TokenState::Consumed) | None => None,
            },
            status: self.status,
            target_key: self.target_key.clone(),
            baseline_public: self.baseline_public.clone(),
            validation_layers: self.validation_layers.clone(),
            preview: self.preview.clone(),
            expires_on: "app_exit_or_invalidation",
            errors: self.errors.clone(),
        }
    }

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
    pub(super) next_candidate_epoch: u64,
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
    pub(super) validated_apply_baseline: EnvironmentValidatedApplyBaseline,
    pub(super) completed: bool,
}

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

    pub fn take_staged_material(
        &mut self,
    ) -> Result<StagedProtectedMaterialHandle, EnvironmentCandidateLifecycleError> {
        self.material
            .take()
            .and_then(PrivateCandidateMaterial::into_staged)
            .ok_or(EnvironmentCandidateLifecycleError::InvalidState)
    }

    pub fn validated_apply_baseline(&self) -> &EnvironmentValidatedApplyBaseline {
        &self.validated_apply_baseline
    }

    pub fn finish_committed(
        mut self,
        receipt: EnvironmentCommitReceipt,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        let (result, receipt_task_id) = receipt.into_parts();
        if receipt_task_id != self.apply_task_id {
            return Err(EnvironmentCandidateLifecycleError::InvalidState);
        }
        let terminal = EnvironmentTerminalResult::committed(
            result.workspace_id,
            result.revision,
            result.selected_workspace_id,
            Some(self.apply_task_id.as_str().to_owned()),
        );
        self.finish(EnvironmentCandidateStatus::Committed, terminal)
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

    pub fn finish_stale(
        mut self,
        status_code: EnvironmentStatusCode,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        self.finish(
            EnvironmentCandidateStatus::Stale,
            EnvironmentTerminalResult::stale_with(status_code),
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
