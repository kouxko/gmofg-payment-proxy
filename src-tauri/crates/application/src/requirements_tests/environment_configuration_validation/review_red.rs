use super::*;
use crate::{
    EnvironmentCandidateEpoch, EnvironmentCandidatePolicy, EnvironmentCandidateRegistry,
    EnvironmentCandidateStatus, parse_environment_configuration_candidate_v1,
};

fn candidate_json_with(mut edit: impl FnMut(&mut serde_json::Value)) -> Vec<u8> {
    let mut candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    edit(&mut candidate);
    serde_json::to_vec(&candidate).unwrap()
}

#[tokio::test]
async fn unknown_root_field_fails_schema_before_any_validation_port_call() {
    let candidate = candidate_json_with(|candidate| {
        candidate["unexpected_root_field"] = serde_json::json!(true);
    });
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    let report = validator(Arc::clone(&port))
        .validate(&candidate, CancellationToken::new())
        .await;

    assert!(port.calls().is_empty());
    assert_eq!(
        report.layers()[0].layer(),
        EnvironmentValidationLayer::Schema
    );
    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(
        report.layers()[0].code(),
        Some(EnvironmentStatusCode::UnknownField)
    );
}

#[tokio::test]
async fn whitespace_workspace_name_fails_real_domain_validation() {
    let candidate = candidate_json_with(|candidate| {
        candidate["target"]["name"] = serde_json::json!("   ");
    });
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    let report = validator(port)
        .validate(&candidate, CancellationToken::new())
        .await;

    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Passed
    );
    assert_eq!(
        report.layers()[1].layer(),
        EnvironmentValidationLayer::Domain
    );
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(
        report.layers()[1].code(),
        Some(EnvironmentStatusCode::WorkspaceNameEmpty)
    );
}

#[tokio::test]
async fn preview_baseline_collision_preserves_its_registered_code() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Fail(
        EnvironmentValidationLayer::PreviewBaseline,
        "WORKSPACE_NAME_COLLISION",
    )));
    let report = validator(port)
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;

    assert_eq!(
        report.layers()[6].layer(),
        EnvironmentValidationLayer::PreviewBaseline
    );
    assert_eq!(
        report.layers()[6].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(
        report.layers()[6].code(),
        Some(EnvironmentStatusCode::WorkspaceNameCollision)
    );
    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::WorkspaceNameCollision)
    );
}

pub(super) fn validating_registry_candidate() -> (
    EnvironmentCandidateRegistry,
    crate::EnvironmentCandidateId,
    CancellationToken,
) {
    let registry = EnvironmentCandidateRegistry::new(EnvironmentCandidatePolicy::default());
    let candidate = parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let admitted = registry
        .insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let candidate_id = admitted.candidate_id().clone();
    let cancellation = registry.validation_cancellation(&candidate_id).unwrap();
    (registry, candidate_id, cancellation)
}

#[tokio::test]
async fn registry_cancel_interrupts_a_blocked_validation_port() {
    let (registry, candidate_id, cancellation) = validating_registry_candidate();
    let port = Arc::new(RecordingValidationPort::new(Behavior::Block(
        EnvironmentValidationLayer::Schema,
    )));
    let task = tokio::spawn({
        let port = Arc::clone(&port);
        async move { validator(port).validate(FULL_SHAPE, cancellation).await }
    });
    while port.calls().is_empty() {
        tokio::task::yield_now().await;
    }

    registry.cancel(&candidate_id);
    let report = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("registry cancel interrupts the blocked layer")
        .unwrap();

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::CandidateCancelled)
    );
    assert_eq!(
        registry.status(&candidate_id).status(),
        EnvironmentCandidateStatus::Cancelled
    );
}

#[tokio::test]
async fn registry_shutdown_interrupts_a_blocked_port_with_shutdown_code() {
    let (registry, candidate_id, cancellation) = validating_registry_candidate();
    let port = Arc::new(RecordingValidationPort::new(Behavior::Block(
        EnvironmentValidationLayer::Schema,
    )));
    let task = tokio::spawn({
        let port = Arc::clone(&port);
        async move { validator(port).validate(FULL_SHAPE, cancellation).await }
    });
    while port.calls().is_empty() {
        tokio::task::yield_now().await;
    }

    registry.begin_shutdown().await;
    let report = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("shutdown interrupts the blocked layer")
        .unwrap();

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::CandidateCancelledByShutdown)
    );
    assert_eq!(
        registry.status(&candidate_id).status(),
        EnvironmentCandidateStatus::CancelledByShutdown
    );
}
