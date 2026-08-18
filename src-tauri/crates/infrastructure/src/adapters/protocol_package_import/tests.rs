use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Mutex,
};

use intercept_proxy_application::{
    AppError, AppResult, ProtocolPackageImportDispositionViewModel,
    ProtocolPackageImportOutcomeViewModel, ProtocolPackageImportPort,
};
use tempfile::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;
use crate::{SqliteStore, adapters::FileSelection};

const MANIFEST: &str = r#"
api = 1

[package]
id = "example-protocol"
name = "Example Protocol"
version = "1.0.0"

[document.upstream]
schema = "document.toml"
display = "display"

[document.downstream]
schema = "document.toml"
display = "display"

[hooks.upstream]
frame = "frame"
decode = "decode"
encode = "encode"

[hooks.downstream]
frame = "frame"
decode = "decode"
encode = "encode"
"#;

const SCHEMA: &str = r#"
id = "example-message"
version = 1
title = "Example Message"

[[fields]]
name = "trace_id"
label = "Trace ID"
type = "string"

[[fields]]
name = "amount"
label = "Amount"
type = "int"

[[fields]]
name = "approved"
label = "Approved"
type = "bool"
"#;

const SCRIPT: &str = r"
fn frame(reader, context) { framing::complete(1) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
";

const DISPLAY_SCRIPT: &str = r#"
fn display(document, context) { "<p>ok</p>" }
"#;

#[derive(Debug, Default)]
struct QueueDialog {
    paths: Mutex<Vec<AppResult<Option<PathBuf>>>>,
    purposes: Mutex<Vec<String>>,
}

impl QueueDialog {
    fn push(&self, path: AppResult<Option<PathBuf>>) {
        self.paths.lock().unwrap().push(path);
    }
}

impl NativeFileDialog for QueueDialog {
    fn choose_open_file(&self, purpose: &str) -> AppResult<Option<PathBuf>> {
        self.purposes.lock().unwrap().push(purpose.to_owned());
        self.paths.lock().unwrap().remove(0)
    }

    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        unreachable!("protocol package import never opens a save dialog")
    }
}

#[tokio::test]
async fn native_dialog_cancellation_and_permission_errors_never_change_the_registry() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    let dialog = Arc::new(QueueDialog::default());
    dialog.push(Ok(None));
    dialog.push(Err(AppError::new(
        "FILE_DIALOG_PERMISSION_DENIED",
        "denied",
    )));
    let importer = ProtocolPackageImportAdapter::new(repository.clone(), dialog.clone());

    assert_eq!(importer.prepare_zip().await.unwrap(), None);
    let error = importer.prepare_zip().await.unwrap_err();
    assert_eq!(error.view_model.code, "FILE_DIALOG_PERMISSION_DENIED");
    assert!(repository.list().unwrap().is_empty());
    assert_eq!(store.protocol_package_row_counts_for_test(), (0, 0));
    assert_eq!(
        dialog.purposes.lock().unwrap().as_slice(),
        ["protocol_package_zip", "protocol_package_zip"]
    );
}

#[tokio::test]
async fn invalid_zip_and_rhai_are_rejected_without_partial_state() {
    let fixture = TempDir::new().unwrap();
    let invalid_zip = fixture.path().join("invalid.zip");
    std::fs::write(&invalid_zip, b"not a zip").unwrap();
    let invalid_rhai = fixture.path().join("invalid-rhai.zip");
    std::fs::write(&invalid_rhai, package_zip("fn frame( {")).unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    let dialog = Arc::new(QueueDialog::default());
    dialog.push(Ok(Some(invalid_zip)));
    dialog.push(Ok(Some(invalid_rhai)));
    let importer = ProtocolPackageImportAdapter::new(repository.clone(), dialog);

    assert_eq!(
        importer.prepare_zip().await.unwrap_err().view_model.code,
        "INVALID_ZIP"
    );
    assert_eq!(
        importer.prepare_zip().await.unwrap_err().view_model.code,
        "SCRIPT_SYNTAX_INVALID"
    );
    assert!(repository.list().unwrap().is_empty());
    assert_eq!(store.protocol_package_row_counts_for_test(), (0, 0));
}

