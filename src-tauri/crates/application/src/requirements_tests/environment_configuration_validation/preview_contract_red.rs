use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Notify;

use super::*;
use crate::EnvironmentValidationReport;
use crate::environment_configuration::{
    EnvironmentPreviewBaselinePort, EnvironmentPreviewBaselineRequest,
};
use crate::requirements_tests::{
    FakePorts, application_with_environment_preview_ports,
    application_with_fake_ports_and_listener_runtime, application_with_workspace_ports,
    test_environment_identity_allocator,
};
use crate::{
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentApplyGenerations, EnvironmentCandidateEpoch, EnvironmentIdentityAllocator,
    EnvironmentIdentityAllocatorPort, EnvironmentValidatedApplyBaseline, InMemoryListenerRuntime,
    InMemoryWorkspaceStore, ListenerId, ProtocolDocumentRuleId, RuleId, WorkspaceId,
    WorkspaceRepositoryPort,
};

const EXPECTED_PREVIEW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/expected-preview.json"
));

struct BlockingPreview {
    entered: Notify,
}

#[derive(Default)]
struct RecordingPreviewCapture {
    requests: Mutex<Vec<EnvironmentApplyBaselineCaptureRequest>>,
}

#[derive(Default)]
struct CountingIdentityAllocator {
    workspace: AtomicUsize,
    listeners: AtomicUsize,
    http_rules: AtomicUsize,
    protocol_rules: AtomicUsize,
    android_profiles: AtomicUsize,
}

impl EnvironmentIdentityAllocatorPort for CountingIdentityAllocator {
    fn allocate_workspace_id(&self) -> WorkspaceId {
        let call = self.workspace.fetch_add(1, Ordering::SeqCst);
        WorkspaceId::from_uuid(uuid::Uuid::from_u128(0x200 + call as u128))
    }

    fn allocate_listener_id(&self, _: usize, _: &str) -> ListenerId {
        let call = self.listeners.fetch_add(1, Ordering::SeqCst);
        ListenerId::from_uuid(uuid::Uuid::from_u128(0x300 + call as u128))
    }

    fn allocate_http_rule(&self, _: usize) -> (RuleId, u64) {
        let call = self.http_rules.fetch_add(1, Ordering::SeqCst);
        (
            uuid::Uuid::from_u128(0x400 + call as u128),
            100 + call as u64,
        )
    }

    fn allocate_protocol_rule(&self, _: usize) -> (ProtocolDocumentRuleId, u64) {
        let call = self.protocol_rules.fetch_add(1, Ordering::SeqCst);
        (
            ProtocolDocumentRuleId::from_uuid(uuid::Uuid::from_u128(0x500 + call as u128)),
            200 + call as u64,
        )
    }

    fn allocate_android_profile_id(&self, _: usize) -> String {
        let call = self.android_profiles.fetch_add(1, Ordering::SeqCst);
        format!("generated-android-{call}")
    }
}

