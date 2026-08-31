use super::*;

#[tokio::test]
async fn listener_start_holds_mutation_gate_through_fresh_description_and_runtime_start() {
    let (application, services, workspaces, _) = fixture();
    let application = Arc::new(application);
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let workspace = workspaces.get(selected.id).await.unwrap();
    let describe_calls_before = services.describe_calls.load(Ordering::SeqCst);
    services.block_describe.store(true, Ordering::SeqCst);

    let start_application = Arc::clone(&application);
    let start = tokio::spawn(async move {
        start_application
            .listener_start(workspace.id, workspace.revision.get(), listener_id)
            .await
    });
    services.describe_entered.notified().await;

    let mutation_application = Arc::clone(&application);
    let mutation_package = package.clone();
    let mut mutation = tokio::spawn(async move {
        mutation_application
            .protocol_rule_save(input(
                listener_id,
                mutation_package,
                ProtocolDirection::Upstream,
                1,
            ))
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut mutation)
            .await
            .is_err(),
        "rule mutation must wait while start owns the shared mutation gate"
    );

    services.continue_describe.notify_one();
    start.await.unwrap().unwrap();
    mutation.await.unwrap().unwrap();
    assert_eq!(
        services.describe_calls.load(Ordering::SeqCst),
        describe_calls_before + 2
    );
}
