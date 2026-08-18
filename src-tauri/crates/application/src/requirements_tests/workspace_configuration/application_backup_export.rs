use std::{collections::BTreeMap, path::Path, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;

#[derive(Debug, Default)]
struct RecordingBackupExport {
    snapshots: parking_lot::Mutex<Vec<ApplicationBackupExportSnapshot>>,
}

#[derive(Debug, Default)]
struct BlockingBackupExport {
    entered: AtomicUsize,
    entered_notify: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait]
impl ApplicationBackupExportPort for BlockingBackupExport {
    async fn write(
        &self,
        _: ApplicationBackupExportSnapshot,
    ) -> AppResult<ApplicationBackupExportOutcome> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.entered_notify.notify_waiters();
        self.release.notified().await;
        Ok(ApplicationBackupExportOutcome {
            bytes_written: 1,
            replaced_existing: false,
        })
    }
}

#[async_trait]
impl ApplicationBackupExportPort for RecordingBackupExport {
    async fn write(
        &self,
        snapshot: ApplicationBackupExportSnapshot,
    ) -> AppResult<ApplicationBackupExportOutcome> {
        self.snapshots.lock().push(snapshot);
        Ok(ApplicationBackupExportOutcome {
            bytes_written: 1,
            replaced_existing: false,
        })
    }
}

#[tokio::test]
async fn backup_snapshot_holds_mutation_gate_across_all_authoritative_reads() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports.clone(),
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
    );
    let application = Arc::new(application);
    let destination = Arc::new(RecordingBackupExport::default());
    portability
        .block_application_export
        .store(true, Ordering::SeqCst);

    let exporting = {
        let application = application.clone();
        let destination = destination.clone();
        tokio::spawn(async move { application.application_backup_export(&*destination).await })
    };
    portability.application_export_entered.notified().await;

    let (workspace_started, workspace_attempting) = tokio::sync::oneshot::channel();
    let workspace_mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            workspace_started.send(()).unwrap();
            application.workspace_create("after snapshot".into()).await
        })
    };
    let (settings_started, settings_attempting) = tokio::sync::oneshot::channel();
    let settings_mutation = {
        let application = application.clone();
        let mut draft = ports.settings.lock().stored.clone();
        draft.bind_address = "127.0.0.9".into();
        tokio::spawn(async move {
            settings_started.send(()).unwrap();
            application.settings_save(draft).await
        })
    };
    let exact_package = protocol_package("concurrent", "1.0.0");
    let (package_started, package_attempting) = tokio::sync::oneshot::channel();
    let package_mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            package_started.send(()).unwrap();
            application.protocol_package_enable(exact_package).await
        })
    };
    let discard = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "discard after snapshot".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}discard-after-snapshot"),
    };
    let (certificate_started, certificate_attempting) = tokio::sync::oneshot::channel();
    let certificate_mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            certificate_started.send(()).unwrap();
            application.listener_certificate_discard(discard).await
        })
    };

    workspace_attempting.await.unwrap();
    settings_attempting.await.unwrap();
    package_attempting.await.unwrap();
    certificate_attempting.await.unwrap();
    tokio::task::yield_now().await;
    assert!(!workspace_mutation.is_finished());
    assert!(!settings_mutation.is_finished());
    assert!(!package_mutation.is_finished());
    assert!(!certificate_mutation.is_finished());

    portability.continue_application_export.notify_one();
    exporting.await.unwrap().unwrap();
    workspace_mutation.await.unwrap().unwrap();
    settings_mutation.await.unwrap().unwrap();
    assert!(package_mutation.await.unwrap().is_err());
    certificate_mutation.await.unwrap().unwrap();

    let snapshots = destination.snapshots.lock();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].document.application.workspaces.len(), 1);
    assert_ne!(
        snapshots[0].document.application.settings.bind_address,
        "127.0.0.9"
    );
}

