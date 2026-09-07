use super::*;

#[tokio::test]
async fn running_workspace_listener_blocks_configuration_save_and_delete() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            workspaces,
            listener_runtime: listener_runtime.clone(),
            protocol_packages: unused_protocol_package_services(),
            events: Arc::new(EventHub::default()),
            environment_baseline_capture: test_environment_baseline_capture(),
            environment_identity_allocator: test_environment_identity_allocator(),
            environment_apply_lease: test_environment_apply_lease(),
            environment_material_preparer: test_environment_material_preparer(),
            environment_commit: test_environment_commit(),
            environment_validator: test_environment_validator(),
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    );
    let workspace = application.workspace_create("Live".into()).await.unwrap();
    let listener = workspace.listeners[0].clone();
    listener_runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();

    let mut edited = workspace.clone();
    edited.listeners[0].port = edited.listeners[0].port.saturating_add(1);
    let save_error = application
        .workspace_save(edited)
        .await
        .expect_err("live workspace save rejected");
    assert_eq!(save_error.view_model.code, "WORKSPACE_RUNTIME_ACTIVE");
    let delete_error = application
        .workspace_delete(workspace.id, workspace.revision.get())
        .await
        .expect_err("live workspace delete rejected");
    assert_eq!(delete_error.view_model.code, "WORKSPACE_RUNTIME_ACTIVE");
    let listener_save_error = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            workspace.certificate_references.clone(),
        )
        .await
        .expect_err("live listener save rejected");
    assert_eq!(
        listener_save_error.view_model.code,
        "LISTENER_RUNTIME_ACTIVE"
    );
    let listener_delete_error = application
        .listener_delete(workspace.id, workspace.revision.get(), listener.id())
        .await
        .expect_err("live listener delete rejected");
    assert_eq!(
        listener_delete_error.view_model.code,
        "LISTENER_RUNTIME_ACTIVE"
    );
}

#[tokio::test]
async fn running_workspace_listener_allows_saving_and_starting_a_stopped_listener() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            workspaces,
            listener_runtime: listener_runtime.clone(),
            protocol_packages: unused_protocol_package_services(),
            events: Arc::new(EventHub::default()),
            environment_baseline_capture: test_environment_baseline_capture(),
            environment_identity_allocator: test_environment_identity_allocator(),
            environment_apply_lease: test_environment_apply_lease(),
            environment_material_preparer: test_environment_material_preparer(),
            environment_commit: test_environment_commit(),
            environment_validator: test_environment_validator(),
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    );
    let mut workspace = application.workspace_create("Live".into()).await.unwrap();
    let running_listener = workspace.listeners[0].clone();
    let mut stopped_listener = application.listener_copy(running_listener.clone()).unwrap();
    stopped_listener.name = "Second listener".into();
    // 使用与产品无关的测试端口偏移，避免回归测试重新引入已删除的业务端口契约。
    stopped_listener.port = running_listener.port.saturating_add(173);
    workspace.listeners.push(stopped_listener.clone());
    workspace = application.workspace_save(workspace).await.unwrap();

    application
        .listener_start(
            workspace.id,
            workspace.revision.get(),
            running_listener.id(),
        )
        .await
        .unwrap();
    workspace = application.workspace_get(workspace.id).await.unwrap();

    workspace.listeners[1].name = "Second listener edited while first runs".into();
    let edited_listener = workspace.listeners[1].clone();
    let saved = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            edited_listener,
            workspace.certificate_references.clone(),
        )
        .await
        .expect("stopped listener can be saved while another listener runs");
    let second_status = application
        .listener_start(saved.id, saved.revision.get(), stopped_listener.id())
        .await
        .expect("saved stopped listener can start while first listener remains running");

    assert_eq!(second_status.state, ListenerRuntimeState::Running);
    let statuses = application.listener_statuses().await.unwrap();
    assert_eq!(statuses.len(), 2);

    application
        .listener_stop(saved.id, saved.revision.get(), running_listener.id())
        .await
        .expect("stale aggregate revision must not block stopping a listener");
    let statuses = application.listener_statuses().await.unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].listener_id, stopped_listener.id());

    let latest = application.workspace_get(saved.id).await.unwrap();
    application
        .listener_delete(latest.id, latest.revision.get(), running_listener.id())
        .await
        .expect("a stopped listener can be deleted while another listener remains running");
    let latest = application.workspace_get(saved.id).await.unwrap();
    assert_eq!(latest.listeners.len(), 1);
    assert_eq!(latest.listeners[0].id, stopped_listener.id());
}

#[tokio::test]
async fn running_workspace_listener_allows_device_network_profile_persistence() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            workspaces,
            listener_runtime: listener_runtime.clone(),
            protocol_packages: unused_protocol_package_services(),
            events: Arc::new(EventHub::default()),
            environment_baseline_capture: test_environment_baseline_capture(),
            environment_identity_allocator: test_environment_identity_allocator(),
            environment_apply_lease: test_environment_apply_lease(),
            environment_material_preparer: test_environment_material_preparer(),
            environment_commit: test_environment_commit(),
            environment_validator: test_environment_validator(),
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    );
    let mut workspace = application.workspace_create("Live".into()).await.unwrap();
    let listener = workspace.listeners[0].clone();
    listener_runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();

    workspace
        .android_network_profiles
        .push(AndroidNetworkProfile {
            id: "vpn-profile".into(),
            name: "运行时可保存的设备网络方案".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "com.example.client".into(),
                display_name: None,
                uid: 10_001,
            }],
            destination_targets: Vec::new(),
            proxy_routes: Vec::new(),
            confirmed_shared_uids: BTreeSet::default(),
            auto_resume_after_reboot: false,
            stop_vpn_on_control_loss: true,
            weak_network: WeakNetworkProfile::default(),
        });
    let saved = application
        .workspace_save(workspace)
        .await
        .expect("VPN profile persistence must remain available while a listener runs");
    assert_eq!(saved.android_network_profiles.len(), 1);
    assert_eq!(
        listener_runtime.statuses().await.unwrap()[0].state,
        ListenerRuntimeState::Running,
        "saving an independent VPN profile must not restart the listener"
    );
}
