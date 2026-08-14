use super::*;

#[tokio::test]
async fn workspace_export_holds_mutation_gate_until_snapshot_bytes_are_complete() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let workspace_id = workspaces.list().await.unwrap()[0].id;
    let (application, portability) = application_with_workspace_configuration_and_packages(
        Arc::new(FakePorts::default()),
        workspaces,
        Arc::new(InMemoryWorkspaceDocumentStore::default()),
        Arc::new(UnavailableApplicationConfigurationStore),
    );
    let application = Arc::new(application);
    portability
        .block_workspace_export
        .store(true, Ordering::SeqCst);

    let export_application = application.clone();
    let export =
        tokio::spawn(async move { export_application.workspace_export(workspace_id).await });
    tokio::time::timeout(
        Duration::from_secs(1),
        portability.workspace_export_entered.notified(),
    )
    .await
    .expect("export must reach the controlled package snapshot");

    let mutation_application = application.clone();
    let mut mutation = tokio::spawn(async move {
        mutation_application
            .workspace_create("after snapshot".into())
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut mutation)
            .await
            .is_err(),
        "a mutation must wait while export is still constructing snapshot bytes"
    );

    portability.continue_workspace_export.notify_one();
    export.await.unwrap().unwrap();
    mutation.await.unwrap().unwrap();
}

#[tokio::test]
async fn application_export_holds_mutation_gate_until_snapshot_bytes_are_complete() {
    let (application, portability) = application_with_workspace_configuration_and_packages(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::default()),
        Arc::new(InMemoryWorkspaceDocumentStore::default()),
        Arc::new(UnavailableApplicationConfigurationStore),
    );
    let application = Arc::new(application);
    portability
        .block_application_export
        .store(true, Ordering::SeqCst);

    let export_application = application.clone();
    let export =
        tokio::spawn(async move { export_application.application_configuration_export().await });
    tokio::time::timeout(
        Duration::from_secs(1),
        portability.application_export_entered.notified(),
    )
    .await
    .expect("application export must reach the controlled registry snapshot");

    let mutation_application = application.clone();
    let mut mutation = tokio::spawn(async move {
        mutation_application
            .workspace_create("after application snapshot".into())
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut mutation)
            .await
            .is_err(),
        "a mutation must wait while full configuration snapshot bytes are incomplete"
    );

    portability.continue_application_export.notify_one();
    export.await.unwrap().unwrap();
    mutation.await.unwrap().unwrap();
}

fn portable_workspace_with_certificate(
    reference: &CertificateReference,
    material: PortableCertificateMaterial,
) -> WorkspaceDocument {
    let mut workspace = ProxyWorkspace::default();
    workspace.certificate_references.push(reference.clone());
    let ListenerDataPlane::Http(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    settings.downstream_tls.enabled = true;
    settings.downstream_tls.server_identity = Some(reference.id);
    workspace.validate().unwrap();
    WorkspaceDocument {
        format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace,
        certificate_materials: vec![material],
        protocol_packages: Vec::new(),
    }
}

#[tokio::test]
async fn atomic_commit_failure_rolls_back_restored_certificate_and_writes_no_workspace() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "portable identity".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}source"),
    };
    let material = ports.export_portable(reference.clone()).await.unwrap();
    documents.set_next_import(
        serialize_workspace_document(&portable_workspace_with_certificate(&reference, material))
            .unwrap(),
    );
    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports.clone(),
        workspaces.clone(),
        documents,
        Arc::new(UnavailableApplicationConfigurationStore),
    );
    portability.fail_commit.store(true, Ordering::SeqCst);

    assert_eq!(
        application
            .workspace_import()
            .await
            .unwrap_err()
            .view_model
            .code,
        "ATOMIC_COMMIT_FAILED"
    );
    assert_eq!(portability.commit_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 1);
    assert!(workspaces.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn application_reset_uses_bundle_port_and_clears_protocol_registry_snapshot() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let configuration_store = Arc::new(RecordingConfigurationStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports,
        workspaces,
        documents,
        configuration_store.clone(),
    );

    application.application_data_reset(true).await.unwrap();
    assert_eq!(portability.reset_calls.load(Ordering::SeqCst), 1);
    let reset = configuration_store.document.lock().clone().unwrap();
    assert!(reset.protocol_packages.is_empty());
    assert_eq!(reset.workspaces.len(), 1);
}

