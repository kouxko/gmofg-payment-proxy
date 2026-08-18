use super::*;

#[tokio::test]
async fn successful_commit_replaces_exact_candidate_and_consumes_token_once() {
    let (application, portability, ports, store, _) = commit_application();
    let candidate = two_package_backup();
    register_packages(&portability, &candidate);
    let source = FakeBackupPrepareSource::new(candidate.clone());
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();

    let outcome = application
        .application_backup_import_commit(&source, preview.token)
        .await
        .unwrap();

    assert_eq!(outcome.workspace_count, 2);
    assert_eq!(outcome.protocol_package_count, 2);
    assert_eq!(outcome.enabled_protocol_package_count, 1);
    assert!(outcome.requires_restart);
    {
        let stored = store.document.lock();
        let stored = stored.as_ref().unwrap();
        assert_eq!(
            stored.selected_workspace_id,
            candidate.selected_workspace_id
        );
        assert_eq!(stored.workspaces, candidate.workspaces);
        assert_eq!(stored.settings, candidate.settings);
        assert_eq!(stored.protocol_packages, candidate.protocol_packages);
    }
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        application
            .application_backup_import_commit(&source, preview.token)
            .await
            .unwrap_err()
            .view_model
            .code,
        "APPLICATION_BACKUP_IMPORT_TOKEN_INVALID"
    );
}

#[tokio::test]
async fn every_authoritative_baseline_change_rejects_commit_before_writes() {
    for changed in 0..5 {
        let (application, portability, ports, store, workspaces) = commit_application();
        let alternate = if changed == 0 {
            Some(
                application
                    .workspace_create("alternate".into())
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        let candidate = two_package_backup();
        register_packages(&portability, &candidate);
        let source = FakeBackupPrepareSource::new(candidate);
        let preview = application
            .application_backup_import_prepare(&source, Vec::new())
            .await
            .unwrap();

        match changed {
            0 => {
                application
                    .workspace_select(alternate.unwrap().id)
                    .await
                    .unwrap();
            }
            1 => {
                application
                    .workspace_create("new workspace".into())
                    .await
                    .unwrap();
            }
            2 => ports.settings.lock().revision += 1,
            3 => portability.application_packages.lock()[0].enabled = false,
            4 => *ports.certificate_generation.lock() = [9; 32],
            _ => unreachable!(),
        }

        let error = application
            .application_backup_import_commit(&source, preview.token)
            .await
            .unwrap_err();

        assert_eq!(error.view_model.code, "APPLICATION_BACKUP_IMPORT_STALE");
        assert_eq!(store.replace_calls.load(Ordering::SeqCst), 0);
        assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
        assert!(store.document.lock().is_none());
        assert!(!workspaces.list().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn fresh_package_revalidation_failure_consumes_token_without_writes() {
    let (application, portability, _, store, _) = commit_application();
    let candidate = two_package_backup();
    register_packages(&portability, &candidate);
    let source = FakeBackupPrepareSource::new(candidate);
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();
    portability.fail_preflight.store(true, Ordering::SeqCst);

    let error = application
        .application_backup_import_commit(&source, preview.token)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "SCRIPT_SYNTAX_INVALID");
    assert_eq!(store.replace_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
    assert!(
        application
            .application_backup_import_commit(&source, preview.token)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn running_listener_blocks_commit_before_revalidation_or_writes() {
    let (application, portability, _, store, workspaces) = commit_application();
    let current = workspaces
        .get(workspaces.list().await.unwrap()[0].id)
        .await
        .unwrap();
    application
        .listener_start(current.id, current.revision.get(), current.listeners[0].id)
        .await
        .unwrap();
    let candidate = two_package_backup();
    register_packages(&portability, &candidate);
    let source = FakeBackupPrepareSource::new(candidate);
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();
    let preflight_calls = portability.preflight_calls.load(Ordering::SeqCst);

    let error = application
        .application_backup_import_commit(&source, preview.token)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "WORKSPACE_RUNTIME_ACTIVE");
    assert_eq!(
        portability.preflight_calls.load(Ordering::SeqCst),
        preflight_calls
    );
    assert_eq!(store.replace_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn atomic_store_failure_compensates_certificates_and_persists_nothing() {
    let (application, portability, ports, store, _) = commit_application();
    let candidate = certificate_backup(&ports).await;
    register_packages(&portability, &candidate);
    let source = FakeBackupPrepareSource::new(candidate);
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();
    store.fail_replace.store(true, Ordering::SeqCst);

    let error = application
        .application_backup_import_commit(&source, preview.token)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ATOMIC_COMMIT_FAILED");
    assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 1);
    assert!(store.document.lock().is_none());
}

#[tokio::test]
async fn fresh_rule_schema_revalidation_failure_writes_nothing() {
    let (application, portability, _, store, _) = commit_application();
    let candidate = two_package_backup();
    register_packages(&portability, &candidate);
    let source = FakeBackupPrepareSource::new(candidate.clone());
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();
    portability
        .descriptions
        .lock()
        .get_mut(&candidate.protocol_packages[0].package)
        .unwrap()
        .schema
        .version += 1;

    let error = application
        .application_backup_import_commit(&source, preview.token)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert_eq!(store.replace_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn fresh_certificate_revalidation_failure_restores_and_writes_nothing() {
    let (application, portability, ports, store, _) = commit_application();
    let candidate = certificate_backup(&ports).await;
    register_packages(&portability, &candidate);
    let source = FakeBackupPrepareSource::new(candidate);
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();
    source.retained.lock()[0].candidate.certificate_materials[0].material_base64 = "invalid".into();

    assert!(
        application
            .application_backup_import_commit(&source, preview.token)
            .await
            .is_err()
    );
    assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.replace_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn compensation_failure_reports_retryable_error_without_database_success() {
    let (application, portability, ports, store, _) = commit_application();
    let candidate = certificate_backup(&ports).await;
    register_packages(&portability, &candidate);
    let source = FakeBackupPrepareSource::new(candidate);
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();
    store.fail_replace.store(true, Ordering::SeqCst);
    ports.fail_certificate_discard.store(true, Ordering::SeqCst);

    let error = application
        .application_backup_import_commit(&source, preview.token)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ATOMIC_COMMIT_FAILED");
    assert!(error.view_model.retryable);
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 1);
    assert!(store.document.lock().is_none());
}

fn commit_application() -> (
    Application,
    Arc<FakeProtocolPackagePortability>,
    Arc<FakePorts>,
    Arc<RecordingConfigurationStore>,
    Arc<InMemoryWorkspaceStore>,
) {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let store = Arc::new(RecordingConfigurationStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports.clone(),
        workspaces.clone(),
        store.clone(),
    );
    (application, portability, ports, store, workspaces)
}