#[tokio::test]
async fn valid_zip_returns_safe_capabilities_schema_and_idempotent_outcome() {
    let fixture = TempDir::new().unwrap();
    let path = fixture.path().join("example.zip");
    std::fs::write(&path, package_zip(SCRIPT)).unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(store));
    let dialog = Arc::new(QueueDialog::default());
    dialog.push(Ok(Some(path.clone())));
    dialog.push(Ok(Some(path)));
    let importer = ProtocolPackageImportAdapter::new(repository, dialog);

    let first_preview = importer.prepare_zip().await.unwrap().unwrap();
    assert_eq!(
        first_preview.disposition,
        ProtocolPackageImportDispositionViewModel::New
    );
    assert!(
        importer.repository.list().unwrap().is_empty(),
        "prepare must not install before explicit confirmation"
    );
    let installed = importer
        .commit_zip(first_preview.token.unwrap())
        .await
        .unwrap();
    let second_preview = importer.prepare_zip().await.unwrap().unwrap();
    assert_eq!(
        second_preview.disposition,
        ProtocolPackageImportDispositionViewModel::Reusable
    );
    let reused = importer
        .commit_zip(second_preview.token.unwrap())
        .await
        .unwrap();
    assert_eq!(
        installed.outcome,
        ProtocolPackageImportOutcomeViewModel::Installed
    );
    assert_eq!(
        reused.outcome,
        ProtocolPackageImportOutcomeViewModel::Reused
    );
    assert_eq!(installed.version.package, reused.version.package);
    assert!(installed.capabilities.upstream.encode);
    assert!(installed.capabilities.downstream.encode);
    assert!(installed.capabilities.display);
    assert_eq!(
        installed
            .upstream_schema
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["trace_id", "amount", "approved"]
    );
}

#[tokio::test]
async fn commit_uses_frozen_validated_files_and_token_is_single_use() {
    let fixture = TempDir::new().unwrap();
    let path = fixture.path().join("replace-after-prepare.zip");
    std::fs::write(&path, package_zip(SCRIPT)).unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(store));
    let dialog = Arc::new(QueueDialog::default());
    dialog.push(Ok(Some(path.clone())));
    let importer = ProtocolPackageImportAdapter::new(repository.clone(), dialog);

    let preview = importer.prepare_zip().await.unwrap().unwrap();
    let token = preview.token.unwrap();
    // prepare 后即使磁盘原 ZIP 被替换，commit 也只能提交内存中被冻结且已验证的规范文件。
    std::fs::write(&path, b"not the validated archive").unwrap();
    let installed = importer.commit_zip(token).await.unwrap();
    assert_eq!(installed.version.package, preview.package);
    assert!(repository.summary(&preview.package).unwrap().is_some());

    let reused_token = importer.commit_zip(token).await.unwrap_err();
    assert_eq!(
        reused_token.view_model.code,
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
    );
    let forged = ProtocolPackageImportToken::from_uuid(Uuid::new_v4());
    assert_eq!(
        importer
            .commit_zip(forged)
            .await
            .unwrap_err()
            .view_model
            .code,
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
    );
}

#[tokio::test]
async fn conflict_preview_has_no_token_and_commit_rechecks_prepare_races() {
    let fixture = TempDir::new().unwrap();
    let original = fixture.path().join("original.zip");
    let conflicting = fixture.path().join("conflicting.zip");
    std::fs::write(&original, package_zip(SCRIPT)).unwrap();
    std::fs::write(
        &conflicting,
        package_zip(&format!("{SCRIPT}\n// immutable content changed")),
    )
    .unwrap();
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::new(SqliteStore::in_memory().unwrap()),
    ));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    let conflict_dialog = Arc::new(QueueDialog::default());
    conflict_dialog.push(Ok(Some(conflicting)));
    let conflict_importer =
        ProtocolPackageImportAdapter::new(Arc::clone(&repository), conflict_dialog);

    let conflict = conflict_importer.prepare_zip().await.unwrap().unwrap();
    assert_eq!(
        conflict.disposition,
        ProtocolPackageImportDispositionViewModel::IdentityConflict
    );
    assert_eq!(conflict.token, None);
    assert!(conflict_importer.pending.lock().entries.is_empty());

    repository.delete(&conflict.package).unwrap();
    let race_dialog = Arc::new(QueueDialog::default());
    race_dialog.push(Ok(Some(original)));
    let race_importer = ProtocolPackageImportAdapter::new(Arc::clone(&repository), race_dialog);
    let ready = race_importer.prepare_zip().await.unwrap().unwrap();
    let token = ready.token.unwrap();
    repository
        .install_zip(&package_zip(&format!("{SCRIPT}\n// racing writer")))
        .unwrap();
    assert_eq!(
        race_importer
            .commit_zip(token)
            .await
            .unwrap_err()
            .view_model
            .code,
        "PROTOCOL_PACKAGE_IDENTITY_CONFLICT"
    );
    assert_eq!(
        race_importer
            .commit_zip(token)
            .await
            .unwrap_err()
            .view_model
            .code,
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
    );
}

