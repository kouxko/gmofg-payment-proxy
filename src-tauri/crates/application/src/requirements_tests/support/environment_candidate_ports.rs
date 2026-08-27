use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    AppError, AppResult, EnvironmentApplyBaselineCapturePort,
    EnvironmentApplyBaselineCaptureRequest, EnvironmentApplyGenerations, EnvironmentApplyLease,
    EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest, EnvironmentCommitFailure,
    EnvironmentCommitPort, EnvironmentCommitRequest, EnvironmentCommitResult,
    EnvironmentIdentityAllocator, EnvironmentIdentityAllocatorPort, EnvironmentPreparedMaterials,
    EnvironmentProtectedMaterialPreparePort, EnvironmentValidatedApplyBaseline,
    EnvironmentValidationLayerPort, EnvironmentValidationLayerRequest, EnvironmentValidationStatus,
    ListenerId, ProtocolDocumentRuleId, RuleId, StagedProtectedMaterialHandle, WorkspaceId,
};

struct TestEnvironmentBaselineCapture;

#[async_trait]
impl EnvironmentApplyBaselineCapturePort for TestEnvironmentBaselineCapture {
    async fn capture(
        &self,
        _request: EnvironmentApplyBaselineCaptureRequest,
    ) -> crate::AppResult<EnvironmentValidatedApplyBaseline> {
        Ok(EnvironmentValidatedApplyBaseline::validated(
            EnvironmentApplyGenerations::default(),
            [1; 32],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ))
    }
}

pub(in crate::requirements_tests) fn test_environment_baseline_capture()
-> Arc<dyn EnvironmentApplyBaselineCapturePort> {
    Arc::new(TestEnvironmentBaselineCapture)
}

pub(in crate::requirements_tests) fn test_environment_identity_allocator()
-> EnvironmentIdentityAllocator {
    EnvironmentIdentityAllocator::from_port(Arc::new(FixtureEnvironmentIdentityAllocator))
}

struct UnusedEnvironmentApplyPorts;

#[async_trait]
impl EnvironmentApplyLeasePort for UnusedEnvironmentApplyPorts {
    async fn acquire(&self, _: EnvironmentApplyLeaseRequest) -> AppResult<EnvironmentApplyLease> {
        Err(AppError::new("UNUSED_TEST_PORT", "unused test apply lease"))
    }
}

#[async_trait]
impl EnvironmentProtectedMaterialPreparePort for UnusedEnvironmentApplyPorts {
    async fn prepare(
        &self,
        _: StagedProtectedMaterialHandle,
    ) -> AppResult<EnvironmentPreparedMaterials> {
        Err(AppError::new(
            "UNUSED_TEST_PORT",
            "unused test material prepare",
        ))
    }
}

#[async_trait]
impl EnvironmentCommitPort for UnusedEnvironmentApplyPorts {
    async fn commit(
        &self,
        _: EnvironmentCommitRequest,
    ) -> Result<EnvironmentCommitResult, EnvironmentCommitFailure> {
        Err(EnvironmentCommitFailure::before_transaction(AppError::new(
            "UNUSED_TEST_PORT",
            "unused test commit",
        )))
    }
}

#[async_trait]
impl EnvironmentValidationLayerPort for UnusedEnvironmentApplyPorts {
    async fn validate_layer(
        &self,
        _: EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        Ok(EnvironmentValidationStatus::Passed)
    }
}

pub(in crate::requirements_tests) fn test_environment_apply_lease()
-> Arc<dyn EnvironmentApplyLeasePort> {
    Arc::new(UnusedEnvironmentApplyPorts)
}

pub(in crate::requirements_tests) fn test_environment_material_preparer()
-> Arc<dyn EnvironmentProtectedMaterialPreparePort> {
    Arc::new(UnusedEnvironmentApplyPorts)
}

pub(in crate::requirements_tests) fn test_environment_commit() -> Arc<dyn EnvironmentCommitPort> {
    Arc::new(UnusedEnvironmentApplyPorts)
}

pub(in crate::requirements_tests) fn test_environment_validator()
-> Arc<dyn EnvironmentValidationLayerPort> {
    Arc::new(UnusedEnvironmentApplyPorts)
}

struct FixtureEnvironmentIdentityAllocator;

impl EnvironmentIdentityAllocatorPort for FixtureEnvironmentIdentityAllocator {
    fn allocate_workspace_id(&self) -> WorkspaceId {
        WorkspaceId::from_uuid(uuid::Uuid::from_u128(0x100))
    }

    fn allocate_listener_id(&self, candidate_index: usize, _alias: &str) -> ListenerId {
        ListenerId::from_uuid(uuid::Uuid::from_u128(0x101 + candidate_index as u128))
    }

    fn allocate_http_rule(&self, candidate_index: usize) -> (RuleId, u64) {
        (
            uuid::Uuid::from_u128(0x120 + candidate_index as u128),
            10 + candidate_index as u64,
        )
    }

    fn allocate_protocol_rule(&self, candidate_index: usize) -> (ProtocolDocumentRuleId, u64) {
        (
            ProtocolDocumentRuleId::from_uuid(uuid::Uuid::from_u128(
                0x130 + candidate_index as u128,
            )),
            20 + candidate_index as u64,
        )
    }

    fn allocate_android_profile_id(&self, candidate_index: usize) -> String {
        format!("android-profile-{candidate_index}")
    }
}
