use super::*;

mod application_backup_export;
mod application_backup_import;

#[derive(Debug, Default)]
struct RecordingConfigurationStore {
    document: parking_lot::Mutex<Option<ApplicationConfigurationDocument>>,
    replace_calls: AtomicUsize,
    fail_replace: AtomicBool,
}

#[async_trait]
impl ApplicationConfigurationStorePort for RecordingConfigurationStore {
    async fn replace_all(&self, document: ApplicationConfigurationDocument) -> AppResult<()> {
        self.replace_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_replace.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "ATOMIC_COMMIT_FAILED",
                "测试注入：原子替换失败。",
            ));
        }
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
async fn workspace_facade_exposes_complete_headless_crud_and_event_flow() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let application = application_with_workspace_ports(ports, Arc::clone(&workspaces));
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

    application
        .workspace_delete(copied.id, copied.revision.get())
        .await
        .unwrap();
    assert!(application.workspace_get(copied.id).await.is_err());
}

#[tokio::test]
async fn application_data_reset_requires_confirmation_and_replaces_everything_with_defaults() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let configuration_store = Arc::new(RecordingConfigurationStore::default());
    let application =
        application_with_configuration_store(ports, workspaces, configuration_store.clone());

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
    let application = application_with_workspace_ports(ports, workspaces.clone());
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
