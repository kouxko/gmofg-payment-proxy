//! TODO-EXTERNAL-PACKAGE-008：外部软件包应用生命周期的可执行需求。

use super::*;
mod support;

use support::*;

#[tokio::test]
async fn external_socket_package_exposes_rule_capabilities_and_accepts_rules() {
    let external = Arc::new(FakeExternalPackages::default());
    let package = package("external-rules", "1.0.0");
    external.records.lock().insert(
        package.clone(),
        external_record(package.clone(), true, true),
    );
    external
        .descriptions
        .lock()
        .insert(package.clone(), description(package.clone()));
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let selected = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|workspace| workspace.selected)
        .unwrap();
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let listener = &mut workspace.listeners[0];
    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: SocketTopology::LocalResponder(SocketLocalResponderTopology::default()),
        maximum_connections: 8,
        runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
        }),
    });
    let listener_id = listener.id;
    workspaces.save(workspace).await.unwrap();
    let application = fixture_with_workspaces(
        external,
        Arc::new(EmptyUsage),
        Arc::new(InMemoryListenerRuntime::default()),
        workspaces,
    );

    let capabilities = application
        .protocol_rule_capabilities(listener_id, ProtocolRuleStage::ProxyToUpstream)
        .await
        .unwrap();
    assert_eq!(capabilities.package, package);
    assert_eq!(capabilities.fields[0].name, "/amount");

    let saved = application
        .protocol_rule_save(ProtocolRuleSaveInput {
            rule_id: None,
            expected_revision: None,
            name: "外部包规则".into(),
            enabled: true,
            priority: 0,
            listener_id,
            package,
            stage: ProtocolRuleStage::ProxyToUpstream,
            conditions: vec![DocumentCondition::Equals {
                field: JsonPointer::property("amount"),
                value: DocumentValue::integer(0).expect("fixture number"),
            }],
            actions: vec![DocumentAction::RecordMatch],
        })
        .await
        .unwrap();
    assert_eq!(saved.name(), "外部包规则");
}

#[tokio::test]
async fn unified_queries_include_external_versions_and_catalog_requires_online_and_enabled() {
    let external = Arc::new(FakeExternalPackages::default());
    let online = package("external-online", "1.0.0");
    let offline = package("external-offline", "1.0.0");
    external
        .records
        .lock()
        .insert(online.clone(), external_record(online.clone(), true, true));
    external.records.lock().insert(
        offline.clone(),
        external_record(offline.clone(), true, false),
    );
    external
        .descriptions
        .lock()
        .insert(online.clone(), description(online.clone()));
    external
        .descriptions
        .lock()
        .insert(offline.clone(), description(offline.clone()));
    let application = fixture(
        external,
        Arc::new(EmptyUsage),
        Arc::new(InMemoryListenerRuntime::default()),
    );

    let groups = application.protocol_package_list().await.unwrap();
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| matches!(
        group.versions[0].source,
        ProtocolPackageSourceViewModel::External { .. }
    )));

    let detail = application.protocol_package_detail(offline).await.unwrap();
    assert!(matches!(
        detail.version.source,
        ProtocolPackageSourceViewModel::External { online: false }
    ));

    let catalog = application
        .listener_protocol_package_catalog()
        .await
        .unwrap();
    assert_eq!(catalog.options.len(), 1);
    assert_eq!(catalog.options[0].package, online);
    assert!(matches!(
        catalog.options[0].source,
        ProtocolPackageSourceViewModel::External { online: true }
    ));
}

#[tokio::test]
async fn external_enable_requires_online_description_without_invoking_rhai_compiler() {
    let external = Arc::new(FakeExternalPackages::default());
    let target = package("external", "1.0.0");
    external.records.lock().insert(
        target.clone(),
        external_record(target.clone(), false, false),
    );
    external
        .descriptions
        .lock()
        .insert(target.clone(), description(target.clone()));
    let application = fixture(
        external.clone(),
        Arc::new(EmptyUsage),
        Arc::new(InMemoryListenerRuntime::default()),
    );

    let error = application
        .protocol_package_enable(target.clone())
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "EXTERNAL_PACKAGE_OFFLINE");
    assert_eq!(external.set_enabled_calls.load(Ordering::SeqCst), 0);

    external
        .records
        .lock()
        .insert(target.clone(), external_record(target.clone(), false, true));
    let enabled = application.protocol_package_enable(target).await.unwrap();
    assert!(enabled.enabled);
    assert_eq!(external.set_enabled_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn external_disable_stops_all_active_exact_references_and_preserves_connection() {
    let external = Arc::new(FakeExternalPackages::default());
    let target = package("external", "1.0.0");
    external
        .records
        .lock()
        .insert(target.clone(), external_record(target.clone(), true, true));
    let running = ListenerId::new();
    let faulted = ListenerId::new();
    let stopped = ListenerId::new();
    let runtime = Arc::new(TrackingRuntime::default());
    let application = fixture(
        external.clone(),
        Arc::new(FixedUsage(vec![
            usage(running, ListenerRuntimeState::Running),
            usage(faulted, ListenerRuntimeState::Faulted),
            usage(stopped, ListenerRuntimeState::Stopped),
        ])),
        runtime.clone(),
    );

    let disabled = application.protocol_package_disable(target).await.unwrap();

    assert!(!disabled.enabled);
    assert_eq!(runtime.stopped.lock().as_slice(), &[running, faulted]);
    assert_eq!(external.set_enabled_calls.load(Ordering::SeqCst), 1);
    assert_eq!(external.disconnect_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn external_delete_rejects_every_saved_reference_without_disconnect_or_delete() {
    let external = Arc::new(FakeExternalPackages::default());
    let target = package("external", "1.0.0");
    external
        .records
        .lock()
        .insert(target.clone(), external_record(target.clone(), false, true));
    let application = fixture(
        external.clone(),
        Arc::new(FixedUsage(vec![usage(
            ListenerId::new(),
            ListenerRuntimeState::Stopped,
        )])),
        Arc::new(InMemoryListenerRuntime::default()),
    );

    let error = application
        .protocol_package_delete(target)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_REFERENCE_IN_USE");
    assert_eq!(external.disconnect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(external.delete_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn external_delete_rejects_any_reference_and_disconnects_online_unreferenced_version_first() {
    let external = Arc::new(FakeExternalPackages::default());
    let target = package("external", "1.0.0");
    external
        .records
        .lock()
        .insert(target.clone(), external_record(target.clone(), false, true));
    let application = fixture(
        external.clone(),
        Arc::new(EmptyUsage),
        Arc::new(InMemoryListenerRuntime::default()),
    );

    application
        .protocol_package_delete(target.clone())
        .await
        .unwrap();

    assert_eq!(external.disconnect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(external.delete_calls.load(Ordering::SeqCst), 1);
    assert!(!external.records.lock().contains_key(&target));
}
