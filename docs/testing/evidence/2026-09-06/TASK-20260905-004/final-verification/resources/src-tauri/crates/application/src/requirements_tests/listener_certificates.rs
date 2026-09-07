use super::*;

mod runtime_guards;

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
    listener
        .http_mut()
        .unwrap()
        .remote_server_mut()
        .expect("remote HTTP topology")
        .fixed_server = Some(FixedServerSettings {
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
    listener
        .http_mut()
        .unwrap()
        .remote_server_mut()
        .expect("remote HTTP topology")
        .fixed_server = Some(FixedServerSettings {
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
    listener
        .http_mut()
        .unwrap()
        .remote_server_mut()
        .expect("remote HTTP topology")
        .fixed_server = Some(FixedServerSettings {
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
    listener
        .http_mut()
        .unwrap()
        .remote_server_mut()
        .expect("remote HTTP topology")
        .fixed_server = Some(FixedServerSettings {
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
async fn listener_connection_test_supports_http_without_tls_evidence() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener
        .http_mut()
        .unwrap()
        .remote_server_mut()
        .expect("remote HTTP topology")
        .fixed_server = Some(FixedServerSettings {
        upstream_url: "http://upstream.example.test:8080".into(),
        upstream_tls: UpstreamTlsSettings::default(),
    });

    let result = application
        .listener_test_upstream_connection(
            workspace.id,
            workspace.revision.get(),
            listener,
            Vec::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.scheme, "http");
    assert_eq!(result.transport, "tcp");
    assert!(result.tls.is_none());
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
