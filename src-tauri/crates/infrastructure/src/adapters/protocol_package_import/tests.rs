use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Mutex,
};

use intercept_proxy_application::{AppResult, ProtocolPackageImportPort};
use tempfile::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;
use crate::{SqliteStore, adapters::FileSelection};

const MANIFEST: &str = include_str!(
    "../../../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);

#[derive(Debug, Default)]
struct QueueDialog(Mutex<Vec<PathBuf>>);
impl NativeFileDialog for QueueDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(Some(self.0.lock().unwrap().remove(0)))
    }
    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        unreachable!()
    }
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut output);
    for (path, bytes) in entries {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
    output.into_inner()
}

fn importer(
    bytes: &[u8],
) -> (
    TempDir,
    ProtocolPackageImportAdapter,
    Arc<ExternalPackageRegistryAdapter>,
) {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("package.zip");
    std::fs::write(&path, bytes).unwrap();
    let dialog = Arc::new(QueueDialog(Mutex::new(vec![path])));
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::new(SqliteStore::in_memory().unwrap()),
    ));
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::new(
        SqliteStore::in_memory().unwrap(),
    )));
    (
        temp,
        ProtocolPackageImportAdapter::new(repository, Arc::clone(&registry), dialog),
        registry,
    )
}

#[tokio::test]
async fn strict_javascript_resources_preview_without_evaluating_top_level_code() {
    let bytes = zip(&[
        ("manifest.json", MANIFEST.as_bytes()),
        (
            "protocol.js",
            b"throw new Error('preview must not evaluate');",
        ),
        (
            "display.js",
            b"throw new Error('preview must not evaluate');",
        ),
    ]);
    let (_temp, importer, _registry) = importer(&bytes);
    let preview = importer.prepare_zip().await.unwrap().unwrap();
    assert_eq!(
        preview.disposition,
        ProtocolPackageImportDispositionViewModel::New
    );
    assert!(preview.token.is_some());
    assert_eq!(preview.host_api, 1);
}

#[tokio::test]
async fn commit_persists_enabled_archive_without_evaluating_top_level_code_in_importer() {
    let bytes = zip(&[
        ("manifest.json", MANIFEST.as_bytes()),
        (
            "protocol.js",
            b"throw new Error('commit importer must not evaluate');",
        ),
        (
            "display.js",
            b"throw new Error('commit importer must not evaluate');",
        ),
    ]);
    let (_temp, importer, registry) = importer(&bytes);
    let preview = importer.prepare_zip().await.unwrap().unwrap();
    let package = preview.package.clone();
    let error = importer
        .commit_zip(preview.token.unwrap())
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "EXTERNAL_PACKAGE_PROCESS_FAILED");
    let stored = registry
        .get(&package)
        .await
        .unwrap()
        .expect("commit persisted exact local package before process launch");
    assert!(stored.enabled);
}

#[tokio::test]
async fn legacy_toml_rhai_and_wrapper_archives_never_reach_legacy_prepare() {
    for entries in [
        vec![
            ("manifest.toml", b"api=1".as_slice()),
            ("protocol.rhai", b"fn x(){}".as_slice()),
        ],
        vec![
            ("package/manifest.json", MANIFEST.as_bytes()),
            ("package/protocol.js", b"export {}"),
            ("package/display.js", b"export {}"),
        ],
    ] {
        let bytes = zip(&entries);
        let (_temp, importer, _registry) = importer(&bytes);
        let error = importer.prepare_zip().await.unwrap_err();
        assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_INVALID");
        assert!(!error.view_model.field_errors.contains_key("runtime"));
    }
}
