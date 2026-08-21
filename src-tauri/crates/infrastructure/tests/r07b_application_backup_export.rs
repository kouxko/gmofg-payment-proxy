use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use intercept_proxy_application::{
    ApplicationBackupExportPort, ApplicationBackupExportSnapshot, PortableArchivePath,
    parse_application_backup_document,
};
use intercept_proxy_infrastructure::{
    ApplicationBackupArchive, ApplicationBackupFileExporter, ApplicationBackupFileSystem,
    ApplicationBackupTemporaryFile, build_application_backup_zip,
};
use parking_lot::Mutex;
use serde_json::json;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureStage {
    Create,
    Write,
    Flush,
    FileSync,
    Persist,
    ParentSync,
}

#[derive(Debug)]
struct FakeFileSystem {
    target_exists: bool,
    failure: Option<FailureStage>,
    target_bytes: Arc<Mutex<Vec<u8>>>,
    calls: Arc<Mutex<Vec<&'static str>>>,
    temporary_parents: Arc<Mutex<Vec<PathBuf>>>,
    live_temporaries: Arc<AtomicUsize>,
}

impl FakeFileSystem {
    fn new(target_exists: bool, failure: Option<FailureStage>) -> Self {
        Self {
            target_exists,
            failure,
            target_bytes: Arc::new(Mutex::new(b"old-target".to_vec())),
            calls: Arc::new(Mutex::new(Vec::new())),
            temporary_parents: Arc::new(Mutex::new(Vec::new())),
            live_temporaries: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ApplicationBackupFileSystem for FakeFileSystem {
    fn target_exists(&self, _: &Path) -> bool {
        self.calls.lock().push("exists");
        self.target_exists
    }

    fn create_sibling_temporary(
        &self,
        parent: &Path,
    ) -> io::Result<Box<dyn ApplicationBackupTemporaryFile>> {
        self.calls.lock().push("create");
        self.temporary_parents.lock().push(parent.to_path_buf());
        fail_if(self.failure, FailureStage::Create)?;
        self.live_temporaries.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeTemporaryFile {
            failure: self.failure,
            bytes: Vec::new(),
            target_bytes: self.target_bytes.clone(),
            calls: self.calls.clone(),
            live_temporaries: self.live_temporaries.clone(),
        }))
    }

    fn sync_parent(&self, _: &Path) -> io::Result<()> {
        self.calls.lock().push("parent_sync");
        fail_if(self.failure, FailureStage::ParentSync)
    }
}

#[derive(Debug)]
struct FakeTemporaryFile {
    failure: Option<FailureStage>,
    bytes: Vec<u8>,
    target_bytes: Arc<Mutex<Vec<u8>>>,
    calls: Arc<Mutex<Vec<&'static str>>>,
    live_temporaries: Arc<AtomicUsize>,
}

impl Drop for FakeTemporaryFile {
    fn drop(&mut self) {
        self.live_temporaries.fetch_sub(1, Ordering::SeqCst);
    }
}

impl ApplicationBackupTemporaryFile for FakeTemporaryFile {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.calls.lock().push("write");
        fail_if(self.failure, FailureStage::Write)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.calls.lock().push("flush");
        fail_if(self.failure, FailureStage::Flush)
    }

    fn sync_all(&mut self) -> io::Result<()> {
        self.calls.lock().push("file_sync");
        fail_if(self.failure, FailureStage::FileSync)
    }

    fn persist(mut self: Box<Self>, _: &Path, _: bool) -> io::Result<()> {
        self.calls.lock().push("persist");
        fail_if(self.failure, FailureStage::Persist)?;
        *self.target_bytes.lock() = std::mem::take(&mut self.bytes);
        Ok(())
    }
}

#[test]
fn deterministic_zip_has_fixed_sorted_structure_and_identical_bytes() {
    let snapshot = snapshot();

    let first = build_application_backup_zip(&snapshot).unwrap();
    let second = build_application_backup_zip(&snapshot).unwrap();

    assert_eq!(first, second);
    let parsed = ApplicationBackupArchive::read(&first).unwrap();
    assert_eq!(parsed.document, snapshot.document);
    assert_eq!(parsed.files, snapshot.files);
}

#[tokio::test]
async fn overwrite_decline_preserves_existing_target_without_opening_temporary() {
    let file_system = Arc::new(FakeFileSystem::new(true, None));
    let exporter = ApplicationBackupFileExporter::with_file_system(
        PathBuf::from("/safe/backup.zip"),
        false,
        file_system.clone(),
    );

    let error = exporter.write(snapshot()).await.unwrap_err();

    assert_eq!(
        error.view_model.code,
        "APPLICATION_BACKUP_EXPORT_TARGET_EXISTS"
    );
    assert_eq!(&*file_system.target_bytes.lock(), b"old-target");
    assert_eq!(&*file_system.calls.lock(), &["exists"]);
}