#[async_trait]
impl EnvironmentApplyBaselineCapturePort for RecordingPreviewCapture {
    async fn capture(
        &self,
        request: EnvironmentApplyBaselineCaptureRequest,
    ) -> AppResult<EnvironmentValidatedApplyBaseline> {
        self.requests.lock().unwrap().push(request);
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

#[async_trait]
impl EnvironmentPreviewBaselinePort for BlockingPreview {
    async fn validate_preview_baseline(
        &self,
        request: EnvironmentPreviewBaselineRequest<'_>,
    ) -> AppResult<()> {
        assert_eq!(request.validated_candidate_json(), FULL_SHAPE);
        assert_eq!(request.prior_layers().len(), 6);
        self.entered.notify_one();
        pending().await
    }
}

async fn run_blocked_preview(
    validator: EnvironmentCandidateValidator<RecordingValidationPort>,
    candidate_id: crate::EnvironmentCandidateId,
    candidate_cancellation: CancellationToken,
    preview: Arc<BlockingPreview>,
) -> EnvironmentValidationReport {
    validator
        .validate_for_candidate(
            &candidate_id,
            FULL_SHAPE,
            CancellationToken::new(),
            candidate_cancellation,
            preview.as_ref(),
        )
        .await
}

#[tokio::test(start_paused = true)]
async fn preview_baseline_real_work_is_bounded_by_two_seconds() {
    let (_, candidate_id, candidate_cancellation) = review_red::validating_registry_candidate();
    let preview = Arc::new(BlockingPreview {
        entered: Notify::new(),
    });
    let task = tokio::spawn(run_blocked_preview(
        validator(Arc::new(RecordingValidationPort::new(Behavior::Pass))),
        candidate_id,
        candidate_cancellation,
        Arc::clone(&preview),
    ));
    preview.entered.notified().await;
    tokio::time::advance(Duration::from_secs(2)).await;
    let report = task.await.unwrap();

    assert_eq!(
        report.layers()[6].layer(),
        EnvironmentValidationLayer::PreviewBaseline
    );
    assert_eq!(
        report.layers()[6].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(report.layers()[6].reason(), Some("layer_budget_exceeded"));
}

#[tokio::test(start_paused = true)]
async fn total_thirty_second_deadline_dominates_blocked_preview_work() {
    let (_, candidate_id, candidate_cancellation) = review_red::validating_registry_candidate();
    let preview = Arc::new(BlockingPreview {
        entered: Notify::new(),
    });
    let validator = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .with_layer_budget(
            EnvironmentValidationLayer::PreviewBaseline,
            Duration::from_secs(40),
        );
    let task = tokio::spawn(run_blocked_preview(
        validator,
        candidate_id,
        candidate_cancellation,
        Arc::clone(&preview),
    ));
    preview.entered.notified().await;
    tokio::time::advance(Duration::from_secs(30)).await;
    let report = task.await.unwrap();

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::McpCreateDeadlineExceeded)
    );
    assert_eq!(
        report.layers()[6].status(),
        EnvironmentValidationStatus::Cancelled
    );
    assert_eq!(
        report.layers()[6].reason(),
        Some("create_deadline_exceeded")
    );
}

#[tokio::test]
async fn candidate_cancel_interrupts_blocked_preview_work() {
    let (registry, candidate_id, candidate_cancellation) =
        review_red::validating_registry_candidate();
    let preview = Arc::new(BlockingPreview {
        entered: Notify::new(),
    });
    let task = tokio::spawn(run_blocked_preview(
        validator(Arc::new(RecordingValidationPort::new(Behavior::Pass))),
        candidate_id.clone(),
        candidate_cancellation,
        Arc::clone(&preview),
    ));
    preview.entered.notified().await;
    registry.cancel(&candidate_id);
    let report = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("candidate cancel interrupts preview")
        .unwrap();

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::CandidateCancelled)
    );
    assert_eq!(
        report.layers()[6].status(),
        EnvironmentValidationStatus::Cancelled
    );
}

#[tokio::test]
async fn real_application_preview_rejects_an_exact_workspace_name_collision() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    workspaces.create("Store Lab".into()).await.unwrap();
    let application =
        application_with_workspace_ports(Arc::new(FakePorts::default()), Arc::clone(&workspaces));
    let candidate = crate::parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let cancellation = CancellationToken::new();

    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate_for_candidate(
            inserted.candidate_id(),
            FULL_SHAPE,
            cancellation.clone(),
            cancellation,
            &application,
        )
        .await;

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::WorkspaceNameCollision)
    );
    assert_eq!(
        report.layers()[6].status(),
        EnvironmentValidationStatus::Failed
    );
}

#[test]
fn application_preview_tests_the_same_baseline_capture_path_as_production() {
    let facade = include_str!("../../facade/environment_candidates.rs");
    let application = include_str!("../../facade.rs");

    assert!(
        !facade.contains(concat!("#[cfg", "(test)]\n        {"))
            && !facade.contains(
                "#[cfg(not(test))]\n        self.environment_candidate_complete_preview_ready"
            ),
        "PreviewBaseline tests must execute capture -> attach baseline -> PreviewReady, not a cfg(test) substitute",
    );
    assert!(
        !application.contains("#[cfg(not(test))]\n    environment_baseline_capture")
            && !application.contains("#[cfg(not(test))]\n    pub environment_baseline_capture"),
        "tests must be able to inject and observe the production baseline capture port",
    );
}

#[tokio::test(start_paused = true)]
async fn real_application_preview_matches_every_expected_preview_field() {
    let application = application_with_fake_ports_and_listener_runtime(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryListenerRuntime::default()),
    );
    let candidate = crate::parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let cancellation = CancellationToken::new();

    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate_for_candidate(
            inserted.candidate_id(),
            FULL_SHAPE,
            cancellation.clone(),
            cancellation,
            &application,
        )
        .await;
    assert_eq!(report.status_code(), None);

    let status =
        serde_json::to_value(application.environment_candidate_status(inserted.candidate_id()))
            .unwrap();
    let preview = &status["preview"];
    let actual = serde_json::json!({
        "target_key": status["target_key"],
        "target": preview["target"],
        "baseline_public": status["baseline_public"],
        "validation_layers": status["validation_layers"],
        "resources": preview["resources"],
        "alias_graph": preview["alias_graph"],
        "materials_public": preview["materials_public"],
        "protocol_document_values": preview["protocol_document_values"],
        "terminal_action_fields": preview["terminal_action_fields"],
    });
    let expected: serde_json::Value = serde_json::from_slice(EXPECTED_PREVIEW).unwrap();

    assert_eq!(actual, expected);
}

