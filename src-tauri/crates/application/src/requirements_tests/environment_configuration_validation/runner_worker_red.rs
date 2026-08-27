use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration as StdDuration;

use super::*;
use crate::environment_configuration::{EnvironmentCpuWorkProbe, EnvironmentProjectedCandidate};
use crate::requirements_tests::{
    FakePorts, application_with_environment_preview_ports, test_environment_baseline_capture,
    test_environment_identity_allocator,
};
use crate::{EnvironmentCandidateEpoch, InMemoryWorkspaceStore, WorkspaceRepositoryPort};

const MID_WORK_CHECKPOINT: usize = 3;

struct CheckpointProbe {
    cancellation: Option<CancellationToken>,
    deadline_delay: Option<StdDuration>,
    domain_checkpoints: Mutex<Vec<usize>>,
    candidate_buffer_dropped: AtomicUsize,
}

impl CheckpointProbe {
    fn cancelling_at_mid_work(cancellation: CancellationToken) -> Self {
        Self {
            cancellation: Some(cancellation),
            deadline_delay: None,
            domain_checkpoints: Mutex::new(Vec::new()),
            candidate_buffer_dropped: AtomicUsize::new(0),
        }
    }

    fn expiring_deadline_at_mid_work() -> Self {
        Self {
            cancellation: None,
            deadline_delay: Some(StdDuration::from_millis(550)),
            domain_checkpoints: Mutex::new(Vec::new()),
            candidate_buffer_dropped: AtomicUsize::new(0),
        }
    }
}

impl EnvironmentCpuWorkProbe for CheckpointProbe {
    fn checkpoint(&self, layer: EnvironmentValidationLayer, checkpoint_index: usize) {
        if layer == EnvironmentValidationLayer::Domain {
            self.domain_checkpoints
                .lock()
                .unwrap()
                .push(checkpoint_index);
        }
        if layer == EnvironmentValidationLayer::Domain && checkpoint_index == MID_WORK_CHECKPOINT {
            if let Some(cancellation) = &self.cancellation {
                cancellation.cancel();
            }
            if let Some(delay) = self.deadline_delay {
                std::thread::sleep(delay);
            }
        }
    }

    fn candidate_buffer_dropped(&self) {
        self.candidate_buffer_dropped.fetch_add(1, Ordering::SeqCst);
    }
}

fn assert_stopped_at_mid_work(probe: &CheckpointProbe) {
    let checkpoints = probe.domain_checkpoints.lock().unwrap();
    assert_eq!(checkpoints.last(), Some(&MID_WORK_CHECKPOINT));
    assert!(
        checkpoints
            .iter()
            .all(|index| *index <= MID_WORK_CHECKPOINT)
    );
}

#[test]
fn candidate_validation_cpu_path_forbids_detached_secret_bearing_workers() {
    let source = include_str!("../../environment_configuration/validation/runner.rs");

    for forbidden in ["spawn_blocking", "JoinHandle"] {
        assert!(
            !source.contains(forbidden),
            "candidate validation must not use `{forbidden}`"
        );
    }
    assert!(source.contains("checkpoint("));
}

#[tokio::test]
async fn cancellation_at_a_cpu_checkpoint_reports_before_candidate_buffer_drop_returns() {
    let cancellation = CancellationToken::new();
    let probe = Arc::new(CheckpointProbe::cancelling_at_mid_work(
        cancellation.clone(),
    ));
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    let application = application_with_environment_preview_ports(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::new_empty()),
        test_environment_baseline_capture(),
        test_environment_identity_allocator(),
    );
    let candidate = crate::parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();

    let report = validator(Arc::clone(&port))
        .with_cpu_work_probe(probe.clone())
        .validate_for_candidate(
            inserted.candidate_id(),
            FULL_SHAPE,
            CancellationToken::new(),
            cancellation,
            &application,
        )
        .await;

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::CandidateCancelled)
    );
    assert_stopped_at_mid_work(&probe);
    assert_eq!(port.calls(), vec![EnvironmentValidationLayer::Schema]);
    assert_eq!(probe.candidate_buffer_dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Cancelled
    );
    for layer in &report.layers()[2..] {
        assert_eq!(
            layer.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
    }
}

#[tokio::test]
async fn total_deadline_at_a_cpu_checkpoint_reports_before_candidate_buffer_drop_returns() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let allocator = test_environment_identity_allocator();
    let candidate = crate::parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let projected = EnvironmentProjectedCandidate::project(candidate, None, allocator.port())
        .expect("authoritative candidate projects");
    let persisted = store
        .import_workspace(projected.workspace().clone())
        .await
        .unwrap();
    let mut candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    candidate["target"] = serde_json::json!({
        "mode": "existing",
        "workspace_id": persisted.id,
        "expected_revision": persisted.revision,
    });
    for (listener, existing) in candidate["workspace"]["listeners"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(&persisted.listeners)
    {
        listener["id"] = serde_json::json!(existing.id);
    }
    candidate["workspace"]["http_rules"][0]["existing_rule_id"] =
        serde_json::json!(persisted.rules[0].id);
    candidate["workspace"]["protocol_rules"][0]["existing_rule_id"] =
        serde_json::json!(persisted.protocol_rules[0].rule_id());
    let bytes = serde_json::to_vec(&candidate).unwrap();
    let typed = crate::parse_environment_configuration_candidate_v1(&bytes).unwrap();
    let application = application_with_environment_preview_ports(
        Arc::new(FakePorts::default()),
        store,
        test_environment_baseline_capture(),
        test_environment_identity_allocator(),
    );
    let inserted = application
        .environment_candidate_insert_validating(typed, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let probe = Arc::new(CheckpointProbe::expiring_deadline_at_mid_work());
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));

    let report = validator(Arc::clone(&port))
        .with_total_deadline(Duration::from_millis(500))
        .with_cpu_work_probe(probe.clone())
        .validate_for_candidate(
            inserted.candidate_id(),
            &bytes,
            CancellationToken::new(),
            CancellationToken::new(),
            &application,
        )
        .await;

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::McpCreateDeadlineExceeded)
    );
    assert_stopped_at_mid_work(&probe);
    assert_eq!(port.calls(), vec![EnvironmentValidationLayer::Schema]);
    assert_eq!(probe.candidate_buffer_dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Cancelled
    );
    for layer in &report.layers()[2..] {
        assert_eq!(
            layer.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
    }
}
