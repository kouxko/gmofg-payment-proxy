use super::*;

mod application_backup_export;
mod application_backup_import;
mod legacy_protocol_portability;
mod portability_atomicity;
mod protocol_package_portability;
mod rule_round_trip;
mod socket_portability;

#[derive(Debug, Default)]
struct RecordingConfigurationStore {
    document: parking_lot::Mutex<Option<ApplicationConfigurationDocument>>,
}

#[async_trait]
impl ApplicationConfigurationStorePort for RecordingConfigurationStore {
    async fn replace_all(&self, document: ApplicationConfigurationDocument) -> AppResult<()> {
        *self.document.lock() = Some(document);
        Ok(())
    }
}

#[tokio::test]
async fn bootstrap_uses_non_secret_certificate_status_instead_of_full_overview() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());

    application
        .app_bootstrap()
        .await
        .expect("bootstrap snapshot");

    assert_eq!(ports.certificate_status_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_overview_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn workspace_facade_exposes_complete_headless_crud_document_and_event_flow() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let application =
        application_with_workspace_ports(ports, Arc::clone(&workspaces), Arc::clone(&documents));
    let mut events = application.app_subscribe_events(0).unwrap();

    let created = application.workspace_create("Lab".into()).await.unwrap();
    assert_eq!(application.workspace_list().await.unwrap().len(), 1);
    assert_eq!(
        application.workspace_get(created.id).await.unwrap(),
        created
    );
    let created_event = events.live.recv().await.unwrap();
    assert!(matches!(
        created_event.payload,
        UiEventPayload::WorkspaceChanged(WorkspaceChangedViewModel {
            kind: WorkspaceChangeKind::Created,
            ..
        })
    ));

    let mut invalid = created.clone();
    invalid.name.clear();
    assert!(!application.workspace_validate(invalid).await.unwrap().valid);
    let mut edited = created.clone();
    edited.name = "Lab Updated".into();
    let saved = application.workspace_save(edited).await.unwrap();
    assert_eq!(saved.revision.get(), 2);
    let copied = application.workspace_copy(saved.id).await.unwrap();
    application.workspace_select(copied.id).await.unwrap();

    application.workspace_export(saved.id).await.unwrap();
    let (file_name, exported) = documents.take_last_export().unwrap();
    assert_eq!(file_name, "Lab_Updated.intercept-workspace");
    documents.set_next_import(exported);
    let imported = application.workspace_import().await.unwrap();
    assert!(imported.success);
    assert_eq!(application.workspace_list().await.unwrap().len(), 3);

    application
        .workspace_delete(copied.id, copied.revision.get())
        .await
        .unwrap();
    assert!(application.workspace_get(copied.id).await.is_err());
}

#[tokio::test]
async fn full_configuration_export_keeps_reachable_listener_material_and_drops_orphans() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let root_id = CertificateReferenceId::new();
    workspace.certificate_references.push(CertificateReference {
        id: root_id,
        label: "本机 MITM Root CA".into(),
        kind: CertificateReferenceKind::MitmRootCa,
        reference: INSTALLATION_ROOT_CERTIFICATE_REFERENCE.into(),
    });
    let server_identity = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "Listener 服务端身份".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}server-identity"),
    };
    let client_trust = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "下游客户端 CA".into(),
        kind: CertificateReferenceKind::DownstreamClientTrust,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}client-trust"),
    };
    let server_trust = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "上游服务端 CA".into(),
        kind: CertificateReferenceKind::UpstreamServerTrust,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}server-trust"),
    };
    let client_identity = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "上游 P12".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}client-identity"),
    };
    let orphan = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "已丢失的旧临时证书".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: "file:/tmp/deleted-listener-identity.pem".into(),
    };
    workspace.certificate_references.extend([
        server_identity.clone(),
        client_trust.clone(),
        server_trust.clone(),
        client_identity.clone(),
        orphan,
    ]);
    let http = workspace.listeners[0].http_mut().unwrap();
    http.mitm.root_ca = Some(root_id);
    http.downstream_tls.enabled = true;
    http.downstream_tls.server_identity = Some(server_identity.id);
    http.downstream_tls.client_authentication = DownstreamClientAuthentication::Required {
        trust: client_trust.id,
    };
    http.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://upstream.test".into(),
        upstream_tls: UpstreamTlsSettings {
            verify_hostname: true,
            server_trust: Some(server_trust.id),
            client_identity: Some(client_identity.id),
        },
    });
    workspaces.save(workspace).await.unwrap();
    let application = application_with_workspace_ports(ports, workspaces, Arc::clone(&documents));

    application
        .application_configuration_export()
        .await
        .unwrap();

    let (_, bytes) = documents.take_last_export().unwrap();
    let exported = parse_application_configuration(&bytes).unwrap();
    assert_eq!(exported.certificate_materials.len(), 4);
    assert_eq!(
        exported
            .certificate_materials
            .iter()
            .find(|material| material.reference_id == client_identity.id)
            .unwrap()
            .password
            .as_deref(),
        Some("test-password")
    );
    assert_eq!(exported.workspaces[0].certificate_references.len(), 5);
    assert!(
        exported.workspaces[0]
            .certificate_references
            .iter()
            .any(|reference| reference.reference == INSTALLATION_ROOT_CERTIFICATE_REFERENCE)
    );
    assert!(
        exported.workspaces[0]
            .certificate_references
            .iter()
            .all(|reference| !reference.reference.starts_with("file:"))
    );
}