#[tokio::test]
async fn new_target_preview_captures_the_normalized_candidate_workspace_for_apply() {
    let capture = Arc::new(RecordingPreviewCapture::default());
    let application = application_with_environment_preview_ports(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::default()),
        capture.clone(),
        test_environment_identity_allocator(),
    );
    let candidate = crate::parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let cancellation = CancellationToken::new();

    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate_for_candidate(
            inserted.candidate_id(),
            FULL_SHAPE,
            cancellation.clone(),
            cancellation,
            &application,
        )
        .await;
    assert_eq!(report.status_code(), None);

    let requests = capture.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let workspace = &requests[0].candidate_workspace;
    assert_eq!(workspace.name, "Store Lab");
    assert_eq!(workspace.listeners.len(), 3);
    assert_eq!(workspace.rules.len(), 14);
    assert_eq!(workspace.protocol_rules.len(), 1);
    assert_eq!(workspace.android_network_profiles.len(), 1);
}

#[tokio::test]
async fn existing_target_preview_captures_persisted_and_candidate_workspaces_for_apply() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let persisted = workspaces.create("Persisted".into()).await.unwrap();
    let capture = Arc::new(RecordingPreviewCapture::default());
    let application = application_with_environment_preview_ports(
        Arc::new(FakePorts::default()),
        workspaces,
        capture.clone(),
        test_environment_identity_allocator(),
    );
    let mut value: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    value["target"] = serde_json::json!({
        "mode": "existing",
        "workspace_id": persisted.id,
        "expected_revision": persisted.revision,
    });
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

    let requests = capture.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].persisted_workspace.as_ref(), Some(&persisted));
    assert_eq!(requests[0].candidate_workspace.listeners.len(), 3);
    assert_eq!(requests[0].candidate_workspace.rules.len(), 14);
    assert_eq!(requests[0].candidate_workspace.protocol_rules.len(), 1);
}

#[tokio::test]
async fn preview_and_apply_projection_share_each_non_idempotent_allocation_once() {
    let capture = Arc::new(RecordingPreviewCapture::default());
    let allocator = Arc::new(CountingIdentityAllocator::default());
    let application = application_with_environment_preview_ports(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::default()),
        capture.clone(),
        EnvironmentIdentityAllocator::from_port(allocator.clone()),
    );
    let candidate = crate::parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let cancellation = CancellationToken::new();

    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate_for_candidate(
            inserted.candidate_id(),
            FULL_SHAPE,
            cancellation.clone(),
            cancellation,
            &application,
        )
        .await;
    assert_eq!(report.status_code(), None);

    let status =
        serde_json::to_value(application.environment_candidate_status(inserted.candidate_id()))
            .unwrap();
    let workspace =
        serde_json::to_value(&capture.requests.lock().unwrap()[0].candidate_workspace).unwrap();
    assert_eq!(
        status["preview"]["resources"]["listeners"][0]["candidate_local_id"],
        workspace["listeners"][0]["id"]
    );
    assert_eq!(
        status["preview"]["resources"]["http_rules"][0]["candidate_local_id"],
        workspace["rules"][0]["id"]
    );
    assert_eq!(
        status["preview"]["resources"]["http_rules"][0]["created_order"],
        workspace["rules"][0]["created_order"]
    );
    assert_eq!(
        status["preview"]["resources"]["protocol_rules"][0]["candidate_local_id"],
        workspace["protocol_rules"][0]["rule_id"]
    );
    assert_eq!(
        status["preview"]["resources"]["protocol_rules"][0]["created_order"],
        workspace["protocol_rules"][0]["created_order"]
    );
    assert_eq!(allocator.workspace.load(Ordering::SeqCst), 1);
    assert_eq!(allocator.listeners.load(Ordering::SeqCst), 3);
    assert_eq!(allocator.http_rules.load(Ordering::SeqCst), 14);
    assert_eq!(allocator.protocol_rules.load(Ordering::SeqCst), 1);
    assert_eq!(allocator.android_profiles.load(Ordering::SeqCst), 0);
}