#[tokio::test]
async fn full_configuration_package_schema_and_topology_failures_never_replace_state() {
    use super::protocol_package_portability::{
        description, package, portable_package, scripted_workspace,
    };

    let package = package("atomic", "1.0.0");
    let first = ProxyWorkspace::default();
    let second = scripted_workspace(package.clone(), false);
    let portable = portable_package(package.clone(), true);
    let document = ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: first.id,
        workspaces: vec![first, second],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: Vec::new(),
        protocol_packages: vec![portable.clone()],
    };
    let valid_bytes = serialize_application_configuration(&document).unwrap();

    for (syntax_failure, wrong_schema, bytes) in [
        (true, false, valid_bytes.clone()),
        (false, true, valid_bytes.clone()),
        (false, false, forged_last_topology(&valid_bytes)),
    ] {
        let ports = Arc::new(FakePorts::default());
        let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
        documents.set_next_import(bytes);
        let store = Arc::new(RecordingConfigurationStore::default());
        let (application, portability) = application_with_workspace_configuration_and_packages(
            ports,
            Arc::new(InMemoryWorkspaceStore::default()),
            documents,
            store.clone(),
        );
        let mut compiled = description(package.clone());
        if wrong_schema {
            compiled.schema.fields[1].field_type = ProtocolPackageSchemaFieldTypeViewModel::String;
        }
        portability.register(portable.clone(), compiled);
        portability
            .fail_preflight
            .store(syntax_failure, Ordering::SeqCst);

        assert!(
            application
                .application_configuration_import()
                .await
                .is_err()
        );
        assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
        assert!(store.document.lock().is_none());
    }
}

#[tokio::test]
async fn legacy_atomic_replace_failure_rolls_back_restored_certificate() {
    let ports = Arc::new(FakePorts::default());
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "legacy identity".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}legacy"),
    };
    let material = ports.export_portable(reference.clone()).await.unwrap();
    let portable = portable_workspace_with_certificate(&reference, material);
    let mut value = serde_json::to_value(ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: portable.workspace.id,
        workspaces: vec![portable.workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: portable.certificate_materials,
        protocol_packages: Vec::new(),
    })
    .unwrap();
    value["format_version"] = serde_json::json!(APPLICATION_CONFIGURATION_V3_FORMAT_VERSION);
    value.as_object_mut().unwrap().remove("protocol_packages");
    value["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .remove("socket_rules");
    value["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .remove("socket_rule_created_order_high_water");
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    documents.set_next_import(serde_json::to_vec(&value).unwrap());
    let store = Arc::new(RecordingConfigurationStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports.clone(),
        Arc::new(InMemoryWorkspaceStore::default()),
        documents,
        store.clone(),
    );
    portability.fail_commit.store(true, Ordering::SeqCst);

    assert_eq!(
        application
            .application_configuration_import()
            .await
            .unwrap_err()
            .view_model
            .code,
        "ATOMIC_COMMIT_FAILED"
    );
    assert_eq!(portability.legacy_replace_calls.load(Ordering::SeqCst), 1);
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 1);
    assert!(store.document.lock().is_none());
}

fn forged_last_topology(valid: &[u8]) -> Vec<u8> {
    let mut value: serde_json::Value = serde_json::from_slice(valid).unwrap();
    value["workspaces"][1]["listeners"][0]["data_plane"]["settings"]["topology"]["kind"] =
        serde_json::json!("local_responder");
    serde_json::to_vec(&value).unwrap()
}
