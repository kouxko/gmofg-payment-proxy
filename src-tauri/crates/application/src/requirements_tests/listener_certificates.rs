use super::*;

#[tokio::test]
async fn listener_save_rejects_unmanaged_file_and_pkcs12_references() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let listener = workspace.listeners[0].clone();

    for value in [
        "file:/tmp/untrusted-server.pem",
        "pkcs12:/tmp/untrusted-client.p12?password_env=PASSWORD",
    ] {
        let reference = CertificateReference {
            id: CertificateReferenceId::new(),
            label: "不得持久化的外部引用".into(),
            kind: CertificateReferenceKind::UpstreamClientIdentity,
            reference: value.into(),
        };
        let error = application
            .listener_save(
                workspace.id,
                workspace.revision.get(),
                listener.clone(),
                vec![reference],
            )
            .await
            .expect_err("listener save must reject certificate paths from IPC");

        assert_eq!(
            error.view_model.code,
            "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
        );
    }
}

#[tokio::test]
async fn listener_certificate_discard_rejects_references_used_by_any_workspace() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "正在使用的上游身份".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: "managed:listener-tls:in-use".into(),
    };
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://upstream.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings {
            client_identity: Some(reference.id),
            ..UpstreamTlsSettings::default()
        },
    });
    application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            listener,
            vec![reference.clone()],
        )
        .await
        .unwrap();

    let error = application
        .listener_certificate_discard(reference)
        .await
        .expect_err("a persisted reference must never be discarded");

    assert_eq!(error.view_model.code, "CERTIFICATE_REFERENCE_IN_USE");
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn listener_certificate_discard_cleans_an_unreferenced_import() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    application.workspace_create("Lab".into()).await.unwrap();
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "已放弃的上游身份".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: "managed:listener-tls:abandoned".into(),
    };

    let result = application
        .listener_certificate_discard(reference)
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn listener_certificate_discard_is_atomic_with_listener_save() {
    let ports = Arc::new(FakePorts::default());
    ports
        .block_certificate_discard
        .store(true, Ordering::SeqCst);
    let application = Arc::new(application_with_fake_ports(ports.clone()));
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "并发保存的上游身份".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: "managed:listener-tls:concurrent-save".into(),
    };
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://upstream.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings {
            client_identity: Some(reference.id),
            ..UpstreamTlsSettings::default()
        },
    });

    let discard = {
        let application = application.clone();
        let reference = reference.clone();
        tokio::spawn(async move { application.listener_certificate_discard(reference).await })
    };
    ports.certificate_discard_entered.notified().await;

    let mut save = {
        let application = application.clone();
        let reference = reference.clone();
        tokio::spawn(async move {
            application
                .listener_save(
                    workspace.id,
                    workspace.revision.get(),
                    listener,
                    vec![reference],
                )
                .await
        })
    };

    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut save)
            .await
            .is_err(),
        "listener save must wait until reference check and secret deletion finish"
    );

    ports.continue_certificate_discard.notify_one();
    discard.await.unwrap().unwrap();
    let error = save
        .await
        .unwrap()
        .expect_err("saving a discarded managed reference must fail inspection");

    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_MATERIAL_UNAVAILABLE"
    );
    assert!(
        application
            .workspace_get(workspace.id)
            .await
            .unwrap()
            .certificate_references
            .is_empty()
    );
}

#[tokio::test]
async fn listener_tls_test_rejects_unmanaged_file_and_pkcs12_references() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://upstream.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings::default(),
    });

    for value in [
        "file:/tmp/untrusted-server.pem",
        "pkcs12:/tmp/untrusted-client.p12?password_env=PASSWORD",
    ] {
        let reference = CertificateReference {
            id: CertificateReferenceId::new(),
            label: "不得用于握手测试的外部引用".into(),
            kind: CertificateReferenceKind::UpstreamClientIdentity,
            reference: value.into(),
        };
        let error = application
            .listener_test_upstream_tls(
                workspace.id,
                workspace.revision.get(),
                listener.clone(),
                vec![reference],
            )
            .await
            .expect_err("TLS test must reject certificate paths from IPC");

        assert_eq!(
            error.view_model.code,
            "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
        );
    }
}

#[tokio::test]
async fn listener_tls_test_validates_the_persisted_workspace_candidate() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://".into(),
        upstream_tls: UpstreamTlsSettings::default(),
    });

    let error = application
        .listener_test_upstream_tls(workspace.id, workspace.revision.get(), listener, Vec::new())
        .await
        .expect_err("invalid listener drafts must fail before the runtime handshake");

    assert!(
        !error.view_model.field_errors.is_empty(),
        "domain validation must expose the invalid fixed Server URL"
    );
}

#[tokio::test]
async fn listener_validation_uses_persisted_other_listeners_not_unrelated_ui_drafts() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener.name = "当前有效监听草稿".into();

    let validation = application
        .listener_validate(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            workspace.certificate_references.clone(),
        )
        .await
        .expect("target listener validation should be available independently");

    assert!(validation.valid);
    assert_eq!(validation.normalized.listeners.len(), 1);
    assert_eq!(validation.normalized.listeners[0], listener);
}

#[tokio::test]
async fn running_workspace_listener_blocks_configuration_save_and_delete() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces,
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
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
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces,
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
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
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces,
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
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