#[tokio::test]
async fn application_data_reset_requires_confirmation_and_replaces_everything_with_defaults() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let configuration_store = Arc::new(RecordingConfigurationStore::default());
    let application = application_with_configuration_store(
        ports,
        workspaces,
        documents,
        configuration_store.clone(),
    );

    let error = application
        .application_data_reset(false)
        .await
        .expect_err("destructive reset must require explicit confirmation");
    assert_eq!(error.view_model.code, "CONFIRMATION_REQUIRED");
    assert!(configuration_store.document.lock().is_none());

    let result = application
        .application_data_reset(true)
        .await
        .expect("confirmed reset");
    assert!(result.success);
    assert!(result.requires_restart);

    let document = configuration_store
        .document
        .lock()
        .clone()
        .expect("clean document recorded");
    assert_eq!(document.workspaces.len(), 1);
    assert_eq!(document.workspaces[0].name, "Default Workspace");
    assert_eq!(document.selected_workspace_id, document.workspaces[0].id);
    assert_eq!(
        document.settings,
        PortableSettings::from(&SettingsDraft::default())
    );
    assert!(document.certificate_materials.is_empty());
}

#[tokio::test]
async fn workspace_save_rejects_direct_certificate_reference_mutation() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let application = application_with_workspace_ports(ports, workspaces.clone(), documents);
    let selected_id = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.selected)
        .expect("selected workspace")
        .id;
    let mut edited = workspaces.get(selected_id).await.unwrap();
    edited.certificate_references.push(CertificateReference {
        id: CertificateReferenceId::new(),
        label: "绕过原生导入的证书".into(),
        kind: CertificateReferenceKind::UpstreamServerTrust,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}{}", Uuid::new_v4()),
    });

    let error = application
        .workspace_save(edited)
        .await
        .expect_err("aggregate save must not create managed certificate references");

    assert_eq!(
        error.view_model.code,
        "WORKSPACE_CERTIFICATE_IMPORT_REQUIRED"
    );
}

