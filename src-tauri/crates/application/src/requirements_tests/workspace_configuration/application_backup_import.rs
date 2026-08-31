use std::{collections::BTreeMap, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;
mod commit;
mod source;
mod unified_portability;
use source::FakeBackupPrepareSource;

#[tokio::test]
async fn discard_facade_releases_the_exact_preview_token_without_authoritative_writes() {
    let (application, portability, ports, _) = prepared_application();
    let prepared = two_package_backup();
    register_packages(&portability, &prepared);
    let source = FakeBackupPrepareSource::new(prepared);
    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();

    application
        .application_backup_import_discard(&source, preview.token)
        .await
        .unwrap();

    assert_eq!(source.discard_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn baseline_capture_holds_one_gate_against_all_authoritative_mutation_facades() {
    let (application, portability, ports, _) = prepared_application();
    let application = Arc::new(application);
    let prepared = two_package_backup();
    register_packages(&portability, &prepared);
    portability
        .block_backup_baseline
        .store(true, Ordering::SeqCst);
    let source = Arc::new(FakeBackupPrepareSource::new(prepared));
    let prepare = {
        let application = application.clone();
        let source = source.clone();
        tokio::spawn(async move {
            application
                .application_backup_import_prepare(source.as_ref(), Vec::new())
                .await
        })
    };
    portability.backup_baseline_entered.notified().await;

    let (workspace_started, workspace_attempted) = tokio::sync::oneshot::channel();
    let workspace_mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            workspace_started.send(()).unwrap();
            application.workspace_create("blocked".into()).await
        })
    };
    let (settings_started, settings_attempted) = tokio::sync::oneshot::channel();
    let settings_mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            settings_started.send(()).unwrap();
            application.settings_save(valid_settings_draft()).await
        })
    };
    let (package_started, package_attempted) = tokio::sync::oneshot::channel();
    let package_mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            package_started.send(()).unwrap();
            application.protocol_package_restore_builtin().await
        })
    };
    let (certificate_started, certificate_attempted) = tokio::sync::oneshot::channel();
    let certificate_mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            certificate_started.send(()).unwrap();
            application
                .certificate_import_pkcs12("blocked".into())
                .await
        })
    };
    workspace_attempted.await.unwrap();
    settings_attempted.await.unwrap();
    package_attempted.await.unwrap();
    certificate_attempted.await.unwrap();
    tokio::task::yield_now().await;
    assert!(!workspace_mutation.is_finished());
    assert!(!settings_mutation.is_finished());
    assert!(!package_mutation.is_finished());
    assert!(!certificate_mutation.is_finished());

    portability.continue_backup_baseline.notify_one();
    prepare.await.unwrap().unwrap();
    workspace_mutation.await.unwrap().unwrap();
    settings_mutation.await.unwrap().unwrap();
    assert!(package_mutation.await.unwrap().is_err());
    certificate_mutation.await.unwrap().unwrap();
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_import_calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.retained.lock().len(), 1);
}

#[tokio::test]
async fn pure_preflight_does_not_hold_gate_and_baseline_observes_completed_mutation() {
    let (application, portability, _, _) = prepared_application();
    let application = Arc::new(application);
    let prepared = two_package_backup();
    register_packages(&portability, &prepared);
    portability.block_preflight.store(true, Ordering::SeqCst);
    let source = Arc::new(FakeBackupPrepareSource::new(prepared));
    let prepare = {
        let application = application.clone();
        let source = source.clone();
        tokio::spawn(async move {
            application
                .application_backup_import_prepare(source.as_ref(), Vec::new())
                .await
        })
    };
    portability.preflight_entered.notified().await;

    let created = application
        .workspace_create("during preflight".into())
        .await
        .unwrap();

    portability.continue_preflight.notify_one();
    prepare.await.unwrap().unwrap();
    assert!(
        source.retained.lock()[0]
            .baseline
            .workspaces
            .iter()
            .any(|workspace| workspace.workspace_id == created.id)
    );
}

