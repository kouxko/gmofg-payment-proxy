use super::*;
use intercept_proxy_domain::MAX_JAVASCRIPT_SAFE_INTEGER;

#[tokio::test]
async fn compiler_description_identity_is_checked_for_capability_and_save() {
    let (application, services, workspaces, _) = fixture();
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
    services.set_description(
        package.clone(),
        description_with_blob(pkg("other", "2.0.0")),
    );
    assert_eq!(
        error_code(
            &application
                .socket_rule_capabilities(listener_id, SocketDirection::Upstream)
                .await
                .unwrap_err()
        ),
        "PROTOCOL_PACKAGE_DESCRIPTION_IDENTITY_MISMATCH"
    );
    assert_eq!(
        error_code(
            &application
                .socket_rule_save(input(listener_id, package, SocketDirection::Upstream, 0))
                .await
                .unwrap_err()
        ),
        "PROTOCOL_PACKAGE_DESCRIPTION_IDENTITY_MISMATCH"
    );
}

#[tokio::test]
async fn socket_rule_writes_share_the_existing_rule_lifecycle_guard() {
    let services = Arc::new(FakeProtocolPackageServices::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let runtime = Arc::new(InMemoryListenerRuntime::default());
    let ports = Arc::new(FakePorts::default());
    let application =
        application_with_proxy_ports(services.clone(), workspaces.clone(), runtime, ports.clone());
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
    *ports.proxy_state.lock() = ProxyState::Starting;

    let error = application
        .socket_rule_save(input(listener_id, package, SocketDirection::Upstream, 0))
        .await
        .unwrap_err();

    assert_eq!(error_code(&error), "OPERATION_IN_PROGRESS");
    assert_eq!(services.describe_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn v3_workspace_and_configuration_exports_reject_socket_rules() {
    let (application, services, workspaces, _) = fixture();
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
    application
        .socket_rule_save(input(listener_id, package, SocketDirection::Upstream, 0))
        .await
        .unwrap();
    let selected = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.selected)
        .unwrap();

    let workspace_error = application.workspace_export(selected.id).await.unwrap_err();
    assert_eq!(
        error_code(&workspace_error),
        "SOCKET_RULE_PORTABILITY_REQUIRES_V4"
    );
    let configuration_error = application
        .application_configuration_export()
        .await
        .unwrap_err();
    assert_eq!(
        error_code(&configuration_error),
        "SOCKET_RULE_PORTABILITY_REQUIRES_V4"
    );
}

#[tokio::test]
async fn deleting_an_imported_high_order_rule_never_reuses_its_created_order() {
    let (application, services, workspaces, _) = fixture();
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
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let imported = SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::new(),
        true,
        0,
        10_000,
        listener_id,
        package.clone(),
        1,
        SocketDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    workspace.socket_rule_created_order_high_water = imported.created_order();
    workspace.socket_rules.push(imported.clone());
    workspaces.save(workspace).await.unwrap();

    application
        .socket_rule_delete(imported.rule_id(), imported.revision().get(), true)
        .await
        .unwrap();
    let created = application
        .socket_rule_save(input(listener_id, package, SocketDirection::Upstream, 0))
        .await
        .unwrap();

    assert!(created.created_order() > imported.created_order());
}

#[tokio::test]
async fn created_order_exhaustion_is_stable_and_does_not_write() {
    let (application, services, workspaces, _) = fixture();
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
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    workspace.socket_rule_created_order_high_water = MAX_JAVASCRIPT_SAFE_INTEGER;
    let before = workspaces.save(workspace).await.unwrap();

    let error = application
        .socket_rule_save(input(listener_id, package, SocketDirection::Upstream, 0))
        .await
        .unwrap_err();

    assert_eq!(error_code(&error), "SOCKET_RULE_CREATED_ORDER_EXHAUSTED");
    let after = workspaces.get(before.id).await.unwrap();
    assert_eq!(after.revision, before.revision);
    assert!(after.socket_rules.is_empty());
    assert_eq!(
        after.socket_rule_created_order_high_water,
        MAX_JAVASCRIPT_SAFE_INTEGER
    );
}