#[tokio::test]
async fn workspace_import_preserves_managed_certificate_metadata_for_cross_machine_restore() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let application =
        application_with_workspace_ports(ports.clone(), workspaces.clone(), documents.clone());
    let reference_value = format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}{}", Uuid::new_v4());
    ports
        .discarded_certificate_references
        .lock()
        .insert(reference_value.clone());
    let mut workspace = ProxyWorkspace::default();
    workspace.certificate_references.push(CertificateReference {
        id: CertificateReferenceId::new(),
        label: "不存在的托管 CA".into(),
        kind: CertificateReferenceKind::UpstreamServerTrust,
        reference: reference_value,
    });
    let materials = vec![
        ports
            .export_portable(workspace.certificate_references[0].clone())
            .await
            .unwrap(),
    ];
    documents.set_next_import(
        serialize_workspace_document(&WorkspaceDocument {
            format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
            workspace,
            certificate_materials: materials,
            protocol_packages: Vec::new(),
        })
        .unwrap(),
    );

    application
        .workspace_import()
        .await
        .expect("portable metadata may be restored before local secret material");

    let summaries = workspaces.list().await.unwrap();
    assert_eq!(summaries.len(), 1);
    let imported = workspaces.get(summaries[0].id).await.unwrap();
    assert_eq!(imported.certificate_references.len(), 1);

    let mut renamed = imported;
    renamed.name = "跨机恢复配置".into();
    application
        .workspace_save(renamed)
        .await
        .expect("editing portable metadata must not require local certificate material");
}

#[tokio::test]
async fn full_configuration_import_preserves_managed_certificate_metadata() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let configuration_store = Arc::new(RecordingConfigurationStore::default());
    let application = application_with_configuration_store(
        ports.clone(),
        workspaces,
        documents.clone(),
        configuration_store.clone(),
    );
    let reference_value = format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}{}", Uuid::new_v4());
    ports
        .discarded_certificate_references
        .lock()
        .insert(reference_value.clone());
    let mut workspace = ProxyWorkspace::default();
    workspace.certificate_references.push(CertificateReference {
        id: CertificateReferenceId::new(),
        label: "不存在的托管身份".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: reference_value,
    });
    let document = ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: workspace.id,
        workspaces: vec![workspace.clone()],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: vec![
            ports
                .export_portable(workspace.certificate_references[0].clone())
                .await
                .unwrap(),
        ],
        protocol_packages: Vec::new(),
    };
    documents.set_next_import(serialize_application_configuration(&document).unwrap());

    application
        .application_configuration_import()
        .await
        .expect("portable metadata may be restored before local secret material");

    let imported = configuration_store
        .document
        .lock()
        .clone()
        .expect("replacement document recorded");
    assert_eq!(imported.workspaces[0].certificate_references.len(), 1);
}

#[tokio::test]
async fn full_configuration_import_reports_old_certificate_cleanup_as_success_warning() {
    let ports = Arc::new(FakePorts::default());
    ports.fail_certificate_discard.store(true, Ordering::SeqCst);
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let old_summary = workspaces.list().await.unwrap().remove(0);
    let mut old_workspace = workspaces.get(old_summary.id).await.unwrap();
    old_workspace
        .certificate_references
        .push(CertificateReference {
            id: CertificateReferenceId::new(),
            label: "待清理旧 CA".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}old-ca"),
        });
    workspaces.save(old_workspace).await.unwrap();

    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let configuration_store = Arc::new(RecordingConfigurationStore::default());
    let application = application_with_configuration_store(
        ports,
        workspaces,
        documents.clone(),
        configuration_store.clone(),
    );
    let mut events = application.app_subscribe_events(0).unwrap();
    let replacement = ProxyWorkspace::default();
    documents.set_next_import(
        serialize_application_configuration(&ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: replacement.id,
            workspaces: vec![replacement],
            settings: PortableSettings::from(&SettingsDraft::default()),
            certificate_materials: Vec::new(),
            protocol_packages: Vec::new(),
        })
        .unwrap(),
    );

    let result = application
        .application_configuration_import()
        .await
        .expect("committed replacement must remain a successful import");

    assert!(result.success);
    assert_eq!(result.ui_tone, UiTone::Warning);
    assert!(configuration_store.document.lock().is_some());
    let warning = events.live.recv().await.unwrap();
    assert!(matches!(
        warning.payload,
        UiEventPayload::ResourceWarning { ref message }
            if message.contains("旧证书材料未全部清理")
    ));
}

#[tokio::test]
async fn workspace_rejects_arbitrary_certificate_path_reference_creation() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();

    let error = application
        .workspace_component_new(workspace, "certificate_reference")
        .expect_err("certificate references must be created by a managed import");

    assert_eq!(
        error.view_model.code,
        "WORKSPACE_CERTIFICATE_IMPORT_REQUIRED"
    );
}
