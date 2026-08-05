use super::*;

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
    documents.set_next_import(serialize_workspace_document(&workspace).unwrap());

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
        workspaces: vec![workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
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
