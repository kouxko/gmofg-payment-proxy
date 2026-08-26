use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::snapshot::{
    EnvironmentBaselinePublic, EnvironmentCandidatePreview, EnvironmentValidationLayerResult,
};
use crate::environment_configuration::{
    EnvironmentDiagnostic, EnvironmentStatusCode, EnvironmentTerminalResult,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EnvironmentCandidateId(String);

impl EnvironmentCandidateId {
    pub fn new(value: impl Into<String>) -> Result<Self, EnvironmentIdentifierError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(EnvironmentIdentifierError("candidate ID cannot be empty"));
        }
        Ok(Self(value))
    }

    pub(super) fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EnvironmentApplyTaskId(String);

impl EnvironmentApplyTaskId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

pub struct EnvironmentConfirmationToken(Zeroizing<String>);

impl EnvironmentConfirmationToken {
    pub fn new(value: impl Into<String>) -> Result<Self, EnvironmentIdentifierError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(EnvironmentIdentifierError(
                "confirmation token cannot be empty",
            ));
        }
        Ok(Self(Zeroizing::new(value)))
    }

    pub(super) fn generate() -> Self {
        Self(Zeroizing::new(Uuid::new_v4().to_string()))
    }
}

impl Clone for EnvironmentConfirmationToken {
    fn clone(&self) -> Self {
        Self(Zeroizing::new(self.0.to_string()))
    }
}

impl PartialEq for EnvironmentConfirmationToken {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for EnvironmentConfirmationToken {}

impl Serialize for EnvironmentConfirmationToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EnvironmentConfirmationToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .map(Zeroizing::new)
            .map(Self)
    }
}

