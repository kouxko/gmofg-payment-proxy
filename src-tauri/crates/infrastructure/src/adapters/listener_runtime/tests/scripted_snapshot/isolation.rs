use super::*;

#[tokio::test]
async fn frozen_snapshot_ignores_later_package_reinstall_and_workspace_edits() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let limits = ProtocolRuntimeLimits::new(88_888, 25, 32_769, 131_073, 126).unwrap();
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::new(
        Arc::clone(&store),
        intercept_proxy_protocol_scripting::ProtocolArchiveLimits::default(),
        limits,
    ));
    install_enabled(&repository);
    let listener = scripted_listener(SocketTopology::Relay(SocketRelayTopology {
        upstream: SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9_999,
        },
        security: SocketRelaySecurity::Transparent,
    }));
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rules: vec![rule(&listener, 20, 2), rule(&listener, 10, 1)],
        socket_rule_created_order_high_water: 2,
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository.clone());
    let snapshot = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap()
        .scripted_snapshot()
        .unwrap();
    let frozen_generation = snapshot.package().generation();

    repository.delete(&snapshot_package()).unwrap();
    repository
        .install_zip(&snapshot_zip(&format!(
            "{SNAPSHOT_SCRIPT}\n// replacement generation\n"
        )))
        .unwrap();
    repository.set_enabled(&snapshot_package(), true).unwrap();
    let replacement = repository
        .freeze_for_listener_start(&snapshot_package())
        .unwrap();
    workspace.socket_rules.clear();
    workspace.socket_rule_created_order_high_water = 0;
    workspace.listeners[0].name = "edited draft".into();

    assert_ne!(replacement.generation(), frozen_generation);
    assert!(!Arc::ptr_eq(
        snapshot.package().compiled(),
        replacement.compiled()
    ));
    assert_eq!(snapshot.package().generation(), frozen_generation);
    assert_eq!(snapshot.package().compiled().schema().version(), 7);
    assert_eq!(snapshot.runtime_limits(), limits);
    assert_eq!(
        snapshot
            .rules()
            .iter()
            .map(|item| (item.priority(), item.created_order()))
            .collect::<Vec<_>>(),
        vec![(10, 1), (20, 2)]
    );
    assert!(snapshot.certificate_references().is_empty());
    assert_eq!(snapshot.upstream().direction(), ProtocolDirection::Upstream);
    assert_eq!(
        snapshot.downstream().direction(),
        ProtocolDirection::Downstream
    );
}