#[tokio::test]
async fn concurrent_exports_release_mutation_gate_before_destination_write() {
    let (application, _) = application_with_workspace_configuration_and_packages(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::default()),
        Arc::new(NoopApplicationConfigurationStore),
    );
    let application = Arc::new(application);
    let destination = Arc::new(BlockingBackupExport::default());

    let first_entered = destination.entered_notify.notified();
    let first = {
        let application = application.clone();
        let destination = destination.clone();
        tokio::spawn(async move { application.application_backup_export(&*destination).await })
    };
    first_entered.await;

    let second_entered = destination.entered_notify.notified();
    let second = {
        let application = application.clone();
        let destination = destination.clone();
        tokio::spawn(async move { application.application_backup_export(&*destination).await })
    };
    second_entered.await;
    assert_eq!(destination.entered.load(Ordering::SeqCst), 2);

    let (started, attempting) = tokio::sync::oneshot::channel();
    let mutation = {
        let application = application.clone();
        tokio::spawn(async move {
            started.send(()).unwrap();
            application
                .workspace_create("while writes are blocked".into())
                .await
        })
    };
    attempting.await.unwrap();
    tokio::task::yield_now().await;
    assert!(mutation.is_finished());
    mutation.await.unwrap().unwrap();

    destination.release.notify_waiters();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
}

#[tokio::test]
async fn backup_snapshot_includes_portable_configuration_and_raw_iso_package_files() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let root = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "installation root".into(),
        kind: CertificateReferenceKind::MitmRootCa,
        reference: INSTALLATION_ROOT_CERTIFICATE_REFERENCE.into(),
    };
    let identity = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "portable server identity".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}backup-server"),
    };
    workspace.certificate_references = vec![root.clone(), identity.clone()];
    let ListenerDataPlane::Http(listener) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    listener.mitm.root_ca = Some(root.id);
    listener.downstream_tls.enabled = true;
    listener.downstream_tls.server_identity = Some(identity.id);
    workspaces.save(workspace).await.unwrap();

    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports,
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
    );
    let exact_package = protocol_package("iso8583-standard", "1.0.0");
    let raw_files = iso_package_files();
    portability.register(
        PortableApplicationProtocolPackage {
            package: exact_package.clone(),
            enabled: true,
            files: raw_files
                .iter()
                .map(|(path, bytes)| PortableProtocolPackageFile {
                    path: path.clone(),
                    contents_base64: STANDARD.encode(bytes),
                })
                .collect(),
        },
        protocol_package_description(exact_package),
    );
    let destination = RecordingBackupExport::default();

    application
        .application_backup_export(&destination)
        .await
        .unwrap();

    let snapshots = destination.snapshots.lock();
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.document.protocol_packages.len(), 1);
    assert_eq!(snapshot.document.portable_materials.len(), 1);
    assert!(
        snapshot
            .document
            .portable_materials
            .iter()
            .all(|material| material.kind != CertificateReferenceKind::MitmRootCa)
    );
    for (path, expected) in raw_files {
        let archive_path =
            PortableArchivePath::new(format!("protocol-packages/iso8583-standard/1.0.0/{path}"))
                .unwrap();
        assert_eq!(snapshot.files.get(&archive_path), Some(&expected));
    }
    let json = serde_json::to_value(&snapshot.document).unwrap();
    let mut keys = Vec::new();
    collect_json_keys(&json, &mut keys);
    for forbidden in [
        "captures",
        "payloads",
        "session_history",
        "logs",
        "diagnostic_history",
        "runtime_state",
        "import_token",
        "sqlite_database",
        "mitm_root_private_key",
    ] {
        assert!(
            !keys.contains(&forbidden),
            "forbidden backup field {forbidden}"
        );
    }
    assert!(
        snapshot
            .files
            .values()
            .all(|bytes| bytes != b"root-private-key-secret")
    );
}

fn iso_package_files() -> BTreeMap<String, Vec<u8>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join("templates/socket-protocol/iso8583-standard");
    let mut files = BTreeMap::new();
    collect_files(&root, &root, &mut files);
    files
}

fn collect_files(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    let mut entries = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative, std::fs::read(path).unwrap());
        }
    }
}

fn collect_json_keys<'a>(value: &'a serde_json::Value, keys: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                keys.push(key);
                collect_json_keys(child, keys);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_json_keys(child, keys);
            }
        }
        _ => {}
    }
}