impl fmt::Debug for EnvironmentConfirmationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EnvironmentConfirmationToken([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EnvironmentCandidateEpoch(u64);

impl EnvironmentCandidateEpoch {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct EnvironmentIdentifierError(&'static str);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentCandidateStatus {
    Validating,
    PreviewReady,
    ValidationFailed,
    Stale,
    Cancelled,
    CancelledByShutdown,
    ApplyQueued,
    ApplyInProgress,
    Committed,
    RolledBack,
    FailedBeforeCommit,
    NotFound,
}

impl EnvironmentCandidateStatus {
    pub(super) fn is_active(self) -> bool {
        matches!(
            self,
            Self::Validating | Self::PreviewReady | Self::ApplyQueued | Self::ApplyInProgress
        )
    }

    pub(super) fn is_apply_active(self) -> bool {
        matches!(self, Self::ApplyQueued | Self::ApplyInProgress)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentCancelStatus {
    Cancelled,
    ApplyInProgressNotCancellable,
    NotFoundOrTerminal,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EnvironmentCandidateLifecycleError {
    #[error("candidate capacity exceeded")]
    CandidateCapacityExceeded,
    #[error("the target already has an active candidate")]
    TargetCandidateAlreadyActive,
    #[error("an apply task is already active")]
    ApplyAlreadyActive,
    #[error("the confirmation token was already consumed")]
    TokenConsumed,
    #[error("the confirmation token is missing")]
    ConfirmationTokenMissing,
    #[error("the confirmation token is invalid")]
    ConfirmationTokenInvalid,
    #[error("candidate was not found")]
    CandidateNotFound,
    #[error("candidate is not in the required lifecycle state")]
    InvalidState,
    #[error("validated snapshot target does not match the admitted candidate target")]
    ValidatedTargetMismatch,
    #[error("application shutdown is in progress")]
    ShutdownInProgress,
    #[error("candidate private material could not be encoded")]
    PrivateMaterialEncodingFailed,
    #[error("candidate public terminal projection could not be encoded")]
    TerminalProjectionEncodingFailed,
    #[error("candidate policy values must all be greater than zero")]
    InvalidPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentCandidatePolicy {
    pub(super) candidate_capacity: usize,
    pub(super) per_target_capacity: usize,
    pub(super) global_apply_capacity: usize,
    pub(super) target_apply_capacity: usize,
    pub(super) retained_terminal_count: usize,
    pub(super) retained_terminal_bytes: usize,
}

impl Default for EnvironmentCandidatePolicy {
    fn default() -> Self {
        Self {
            candidate_capacity: 4,
            per_target_capacity: 1,
            global_apply_capacity: 1,
            target_apply_capacity: 1,
            retained_terminal_count: 32,
            retained_terminal_bytes: 4_194_304,
        }
    }
}

#[cfg(test)]
impl EnvironmentCandidatePolicy {
    pub fn new(
        max_active_candidates: usize,
        max_active_per_target: usize,
        max_active_apply_global: usize,
        max_active_apply_per_target: usize,
        max_terminal_candidates: usize,
        max_terminal_public_bytes: usize,
    ) -> Result<Self, EnvironmentCandidateLifecycleError> {
        if [
            max_active_candidates,
            max_active_per_target,
            max_active_apply_global,
            max_active_apply_per_target,
            max_terminal_candidates,
            max_terminal_public_bytes,
        ]
        .contains(&0)
        {
            return Err(EnvironmentCandidateLifecycleError::InvalidPolicy);
        }
        Ok(Self {
            candidate_capacity: max_active_candidates,
            per_target_capacity: max_active_per_target,
            global_apply_capacity: max_active_apply_global,
            target_apply_capacity: max_active_apply_per_target,
            retained_terminal_count: max_terminal_candidates,
            retained_terminal_bytes: max_terminal_public_bytes,
        })
    }

    pub const fn max_active_candidates(&self) -> usize {
        self.candidate_capacity
    }
    pub const fn max_active_per_target(&self) -> usize {
        self.per_target_capacity
    }
    pub const fn max_active_apply_global(&self) -> usize {
        self.global_apply_capacity
    }
    pub const fn max_active_apply_per_target(&self) -> usize {
        self.target_apply_capacity
    }
    pub const fn max_terminal_candidates(&self) -> usize {
        self.retained_terminal_count
    }
    pub const fn max_terminal_public_bytes(&self) -> usize {
        self.retained_terminal_bytes
    }
    pub const fn eviction_policy() -> &'static str {
        "oldest_first"
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentCandidateCreateResult {
    pub(super) candidate_id: EnvironmentCandidateId,
    pub(super) confirmation_token: Option<EnvironmentConfirmationToken>,
    pub(super) status: EnvironmentCandidateStatus,
    pub(super) target_key: Option<String>,
    pub(super) baseline_public: Option<EnvironmentBaselinePublic>,
    pub(super) validation_layers: Vec<EnvironmentValidationLayerResult>,
    pub(super) preview: Option<EnvironmentCandidatePreview>,
    pub(super) expires_on: &'static str,
    pub(super) errors: Vec<EnvironmentDiagnostic>,
}

impl EnvironmentCandidateCreateResult {
    pub fn candidate_id(&self) -> &EnvironmentCandidateId {
        &self.candidate_id
    }
    pub fn confirmation_token(&self) -> Option<&EnvironmentConfirmationToken> {
        self.confirmation_token.as_ref()
    }
    pub const fn status(&self) -> EnvironmentCandidateStatus {
        self.status
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentApplyQueuedResult {
    pub(super) candidate_id: EnvironmentCandidateId,
    pub(super) apply_task_id: EnvironmentApplyTaskId,
    pub(super) status: EnvironmentCandidateStatus,
    pub(super) errors: Vec<EnvironmentDiagnostic>,
}

impl EnvironmentApplyQueuedResult {
    pub fn candidate_id(&self) -> &EnvironmentCandidateId {
        &self.candidate_id
    }
    pub fn apply_task_id(&self) -> &EnvironmentApplyTaskId {
        &self.apply_task_id
    }
    pub const fn status(&self) -> EnvironmentCandidateStatus {
        self.status
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentCancelResult {
    candidate_id: EnvironmentCandidateId,
    status: EnvironmentCancelStatus,
    terminal: bool,
    errors: Vec<EnvironmentDiagnostic>,
}

impl EnvironmentCancelResult {
    pub(super) fn new(
        candidate_id: EnvironmentCandidateId,
        status: EnvironmentCancelStatus,
        terminal: bool,
    ) -> Self {
        Self {
            candidate_id,
            status,
            terminal,
            errors: Vec::new(),
        }
    }

    pub const fn status(&self) -> EnvironmentCancelStatus {
        self.status
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentCandidateStatusResult {
    pub(super) candidate_id: EnvironmentCandidateId,
    pub(super) status: EnvironmentCandidateStatus,
    pub(super) target_key: Option<String>,
    pub(super) baseline_public: Option<EnvironmentBaselinePublic>,
    pub(super) validation_layers: Vec<EnvironmentValidationLayerResult>,
    pub(super) preview: Option<EnvironmentCandidatePreview>,
    pub(super) terminal_result: Option<EnvironmentTerminalResult>,
    pub(super) errors: Vec<EnvironmentDiagnostic>,
}

impl EnvironmentCandidateStatusResult {
    pub(super) fn not_found(candidate_id: EnvironmentCandidateId) -> Self {
        Self {
            candidate_id,
            status: EnvironmentCandidateStatus::NotFound,
            target_key: None,
            baseline_public: None,
            validation_layers: Vec::new(),
            preview: None,
            terminal_result: None,
            errors: vec![EnvironmentDiagnostic::error(
                EnvironmentStatusCode::CandidateNotFound,
            )],
        }
    }

    pub const fn status(&self) -> EnvironmentCandidateStatus {
        self.status
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnvironmentCandidateMetrics {
    pub(super) active_candidates: usize,
    pub(super) active_apply_tasks: usize,
    pub(super) private_candidate_bytes: usize,
    pub(super) retained_terminal_candidates: usize,
    pub(super) terminal_public_bytes: usize,
    pub(super) shutdown_draining_apply_tasks: usize,
}

impl EnvironmentCandidateMetrics {
    pub const fn active_candidates(&self) -> usize {
        self.active_candidates
    }
    pub const fn active_apply_tasks(&self) -> usize {
        self.active_apply_tasks
    }
    pub const fn private_candidate_bytes(&self) -> usize {
        self.private_candidate_bytes
    }
    pub const fn retained_terminal_candidates(&self) -> usize {
        self.retained_terminal_candidates
    }
    pub const fn terminal_public_bytes(&self) -> usize {
        self.terminal_public_bytes
    }
    pub const fn shutdown_draining_apply_tasks(&self) -> usize {
        self.shutdown_draining_apply_tasks
    }
}