#[tokio::test]
async fn overwrite_confirmation_atomically_replaces_existing_target() {
    let file_system = Arc::new(FakeFileSystem::new(true, None));
    let exporter = ApplicationBackupFileExporter::with_file_system(
        PathBuf::from("/safe/backup.zip"),
        true,
        file_system.clone(),
    );

    let outcome = exporter.write(snapshot()).await.unwrap();

    assert!(outcome.replaced_existing);
    assert_ne!(&*file_system.target_bytes.lock(), b"old-target");
    assert_eq!(
        &*file_system.calls.lock(),
        &[
            "exists",
            "create",
            "write",
            "flush",
            "file_sync",
            "persist",
            "parent_sync"
        ]
    );
    assert_eq!(
        &*file_system.temporary_parents.lock(),
        &[PathBuf::from("/safe")]
    );
}

#[tokio::test]
async fn failures_before_atomic_replace_preserve_old_target() {
    for stage in [
        FailureStage::Create,
        FailureStage::Write,
        FailureStage::Flush,
        FailureStage::FileSync,
        FailureStage::Persist,
    ] {
        let file_system = Arc::new(FakeFileSystem::new(true, Some(stage)));
        let exporter = ApplicationBackupFileExporter::with_file_system(
            PathBuf::from("/safe/backup.zip"),
            true,
            file_system.clone(),
        );

        let error = exporter.write(snapshot()).await.unwrap_err();

        assert_eq!(error.view_model.code, "APPLICATION_BACKUP_EXPORT_FAILED");
        assert_eq!(
            &*file_system.target_bytes.lock(),
            b"old-target",
            "{stage:?}"
        );
        assert_eq!(file_system.live_temporaries.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn parent_sync_failure_reports_failure_without_claiming_old_target_preservation() {
    let file_system = Arc::new(FakeFileSystem::new(true, Some(FailureStage::ParentSync)));
    let exporter = ApplicationBackupFileExporter::with_file_system(
        PathBuf::from("/safe/backup.zip"),
        true,
        file_system.clone(),
    );

    let error = exporter.write(snapshot()).await.unwrap_err();

    assert_eq!(
        error.view_model.code,
        "APPLICATION_BACKUP_EXPORT_DURABILITY_UNCERTAIN"
    );
    assert!(!error.view_model.message.contains("原目标未被修改"));
    assert_ne!(&*file_system.target_bytes.lock(), b"old-target");
    assert_eq!(file_system.calls.lock().last(), Some(&"parent_sync"));
}

#[test]
fn concurrent_same_directory_exports_use_unique_temps_and_leave_no_residue() {
    let directory = tempfile::tempdir().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for index in 0..2 {
        let barrier = barrier.clone();
        let target = directory.path().join(format!("backup-{index}.zip"));
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            runtime.block_on(ApplicationBackupFileExporter::new(target, false).write(snapshot()))
        }));
    }
    barrier.wait();

    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    let mut names = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["backup-0.zip", "backup-1.zip"]);
}

#[test]
fn async_export_implementation_routes_blocking_zip_and_file_io_off_runtime() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application_backup_export.rs"),
    )
    .unwrap();

    assert!(source.contains("spawn_blocking"));
}

fn fail_if(actual: Option<FailureStage>, expected: FailureStage) -> io::Result<()> {
    if actual == Some(expected) {
        Err(io::Error::other(format!("injected {expected:?}")))
    } else {
        Ok(())
    }
}

fn snapshot() -> ApplicationBackupExportSnapshot {
    let document = parse_application_backup_document(
        &serde_json::to_vec(&json!({
            "format_version": 1,
            "application": {
                "selected_workspace_id": "00000000-0000-0000-0000-000000000001",
                "workspaces": [{
                    "id": "00000000-0000-0000-0000-000000000001",
                    "name": "backup",
                    "revision": 1,
                    "listeners": [],
                    "rules": [],
                    "protocol_rules": [],
                    "protocol_rule_created_order_high_water": 0,
                    "certificate_references": [],
                    "android_network_profiles": []
                }],
                "settings": {
                    "bind_address": "127.0.0.1",
                    "channels": [],
                    "connect_timeout_seconds": 10,
                    "write_timeout_seconds": 10,
                    "read_timeout_seconds": 10,
                    "rewrite_host": true,
                    "max_body_bytes": 1024,
                    "max_sessions": 10,
                    "max_memory_bytes": 4096,
                    "leaf_sans": [],
                    "external_package_service": {
                        "bind_address": "0.0.0.0",
                        "port": 8765,
                        "rpc_timeout_seconds": 5,
                        "max_in_flight": 256
                    }
                }
            },
            "protocol_packages": [{
                "package": { "id": "sample", "version": "1.0.0" },
                "enabled": true,
                "files": ["protocol-packages/sample/1.0.0/manifest.toml"]
            }],
            "portable_materials": []
        }))
        .unwrap(),
    )
    .unwrap();
    ApplicationBackupExportSnapshot {
        document,
        files: BTreeMap::from([(
            PortableArchivePath::new("protocol-packages/sample/1.0.0/manifest.toml").unwrap(),
            b"manifest".to_vec(),
        )]),
    }
}