#[tokio::test]
async fn discard_immediately_recovers_capacity_and_permanently_invalidates_token() {
    let fixture = TempDir::new().unwrap();
    let path = fixture.path().join("example.zip");
    std::fs::write(&path, package_zip(SCRIPT)).unwrap();
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::new(SqliteStore::in_memory().unwrap()),
    ));
    let dialog = Arc::new(QueueDialog::default());
    for _ in 0..(MAX_PENDING_IMPORTS + 2) {
        dialog.push(Ok(Some(path.clone())));
    }
    let importer = ProtocolPackageImportAdapter::new(repository, dialog);
    let mut tokens = Vec::new();
    for _ in 0..MAX_PENDING_IMPORTS {
        tokens.push(
            importer
                .prepare_zip()
                .await
                .unwrap()
                .unwrap()
                .token
                .unwrap(),
        );
    }
    assert_eq!(
        importer.prepare_zip().await.unwrap_err().view_model.code,
        "PROTOCOL_PACKAGE_PENDING_LIMIT"
    );

    let discarded = tokens[0];
    importer.discard_zip(discarded).await.unwrap();
    assert_eq!(
        importer
            .commit_zip(discarded)
            .await
            .unwrap_err()
            .view_model
            .code,
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
    );
    assert_eq!(
        importer
            .discard_zip(discarded)
            .await
            .unwrap_err()
            .view_model
            .code,
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
    );
    assert!(importer.prepare_zip().await.unwrap().is_some());
}

#[tokio::test]
async fn compiler_diagnostic_exposes_only_safe_package_location() {
    let fixture = TempDir::new().unwrap();
    let path = fixture.path().join("secret-machine-path.zip");
    std::fs::write(
        &path,
        package_zip("fn frame(reader, context) {\n  let secret = ;\n}"),
    )
    .unwrap();
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::new(SqliteStore::in_memory().unwrap()),
    ));
    let dialog = Arc::new(QueueDialog::default());
    dialog.push(Ok(Some(path.clone())));
    let importer = ProtocolPackageImportAdapter::new(repository, dialog);

    let error = importer.prepare_zip().await.unwrap_err();
    let diagnostic = error.view_model.diagnostic.as_ref().unwrap();
    assert_eq!(diagnostic.file.as_deref(), Some("protocol.rhai"));
    assert_eq!(diagnostic.line, Some(2));
    assert!(diagnostic.column.is_some());
    let serialized = serde_json::to_string(&error.view_model).unwrap();
    assert!(!serialized.contains(path.to_string_lossy().as_ref()));
    assert!(!serialized.contains("let secret"));
}

#[tokio::test]
async fn declaration_and_entry_diagnostics_preserve_safe_field_and_identifier() {
    let fixture = TempDir::new().unwrap();
    let invalid_schema = fixture.path().join("invalid-schema.zip");
    let missing_entry = fixture.path().join("missing-entry.zip");
    std::fs::write(
        &invalid_schema,
        package_zip_parts(
            &SCHEMA.replacen("type = \"string\"", "type = \"decimal\"", 1),
            SCRIPT,
        ),
    )
    .unwrap();
    std::fs::write(
        &missing_entry,
        package_zip("fn decode(origin, context) { document::create() }"),
    )
    .unwrap();
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::new(SqliteStore::in_memory().unwrap()),
    ));
    let dialog = Arc::new(QueueDialog::default());
    dialog.push(Ok(Some(invalid_schema)));
    dialog.push(Ok(Some(missing_entry)));
    let importer = ProtocolPackageImportAdapter::new(repository, dialog);

    let schema = importer.prepare_zip().await.unwrap_err();
    let schema_diagnostic = schema.view_model.diagnostic.unwrap();
    assert_eq!(schema_diagnostic.file.as_deref(), Some("document.toml"));
    assert!(schema_diagnostic.field.as_deref().is_some_and(|field| {
        !field.is_empty()
            && field
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.$[]".contains(&byte))
    }));

    let entry = importer.prepare_zip().await.unwrap_err();
    let entry_diagnostic = entry.view_model.diagnostic.unwrap();
    assert_eq!(entry_diagnostic.file.as_deref(), Some("protocol.rhai"));
    assert_eq!(entry_diagnostic.entry.as_deref(), Some("frame"));
    assert_eq!(entry_diagnostic.line, None);
    assert_eq!(entry_diagnostic.column, None);
}

#[test]
fn pending_imports_expire_and_enforce_the_count_limit() {
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ));
    let now = Instant::now();
    let mut pending = PendingProtocolPackageImports::default();
    let first = pending
        .insert(repository.prepare_zip(&package_zip(SCRIPT)).unwrap(), now)
        .unwrap();
    assert_eq!(
        pending
            .take(first, now + PENDING_IMPORT_TTL)
            .unwrap_err()
            .view_model
            .code,
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
    );

    for _ in 0..MAX_PENDING_IMPORTS {
        pending
            .insert(repository.prepare_zip(&package_zip(SCRIPT)).unwrap(), now)
            .unwrap();
    }
    let error = pending
        .insert(repository.prepare_zip(&package_zip(SCRIPT)).unwrap(), now)
        .unwrap_err();
    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_PENDING_LIMIT");
}

fn package_zip(script: &str) -> Vec<u8> {
    package_zip_parts(SCHEMA, script)
}

fn package_zip_parts(schema: &str, script: &str) -> Vec<u8> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", MANIFEST.as_bytes()),
        ("document.toml", schema.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
        ("display.rhai", DISPLAY_SCRIPT.as_bytes()),
    ] {
        archive
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
