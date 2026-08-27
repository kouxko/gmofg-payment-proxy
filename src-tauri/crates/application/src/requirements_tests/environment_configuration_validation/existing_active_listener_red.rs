use super::*;

use crate::requirements_tests::{
    FakePorts, application_with_environment_preview_apply_ports_and_runtime,
    application_with_environment_preview_ports_and_runtime, test_environment_identity_allocator,
};
use crate::{
    EnvironmentAffectedListenerBaseline, EnvironmentApplyBaselineCapturePort,
    EnvironmentApplyBaselineCaptureRequest, EnvironmentApplyGenerations, EnvironmentApplyLease,
    EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest, EnvironmentCandidateEpoch,
    EnvironmentCandidateStatus, EnvironmentCommitFailure, EnvironmentCommitPort,
    EnvironmentCommitRequest, EnvironmentCommitResult, EnvironmentConfirmationToken,
    EnvironmentPreparedMaterials, EnvironmentPreviewBaselinePort,
    EnvironmentPreviewBaselineRequest, EnvironmentProtectedMaterialPreparePort,
    EnvironmentValidatedApplyBaseline, InMemoryListenerRuntime, InMemoryWorkspaceStore,
    ListenerRuntimePort, StagedProtectedMaterialHandle, WorkspaceRepositoryPort,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct RuntimeObservingBaselineCapture {
    runtime: Arc<InMemoryListenerRuntime>,
    observed: Mutex<Vec<EnvironmentAffectedListenerBaseline>>,
}

struct RuntimeActiveLease {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl EnvironmentApplyLeasePort for RuntimeActiveLease {
    async fn acquire(&self, _: EnvironmentApplyLeaseRequest) -> AppResult<EnvironmentApplyLease> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(EnvironmentApplyLease::runtime_active_with_reverse_release(
            EnvironmentApplyGenerations::default(),
            || {},
        ))
    }
}

struct CountingPrepare {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl EnvironmentProtectedMaterialPreparePort for CountingPrepare {
    async fn prepare(
        &self,
        _: StagedProtectedMaterialHandle,
    ) -> AppResult<EnvironmentPreparedMaterials> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(crate::AppError::new(
            "UNEXPECTED_PREPARE",
            "prepare must not run",
        ))
    }
}

struct CountingCommit {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl EnvironmentCommitPort for CountingCommit {
    async fn commit(
        &self,
        _: EnvironmentCommitRequest,
    ) -> Result<EnvironmentCommitResult, EnvironmentCommitFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(EnvironmentCommitFailure::before_transaction(
            crate::AppError::new("UNEXPECTED_COMMIT", "commit must not run"),
        ))
    }
}

struct TokenCapturingPreview<'a> {
    application: &'a crate::Application,
    token: Mutex<Option<EnvironmentConfirmationToken>>,
}

#[async_trait::async_trait]
impl EnvironmentPreviewBaselinePort for TokenCapturingPreview<'_> {
    fn domain_projection_port(
        &self,
    ) -> Option<&dyn crate::environment_configuration::EnvironmentDomainProjectionPort> {
        Some(self.application)
    }

    async fn validate_preview_baseline(
        &self,
        request: EnvironmentPreviewBaselineRequest<'_>,
    ) -> AppResult<()> {
        let projected = request.projected_candidate().unwrap();
        let snapshot = crate::environment_configuration::candidate_preview_snapshot(
            projected.candidate(),
            request.prior_layers(),
            projected.workspace(),
        )?;
        let ready = self
            .application
            .environment_candidate_complete_preview_ready(
                request.candidate_id(),
                snapshot,
                projected.workspace().clone(),
            )
            .await
            .unwrap();
        self.token
            .lock()
            .unwrap()
            .clone_from(&ready.confirmation_token().cloned());
        Ok(())
    }
}

#[test]
fn tests_and_production_share_the_same_facade_apply_path() {
    let facade = include_str!("../../facade.rs");
    let candidates = include_str!("../../facade/environment_candidates.rs");

    for field in [
        "environment_apply_lease",
        "environment_material_preparer",
        "environment_commit",
    ] {
        let cfg_guarded = format!("#[cfg(not(test))]\n    pub {field}");
        assert!(
            !facade.contains(&cfg_guarded),
            "ApplicationDependencies `{field}` must be injectable in the real test path"
        );
    }
    assert!(
        !candidates
            .contains("#[cfg(not(test))]\n    pub fn environment_candidate_queue_and_start_apply")
    );
}

#[async_trait::async_trait]
impl EnvironmentApplyBaselineCapturePort for RuntimeObservingBaselineCapture {
    async fn capture(
        &self,
        request: EnvironmentApplyBaselineCaptureRequest,
    ) -> AppResult<EnvironmentValidatedApplyBaseline> {
        let statuses = self.runtime.statuses().await?;
        let affected = statuses
            .into_iter()
            .filter(|status| {
                request
                    .candidate_workspace
                    .listeners
                    .iter()
                    .any(|listener| listener.id == status.listener_id)
            })
            .map(|status| {
                EnvironmentAffectedListenerBaseline::observed(
                    status.listener_id.as_uuid(),
                    status.runtime_epoch,
                    1,
                )
            })
            .collect::<Vec<_>>();
        self.observed.lock().unwrap().clone_from(&affected);
        Ok(EnvironmentValidatedApplyBaseline::validated(
            EnvironmentApplyGenerations::default(),
            [1; 32],
            affected,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ))
    }
}

