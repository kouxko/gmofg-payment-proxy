use super::*;

#[tokio::test]
async fn listener_start_holds_mutation_gate_through_fresh_compile_and_runtime_start() {
    let (application, services, workspaces, _) = fixture();
    let application = Arc::new(application);
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(
        &services,
        &workspaces,
        &package,
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: true,
        },
        DirectionProcessingOptions::default(),
    )
    .await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let workspace = workspaces.get(selected.id).await.unwrap();
    services.block_compile.store(true, Ordering::SeqCst);

    let start_application = Arc::clone(&application);
    let start = tokio::spawn(async move {
        start_application
            .listener_start(workspace.id, workspace.revision.get(), listener_id)
            .await
    });
    services.compile_entered.notified().await;

    let mutation_application = Arc::clone(&application);
    let mutation_package = package.clone();
    let mut mutation = tokio::spawn(async move {
        mutation_application
            .socket_rule_save(input(
                listener_id,
                mutation_package,
                SocketDirection::Upstream,
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

    services.continue_compile.notify_one();
    start.await.unwrap().unwrap();
    let error = mutation.await.unwrap().unwrap_err();
    assert_eq!(error_code(&error), "WORKSPACE_RUNTIME_ACTIVE");
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 1);
}
