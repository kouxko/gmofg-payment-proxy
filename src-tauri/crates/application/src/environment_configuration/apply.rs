use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    AppError, AppResult, ListenerId, ProtocolPackageRef, ProxyWorkspace, listener_protocol_package,
};

use super::{EnvironmentApplyTaskId, EnvironmentCandidateEpoch, EnvironmentCandidateId};

mod affected;
mod baseline;
mod capability;
mod lease;
mod lifting;

/// A resource diff is classified as `Added`, `Removed`, `Changed`, or `Unchanged`.
pub use affected::{
    EnvironmentAffectedResourceDiff, EnvironmentAffectedResourceKey, EnvironmentResourceChangeKind,
};
pub use baseline::{
    EnvironmentAffectedListenerBaseline, EnvironmentAndroidOwnerBaseline,
    EnvironmentExactPackageBaseline, EnvironmentMaterialInventoryBaseline,
    EnvironmentValidatedApplyBaseline, EnvironmentValidatedApplyBaselineCollector,
};
pub(in crate::environment_configuration) use capability::PreparedMaterialCapabilityHandle;
pub use capability::{
    EnvironmentCommitRequest, EnvironmentConsumedCommitRequest,
    EnvironmentConsumedPreparedMaterials, EnvironmentPreparedMaterialCapability,
    EnvironmentPreparedMaterialKind, EnvironmentPreparedMaterialVisitor,
    EnvironmentPreparedMaterials, MaterialAlias,
};

/// Plain candidate bytes owned by the apply task. The value is intentionally neither
/// serializable nor debuggable and is zeroed when the handle is consumed or dropped.
#[expect(
    missing_debug_implementations,
    reason = "plaintext candidate material must stay redacted"
)]
pub struct StagedProtectedMaterialHandle {
    candidate_json: Zeroizing<Vec<u8>>,
    workspace: ProxyWorkspace,
}

impl StagedProtectedMaterialHandle {
    pub(super) fn from_candidate_json(
        candidate_json: Zeroizing<Vec<u8>>,
        workspace: ProxyWorkspace,
    ) -> Self {
        Self {
            candidate_json,
            workspace,
        }
    }

    /// Infrastructure consumes the stage exactly once while preparing protected material.
    pub(super) fn into_candidate_json(self) -> (Zeroizing<Vec<u8>>, ProxyWorkspace) {
        let Self {
            candidate_json,
            workspace,
        } = self;
        (candidate_json, workspace)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EnvironmentApplyGenerations {
    pub selected_workspace_id: Option<Uuid>,
    pub listener: u64,
    pub android: u64,
    /// Process-local generation for exact package runtime projections.
    pub package: u64,
    /// Stable fingerprint of the exact package inventory persisted in `SQLite`.
    pub package_inventory: u64,
    pub certificate_inventory: u64,
    pub protected_secret_inventory: u64,
    pub application_mutation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentApplyLeaseRequest {
    pub candidate_id: EnvironmentCandidateId,
    pub candidate_epoch: EnvironmentCandidateEpoch,
    pub expected: EnvironmentApplyGenerations,
    pub validated_baseline: EnvironmentValidatedApplyBaseline,
}

/// Logical guards remain owned by this value until terminalization completes. Implementations
/// queue publication while it is alive and release guards in reverse canonical order on Drop.
pub struct EnvironmentApplyLease {
    observed: EnvironmentApplyGenerations,
    outcome: EnvironmentApplyLeaseOutcome,
    release_reverse_order: Option<Box<dyn FnOnce() + Send>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentApplyLeaseOutcome {
    Acquired,
    RuntimeActive,
    AndroidOwnerMismatch,
    PackageStale,
    GenerationMismatch,
}

impl EnvironmentApplyLease {
    pub fn acquired(observed: EnvironmentApplyGenerations) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::Acquired,
            release_reverse_order: None,
        }
    }

    pub fn package_stale(observed: EnvironmentApplyGenerations) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::PackageStale,
            release_reverse_order: None,
        }
    }

    pub fn package_stale_with_reverse_release(
        observed: EnvironmentApplyGenerations,
        release_reverse_order: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::PackageStale,
            release_reverse_order: Some(Box::new(release_reverse_order)),
        }
    }

    pub fn runtime_active_with_reverse_release(
        observed: EnvironmentApplyGenerations,
        release_reverse_order: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::RuntimeActive,
            release_reverse_order: Some(Box::new(release_reverse_order)),
        }
    }

    pub fn android_owner_mismatch_with_reverse_release(
        observed: EnvironmentApplyGenerations,
        release_reverse_order: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::AndroidOwnerMismatch,
            release_reverse_order: Some(Box::new(release_reverse_order)),
        }
    }

    pub fn generation_mismatch(observed: EnvironmentApplyGenerations) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::GenerationMismatch,
            release_reverse_order: None,
        }
    }

    pub fn generation_mismatch_with_reverse_release(
        observed: EnvironmentApplyGenerations,
        release_reverse_order: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::GenerationMismatch,
            release_reverse_order: Some(Box::new(release_reverse_order)),
        }
    }

    pub fn acquired_with_reverse_release(
        observed: EnvironmentApplyGenerations,
        release_reverse_order: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            observed,
            outcome: EnvironmentApplyLeaseOutcome::Acquired,
            release_reverse_order: Some(Box::new(release_reverse_order)),
        }
    }
}