#[tokio::test]
async fn existing_target_with_the_same_active_listener_id_reaches_preview_ready() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let persisted = workspaces.create("Persisted".into()).await.unwrap();
    let runtime = Arc::new(InMemoryListenerRuntime::default());
    runtime
        .start(persisted.clone(), persisted.listeners[0].clone())
        .await
        .unwrap();
    let capture = Arc::new(RuntimeObservingBaselineCapture {
        runtime: runtime.clone(),
        observed: Mutex::new(Vec::new()),
    });
    let application = application_with_environment_preview_ports_and_runtime(
        Arc::new(FakePorts::default()),
        workspaces,
        runtime,
        capture.clone(),
        test_environment_identity_allocator(),
    );
    let mut value: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    value["target"] = serde_json::json!({
        "mode": "existing",
        "workspace_id": persisted.id,
        "expected_revision": persisted.revision,
    });
    value["workspace"]["listeners"][0]["id"] = serde_json::json!(persisted.listeners[0].id);
    let bytes = serde_json::to_vec(&value).unwrap();
    let candidate = crate::parse_environment_configuration_candidate_v1(&bytes).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let cancellation = CancellationToken::new();

    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate_for_candidate(
            inserted.candidate_id(),
            &bytes,
            cancellation.clone(),
            cancellation,
            &application,
        )
        .await;

    assert_eq!(report.status_code(), None);
    assert_eq!(
        report.layers()[6].status(),
        EnvironmentValidationStatus::Passed
    );
    assert_eq!(
        application
            .environment_candidate_status(inserted.candidate_id())
            .status(),
        EnvironmentCandidateStatus::PreviewReady,
    );
    let observed = capture.observed.lock().unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(
        observed[0].listener_id(),
        persisted.listeners[0].id.as_uuid()
    );
    assert_eq!(observed[0].active_count(), 1);
}

#[tokio::test]
async fn same_active_listener_candidate_fails_apply_before_prepare_or_commit() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let persisted = workspaces.create("Persisted".into()).await.unwrap();
    let runtime = Arc::new(InMemoryListenerRuntime::default());
    runtime
        .start(persisted.clone(), persisted.listeners[0].clone())
        .await
        .unwrap();
    let capture = Arc::new(RuntimeObservingBaselineCapture {
        runtime: runtime.clone(),
        observed: Mutex::new(Vec::new()),
    });
    let lease = Arc::new(RuntimeActiveLease {
        calls: AtomicUsize::new(0),
    });
    let prepare = Arc::new(CountingPrepare {
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(CountingCommit {
        calls: AtomicUsize::new(0),
    });
    let validation_port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    let application = application_with_environment_preview_apply_ports_and_runtime(
        Arc::new(FakePorts::default()),
        workspaces,
        runtime,
        capture,
        test_environment_identity_allocator(),
        lease.clone(),
        prepare.clone(),
        commit.clone(),
        validation_port.clone(),
    );
    let mut value: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    value["target"] = serde_json::json!({
        "mode": "existing",
        "workspace_id": persisted.id,
        "expected_revision": persisted.revision,
    });
    value["workspace"]["listeners"][0]["id"] = serde_json::json!(persisted.listeners[0].id);
    let bytes = serde_json::to_vec(&value).unwrap();
    let candidate = crate::parse_environment_configuration_candidate_v1(&bytes).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(2))
        .unwrap();
    let preview = TokenCapturingPreview {
        application: &application,
        token: Mutex::new(None),
    };
    let cancellation = CancellationToken::new();

    let report = validator(validation_port)
        .validate_for_candidate(
            inserted.candidate_id(),
            &bytes,
            cancellation.clone(),
            cancellation,
            &preview,
        )
        .await;
    assert_eq!(report.status_code(), None);
    let token = preview.token.lock().unwrap().clone().unwrap();
    application
        .environment_candidate_queue_and_start_apply(inserted.candidate_id(), &token)
        .unwrap();
    for _ in 0..100 {
        if application
            .environment_candidate_status(inserted.candidate_id())
            .status()
            == EnvironmentCandidateStatus::FailedBeforeCommit
        {
            break;
        }
        tokio::task::yield_now().await;
    }

    let status = application.environment_candidate_status(inserted.candidate_id());
    assert_eq!(
        status.status(),
        EnvironmentCandidateStatus::FailedBeforeCommit
    );
    let status_json = serde_json::to_value(status).unwrap();
    assert_eq!(
        status_json["terminal_result"]["status_code"],
        "RUNTIME_ACTIVE"
    );
    assert_eq!(lease.calls.load(Ordering::SeqCst), 1);
    assert_eq!(prepare.calls.load(Ordering::SeqCst), 0);
    assert_eq!(commit.calls.load(Ordering::SeqCst), 0);
}