#[tokio::test]
async fn preview_reports_exact_counts_identities_and_scope() {
    let (application, portability, ports, workspaces) = prepared_application();
    let prepared = two_package_backup();
    register_packages(&portability, &prepared);
    let source = FakeBackupPrepareSource::new(prepared.clone());

    let preview = application
        .application_backup_import_prepare(&source, b"ignored-by-fake".to_vec())
        .await
        .unwrap();

    assert_eq!(preview.expires_in_seconds, 300);
    assert_eq!(preview.workspace_count, 2);
    assert_eq!(preview.protocol_package_count, 2);
    assert_eq!(preview.enabled_protocol_package_count, 1);
    assert_eq!(preview.portable_material_count, 0);
    assert_eq!(
        preview
            .protocol_packages
            .iter()
            .map(|entry| (entry.package.id.as_str(), entry.enabled))
            .collect::<Vec<_>>(),
        [("alpha", true), ("omega", false)]
    );
    assert!(preview.replacement_scope.replaces_all_workspaces);
    assert!(preview.replacement_scope.replaces_selected_workspace);
    assert!(preview.replacement_scope.replaces_portable_settings);
    assert!(preview.replacement_scope.replaces_protocol_package_registry);
    assert_eq!(source.retain_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 0);
    let authoritative_workspaces = workspaces.list().await.unwrap();
    let retained = source.retained.lock();
    let baseline = &retained[0].baseline;
    assert_eq!(
        baseline.selected_workspace_id,
        authoritative_workspaces[0].id
    );
    assert_eq!(
        baseline.workspaces,
        authoritative_workspaces
            .iter()
            .map(|workspace| ApplicationBackupWorkspaceBaseline {
                workspace_id: workspace.id,
                revision: intercept_proxy_domain::Revision::new(workspace.revision),
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(baseline.settings_revision.get(), 1);
    assert_eq!(
        baseline
            .protocol_packages
            .iter()
            .map(|entry| (
                entry.package.id.as_str(),
                entry.package.version.as_str(),
                entry.enabled,
                entry.generation,
            ))
            .collect::<Vec<_>>(),
        [
            ("alpha", "1.0.0", true, uuid::Uuid::nil()),
            ("omega", "2.0.0", false, uuid::Uuid::nil()),
        ]
    );
    assert_eq!(baseline.listener_certificate_generation, [0; 32]);
}

#[tokio::test]
async fn first_and_last_package_compile_failures_retain_nothing_and_write_nothing() {
    for failure_index in [0, 1] {
        let (application, portability, ports, workspaces) = prepared_application();
        let prepared = two_package_backup();
        register_packages(&portability, &prepared);
        *portability.fail_preflight_at.lock() = Some(failure_index);
        let before_workspaces = workspaces.list().await.unwrap();
        let before_packages = portability.application_packages.lock().clone();
        let source = FakeBackupPrepareSource::new(prepared);

        let error = application
            .application_backup_import_prepare(&source, Vec::new())
            .await
            .unwrap_err();

        assert_eq!(error.view_model.code, "SCRIPT_SYNTAX_INVALID");
        assert_eq!(source.retain_calls.load(Ordering::SeqCst), 0);
        assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 0);
        assert_eq!(workspaces.list().await.unwrap(), before_workspaces);
        assert_eq!(*portability.application_packages.lock(), before_packages);
    }
}

#[tokio::test]
async fn schema_rule_mismatch_fails_before_token_retention_or_writes() {
    let (application, portability, ports, workspaces) = prepared_application();
    let prepared = two_package_backup();
    register_packages(&portability, &prepared);
    let first = prepared.protocol_packages[0].package.clone();
    {
        let mut descriptions = portability.descriptions.lock();
        let schema = &mut descriptions
            .get_mut(&first)
            .unwrap()
            .upstream_schema
            .as_mut()
            .unwrap()
            .root;
        let intercept_proxy_domain::DocumentSchemaNode::Object { properties, .. } = schema else {
            unreachable!()
        };
        properties.insert(
            "amount".into(),
            intercept_proxy_domain::DocumentSchemaNode::String { title: None },
        );
    }
    let before = workspaces.list().await.unwrap();
    let source = FakeBackupPrepareSource::new(prepared);

    let error = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert_eq!(source.retain_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 0);
    assert_eq!(workspaces.list().await.unwrap(), before);
}

#[tokio::test]
async fn invalid_settings_fail_before_certificate_preflight_or_retention() {
    let (application, portability, ports, _) = prepared_application();
    let prepared = two_package_backup();
    register_packages(&portability, &prepared);
    *ports.settings_validation_override.lock() = Some(FieldValidationViewModel {
        valid: false,
        field_errors: BTreeMap::from([("bind_address".into(), vec!["invalid".into()])]),
        warnings: Vec::new(),
    });
    let source = FakeBackupPrepareSource::new(prepared);

    let error = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "APPLICATION_BACKUP_IMPORT_INVALID");
    assert_eq!(source.retain_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.certificate_preflight_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn certificate_identity_kind_reference_and_material_errors_never_restore_or_retain() {
    for corruption in 0..4 {
        let (application, portability, ports, workspaces) = prepared_application();
        let mut prepared = certificate_backup(&ports).await;
        register_packages(&portability, &prepared);
        match corruption {
            0 => prepared.certificate_materials[0].reference_id = CertificateReferenceId::new(),
            1 => {
                prepared.certificate_materials[0].kind =
                    CertificateReferenceKind::UpstreamServerTrust;
            }
            2 => prepared.workspaces[0].certificate_references.clear(),
            3 => prepared.certificate_materials[0].material_base64 = "not-base64".into(),
            _ => unreachable!(),
        }
        let before = workspaces.list().await.unwrap();
        let source = FakeBackupPrepareSource::new(prepared);

        assert!(
            application
                .application_backup_import_prepare(&source, Vec::new())
                .await
                .is_err()
        );
        assert_eq!(source.retain_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ports.certificate_restore_calls.load(Ordering::SeqCst), 0);
        assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
        assert_eq!(workspaces.list().await.unwrap(), before);
    }
}

#[tokio::test]
async fn preview_and_debug_never_expose_scripts_certificate_bytes_passwords_paths_or_payloads() {
    let (application, portability, ports, _) = prepared_application();
    let mut prepared = certificate_backup(&ports).await;
    prepared.protocol_packages[0].files[0].contents_base64 =
        STANDARD.encode(b"script-secret-marker");
    prepared.certificate_materials[0].password = Some("password-secret-marker".into());
    register_packages(&portability, &prepared);
    let prepared_debug = format!("{prepared:?}");
    let source = FakeBackupPrepareSource::new(prepared);

    let preview = application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .unwrap();
    let serialized = serde_json::to_string(&preview).unwrap();
    let debug = format!("{preview:?}");

    for secret in [
        "script-secret-marker",
        "fake-test-certificate",
        "password-secret-marker",
        "manifest.json",
        "captured-payload-marker",
    ] {
        assert!(!prepared_debug.contains(secret));
        assert!(!serialized.contains(secret));
        assert!(!debug.contains(secret));
    }
}

fn prepared_application() -> (
    Application,
    Arc<FakeProtocolPackagePortability>,
    Arc<FakePorts>,
    Arc<InMemoryWorkspaceStore>,
) {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports.clone(),
        workspaces.clone(),
        Arc::new(NoopApplicationConfigurationStore),
    );
    (application, portability, ports, workspaces)
}

fn two_package_backup() -> ApplicationBackupImportCandidate {
    let alpha = protocol_package("alpha", "1.0.0");
    let omega = protocol_package("omega", "2.0.0");
    let first = scripted_workspace(alpha.clone(), false);
    let second = scripted_workspace(omega.clone(), true);
    ApplicationBackupImportCandidate {
        selected_workspace_id: first.id,
        workspaces: vec![first, second],
        settings: PortableSettings::from(&SettingsDraft::default()),
        protocol_packages: vec![
            portable_protocol_package(alpha, true),
            portable_protocol_package(omega, false),
        ],
        certificate_materials: Vec::new(),
    }
}

async fn certificate_backup(ports: &FakePorts) -> ApplicationBackupImportCandidate {
    let exact = protocol_package("cert-package", "1.0.0");
    let mut workspace = scripted_workspace(exact.clone(), false);
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "server identity".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}candidate"),
    };
    workspace.certificate_references.push(reference.clone());
    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    let SocketTopology::Relay(relay) = &mut settings.topology else {
        unreachable!()
    };
    relay.security = SocketRelaySecurity::TlsToTcp {
        downstream_tls: SocketDownstreamTlsSettings {
            server_identity: reference.id,
            client_authentication: DownstreamClientAuthentication::Disabled,
        },
    };
    let material = ports.export_portable(reference).await.unwrap();
    ApplicationBackupImportCandidate {
        selected_workspace_id: workspace.id,
        workspaces: vec![workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
        protocol_packages: vec![portable_protocol_package(exact, true)],
        certificate_materials: vec![material],
    }
}

fn register_packages(
    portability: &FakeProtocolPackagePortability,
    prepared: &ApplicationBackupImportCandidate,
) {
    for portable in &prepared.protocol_packages {
        portability.register(
            portable.clone(),
            protocol_package_description(portable.package.clone()),
        );
    }
}