#[async_trait]
pub trait EnvironmentApplyLeasePort: Send + Sync + 'static {
    async fn acquire(
        &self,
        request: EnvironmentApplyLeaseRequest,
    ) -> AppResult<EnvironmentApplyLease>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentApplyBaselineCaptureRequest {
    pub target: EnvironmentCommitTarget,
    pub persisted_workspace: Option<ProxyWorkspace>,
    pub candidate_workspace: ProxyWorkspace,
    pub schema_version: u8,
    pub validation_engine_version: u32,
}

impl EnvironmentApplyBaselineCaptureRequest {
    pub fn affected_resource_diff(&self) -> Vec<EnvironmentAffectedResourceDiff> {
        affected::classify(self.persisted_workspace.as_ref(), &self.candidate_workspace)
    }

    pub fn affected_listener_ids(&self) -> Vec<ListenerId> {
        self.affected_resource_diff()
            .into_iter()
            .filter_map(|resource| match (resource.key, resource.change) {
                (
                    EnvironmentAffectedResourceKey::Listener(listener_id),
                    EnvironmentResourceChangeKind::Added
                    | EnvironmentResourceChangeKind::Removed
                    | EnvironmentResourceChangeKind::Changed,
                ) => Some(listener_id),
                _ => None,
            })
            .collect()
    }

    pub fn persisted_workspace_structural_hash(&self) -> [u8; 32] {
        let workspace = self
            .persisted_workspace
            .as_ref()
            .unwrap_or(&self.candidate_workspace);
        let encoded = serde_json::to_vec(workspace)
            .expect("validated ProxyWorkspace serialization cannot fail");
        let digest = ring::digest::digest(&ring::digest::SHA256, &encoded);
        let mut structural_hash = [0_u8; 32];
        structural_hash.copy_from_slice(digest.as_ref());
        structural_hash
    }

    pub fn exact_package_refs(&self) -> Vec<ProtocolPackageRef> {
        let mut packages = self
            .persisted_workspace
            .iter()
            .chain(std::iter::once(&self.candidate_workspace))
            .flat_map(|workspace| workspace.listeners.iter())
            .filter_map(listener_protocol_package)
            .cloned()
            .collect::<Vec<_>>();
        packages.sort_by(|left, right| {
            left.id.as_str().cmp(right.id.as_str()).then_with(|| {
                left.version
                    .semantic_cmp(&right.version)
                    .then_with(|| left.version.as_str().cmp(right.version.as_str()))
            })
        });
        packages.dedup();
        packages
    }
}

#[async_trait]
pub trait EnvironmentApplyBaselineCapturePort: Send + Sync + 'static {
    async fn capture(
        &self,
        request: EnvironmentApplyBaselineCaptureRequest,
    ) -> AppResult<EnvironmentValidatedApplyBaseline>;
}

#[async_trait]
pub trait EnvironmentProtectedMaterialPreparePort: Send + Sync + 'static {
    async fn prepare(
        &self,
        staged: StagedProtectedMaterialHandle,
    ) -> AppResult<EnvironmentPreparedMaterials>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentCommitTarget {
    Existing {
        workspace_id: Uuid,
        expected_revision: u64,
    },
    New {
        workspace_id: Uuid,
        display_name: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentSelectionPolicy {
    PreserveExistingSelectionOrSelectNewWhenNone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentCommitResult {
    pub workspace_id: Uuid,
    pub revision: u64,
    pub selected_workspace_id: Option<Uuid>,
    pub reused_materials: usize,
    pub inserted_materials: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvironmentCommitFailure {
    BeforeTransaction(AppError),
    RolledBack {
        error: AppError,
        outcome: EnvironmentCommitRollbackOutcome,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentCommitRollbackOutcome {
    BaselineMismatch,
    Failed,
}

impl EnvironmentCommitFailure {
    pub fn before_transaction(error: AppError) -> Self {
        Self::BeforeTransaction(error)
    }

    pub fn rolled_back(error: AppError, outcome: EnvironmentCommitRollbackOutcome) -> Self {
        Self::RolledBack { error, outcome }
    }

    pub const fn rollback_outcome(&self) -> Option<EnvironmentCommitRollbackOutcome> {
        match self {
            Self::BeforeTransaction(_) => None,
            Self::RolledBack { outcome, .. } => Some(*outcome),
        }
    }

    pub fn error(&self) -> &AppError {
        match self {
            Self::BeforeTransaction(error) | Self::RolledBack { error, .. } => error,
        }
    }
}

#[async_trait]
pub trait EnvironmentCommitPort: Send + Sync + 'static {
    async fn commit(
        &self,
        request: EnvironmentCommitRequest,
    ) -> Result<EnvironmentCommitResult, EnvironmentCommitFailure>;
}

/// Success authority manufactured only by the Application worker after a commit port returns.
#[expect(missing_debug_implementations, reason = "unforgeable authority")]
pub struct EnvironmentCommitReceipt {
    result: EnvironmentCommitResult,
    apply_task_id: EnvironmentApplyTaskId,
}
impl EnvironmentCommitReceipt {
    pub(super) fn after_commit(
        result: EnvironmentCommitResult,
        apply_task_id: EnvironmentApplyTaskId,
    ) -> Self {
        Self {
            result,
            apply_task_id,
        }
    }

    pub(super) fn into_parts(self) -> (EnvironmentCommitResult, EnvironmentApplyTaskId) {
        (self.result, self.apply_task_id)
    }
}
