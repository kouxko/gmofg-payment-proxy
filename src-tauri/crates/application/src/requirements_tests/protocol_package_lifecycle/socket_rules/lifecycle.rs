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

#[tokio::test]
async fn listener_save_and_validate_fresh_compile_disabled_exact_package_and_rules() {
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
    let mut disabled = services.record(&package).unwrap();
    disabled.enabled = false;
    services.insert(disabled);

    application
        .listener_validate(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    workspace = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 2);
    assert_eq!(services.describe_calls.load(Ordering::SeqCst), 2);

    let mut incompatible = description_with_blob(package.clone());
    incompatible.capabilities.upstream.encode = false;
    services.set_description(package.clone(), incompatible);
    let error = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_CAPABILITY_MISMATCH");
    assert!(
        error
            .view_model
            .field_errors
            .contains_key("listener.data_plane.socket.processing")
    );
    assert_eq!(
        workspaces.get(workspace.id).await.unwrap().revision,
        workspace.revision
    );

    services.set_description(package.clone(), description_with_blob(package.clone()));
    let rule = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            0,
        ))
        .await
        .unwrap();
    let mut wrong_schema = description_with_blob(package);
    wrong_schema.schema.version = rule.schema_version() + 1;
    services.set_description(wrong_schema.package.clone(), wrong_schema);
    let current = workspaces.get(workspace.id).await.unwrap();
    let error = application
        .listener_validate(
            current.id,
            current.revision.get(),
            current.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "RULE_INVALID");
}

#[tokio::test]
async fn listener_start_rechecks_revision_enabled_and_fresh_compilation() {
    let (application, services, workspaces, runtime) = fixture();
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
    let get_calls_before_stale_start = services.get_calls.load(Ordering::SeqCst);

    let stale = application
        .listener_start(
            workspace.id,
            workspace.revision.get().saturating_sub(1),
            listener_id,
        )
        .await
        .unwrap_err();
    assert_eq!(error_code(&stale), "REVISION_CONFLICT");
    assert_eq!(
        services.get_calls.load(Ordering::SeqCst),
        get_calls_before_stale_start
    );

    let mut disabled = services.record(&package).unwrap();
    disabled.enabled = false;
    services.insert(disabled);
    let error = application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_DISABLED");
    assert!(runtime.statuses().await.unwrap().is_empty());

    services.insert(record(package.clone(), true));
    services.set_description(package.clone(), description_with_blob(package.clone()));
    services.set_compilation_result(
        package.clone(),
        Ok(ProtocolPackageCompilationReceipt {
            package: package.clone(),
            host_api: 2,
            compatible: true,
        }),
    );
    let error = application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_API_INCOMPATIBLE");
    assert!(runtime.statuses().await.unwrap().is_empty());
    assert!(!workspaces.get(workspace.id).await.unwrap().listeners[0].enabled);
}

#[tokio::test]
async fn active_scripted_listener_freezes_listener_rules_and_package() {
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
    let workspace = workspaces.get(selected.id).await.unwrap();
    let rule = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            0,
        ))
        .await
        .unwrap();
    let workspace = workspaces.get(workspace.id).await.unwrap();

    application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap();
    let running = workspaces.get(workspace.id).await.unwrap();
    let mut edited = running.listeners[0].clone();
    edited.name.push_str(" changed");
    assert_eq!(
        error_code(
            &application
                .listener_save(running.id, running.revision.get(), edited, Vec::new(),)
                .await
                .unwrap_err()
        ),
        "LISTENER_RUNTIME_ACTIVE"
    );

    let save_error = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            1,
        ))
        .await
        .unwrap_err();
    assert_eq!(error_code(&save_error), "WORKSPACE_RUNTIME_ACTIVE");
    let toggle_error = application
        .socket_rule_toggle(rule.rule_id(), rule.revision().get(), false)
        .await
        .unwrap_err();
    assert_eq!(error_code(&toggle_error), "WORKSPACE_RUNTIME_ACTIVE");
    let delete_error = application
        .socket_rule_delete(rule.rule_id(), rule.revision().get(), true)
        .await
        .unwrap_err();
    assert_eq!(error_code(&delete_error), "WORKSPACE_RUNTIME_ACTIVE");
    services.set_usages(
        package.clone(),
        vec![usage(
            workspace.id,
            listener_id,
            ListenerRuntimeState::Running,
        )],
    );
    assert_eq!(
        error_code(
            &application
                .protocol_package_disable(package.clone())
                .await
                .unwrap_err()
        ),
        "PROTOCOL_PACKAGE_RUNTIME_IN_USE"
    );

    let running = workspaces.get(workspace.id).await.unwrap();
    application
        .listener_stop(running.id, running.revision.get(), listener_id)
        .await
        .unwrap();
    services.set_usages(
        package.clone(),
        vec![usage(
            workspace.id,
            listener_id,
            ListenerRuntimeState::Stopped,
        )],
    );
    application
        .socket_rule_toggle(rule.rule_id(), rule.revision().get(), false)
        .await
        .unwrap();
    application.protocol_package_disable(package).await.unwrap();
}
