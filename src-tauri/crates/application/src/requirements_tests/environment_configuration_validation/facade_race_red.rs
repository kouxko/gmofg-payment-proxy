use super::*;
use crate::requirements_tests::{FakePorts, application_with_fake_ports};
use crate::{
    EnvironmentCandidateEpoch, EnvironmentCandidateStatus, EnvironmentValidationReport,
    parse_environment_configuration_candidate_v1,
};

async fn shutdown_cancelled_report() -> EnvironmentValidationReport {
    let (registry, candidate_id, cancellation) = review_red::validating_registry_candidate();
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
    assert_eq!(
        registry.status(&candidate_id).status(),
        EnvironmentCandidateStatus::CancelledByShutdown
    );
    task.await.unwrap()
}

#[tokio::test]
async fn facade_shutdown_barrier_does_not_complete_validation_failed_twice() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let inserted = application
        .environment_candidate_insert_validating(
            parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap(),
            EnvironmentCandidateEpoch::new(1),
        )
        .unwrap();
    application.environment_candidate_begin_shutdown().await;
    let report = shutdown_cancelled_report().await;

    application
        .environment_candidate_finish_validation(inserted.candidate_id(), report)
        .expect("shutdown already terminalized the candidate");
    assert_eq!(
        application
            .environment_candidate_status(inserted.candidate_id())
            .status(),
        EnvironmentCandidateStatus::CancelledByShutdown
    );
}
