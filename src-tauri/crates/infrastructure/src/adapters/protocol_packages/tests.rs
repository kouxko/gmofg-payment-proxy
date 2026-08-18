use std::{
    io::{Cursor, Write},
    sync::{Arc, Barrier},
};

use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use intercept_proxy_protocol_scripting::ProtocolPackageKind;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

#[path = "tests/concurrent_imports.rs"]
mod concurrent_imports;

mod application_ports;
mod cache;

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
name = "amount"
label = "Amount"
type = "int"
"#;

const SCRIPT: &str = r"
fn frame(reader, context) { framing::complete(1) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
";

const DISPLAY: &str = r#"fn display(document, context) { "<p>ok</p>" }"#;

#[test]
fn install_list_enable_compile_and_delete_use_no_source_summary() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    let zip = package_zip(MANIFEST, SCRIPT);

    let ProtocolPackageInstallOutcome::Installed(installed) = repository.install_zip(&zip).unwrap()
    else {
        panic!("first import must install");
    };
    assert_eq!(installed.package, package("1.0.0"));
    assert_eq!(installed.name, "Example Protocol");
    assert_eq!(installed.kind, ProtocolPackageKind::Socket);
    assert!(!installed.enabled);
    assert_eq!(installed.validation, ProtocolPackageValidationStatus::Valid);
    assert_eq!(repository.list().unwrap(), vec![installed.clone()]);
    assert_eq!(
        repository.compiled(&installed.package).unwrap().package(),
        &installed.package
    );

    repository.set_enabled(&installed.package, true).unwrap();
    assert!(
        repository
            .summary(&installed.package)
            .unwrap()
            .unwrap()
            .enabled
    );
    repository.delete(&installed.package).unwrap();
    assert!(repository.list().unwrap().is_empty());
    assert_eq!(
        repository.compiled(&installed.package).unwrap_err().code(),
        ProtocolPackageStorageErrorCode::NotFound
    );
}

#[test]
fn identical_reimport_reuses_record_while_versions_coexist_and_overwrite_is_rejected() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    let zip = package_zip(MANIFEST, SCRIPT);
    let ProtocolPackageInstallOutcome::Installed(first) = repository.install_zip(&zip).unwrap()
    else {
        panic!("first import must install");
    };
    repository.set_enabled(&first.package, true).unwrap();

    let ProtocolPackageInstallOutcome::Reused(reused) = repository.install_zip(&zip).unwrap()
    else {
        panic!("same bytes must reuse");
    };
    assert_eq!(reused.installed_at, first.installed_at);
    assert!(
        reused.enabled,
        "idempotent import must preserve enabled state"
    );

    let changed = package_zip(MANIFEST, &SCRIPT.replace("origin }", "blob() }"));
    let error = repository.install_zip(&changed).unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageStorageErrorCode::IdentityConflict
    );
    assert_eq!(
        error.detail_code(),
        Some("PROTOCOL_PACKAGE_IDENTITY_CONFLICT")
    );
    assert_eq!(
        repository.compiled(&first.package).unwrap().package(),
        &first.package
    );

    let manifest_v2 = MANIFEST.replace("version = \"1.0.0\"", "version = \"2.0.0\"");
    assert!(matches!(
        repository
            .install_zip(&package_zip(&manifest_v2, SCRIPT))
            .unwrap(),
        ProtocolPackageInstallOutcome::Installed(_)
    ));
    assert_eq!(repository.list().unwrap().len(), 2);
}

#[test]
fn invalid_zip_and_invalid_script_never_create_partial_rows() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    let archive_error = repository.install_zip(b"not a zip").unwrap_err();
    assert_eq!(
        archive_error.code(),
        ProtocolPackageStorageErrorCode::ArchiveInvalid
    );
    assert_eq!(archive_error.detail_code(), Some("INVALID_ZIP"));
    let invalid = package_zip(MANIFEST, "fn frame( {");
    let error = repository.install_zip(&invalid).unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageStorageErrorCode::CompilationFailed
    );
    assert_eq!(error.detail_code(), Some("SCRIPT_SYNTAX_INVALID"));
    assert!(repository.list().unwrap().is_empty());
    assert_eq!(store.protocol_package_row_counts_for_test(), (0, 0));
}

#[test]
fn cache_recovery_marks_missing_and_uncompilable_files_invalid_without_blocking_good_version() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let installer = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    installer
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let manifest_v2 = MANIFEST.replace("version = \"1.0.0\"", "version = \"2.0.0\"");
    installer
        .install_zip(&package_zip(&manifest_v2, SCRIPT))
        .unwrap();
    store.delete_protocol_package_file_for_test(&package("1.0.0"), "manifest.toml");
    store.replace_protocol_package_file_for_test(
        &package("2.0.0"),
        "protocol.rhai",
        b"fn frame( {",
    );

    let restarted = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    let report = restarted.recover_cache().unwrap();
    assert!(report.loaded.is_empty());
    assert_eq!(report.failed.len(), 2);
    assert!(report.failed.iter().any(|failure| {
        failure.package == package("1.0.0") && failure.code == "MANIFEST_MISSING"
    }));
    assert!(report.failed.iter().any(|failure| {
        failure.package == package("2.0.0") && failure.code == "SCRIPT_SYNTAX_INVALID"
    }));
    assert!(restarted.list().unwrap().iter().all(|summary| matches!(
        summary.validation,
        ProtocolPackageValidationStatus::Invalid { .. }
    )));
}

#[test]
fn corrupt_header_is_quarantined_without_blocking_healthy_version_recovery() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let installer = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    installer
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let manifest_v2 = MANIFEST.replace("version = \"1.0.0\"", "version = \"2.0.0\"");
    installer
        .install_zip(&package_zip(&manifest_v2, SCRIPT))
        .unwrap();
    store.corrupt_protocol_package_host_api_for_test(&package("1.0.0"), -1);

    let restarted = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    let report = restarted.recover_cache().unwrap();
    assert_eq!(report.loaded, vec![package("2.0.0")]);
    assert_eq!(
        report.failed,
        vec![ProtocolPackageRecoveryFailure {
            package: package("1.0.0"),
            code: "PERSISTENCE_CORRUPT".to_owned(),
        }]
    );
    let corrupt = restarted.summary(&package("1.0.0")).unwrap().unwrap();
    assert_eq!(corrupt.host_api, 0);
    assert!(!corrupt.enabled);
    assert_eq!(
        corrupt.validation,
        ProtocolPackageValidationStatus::Invalid {
            code: "PERSISTENCE_CORRUPT".to_owned(),
        }
    );
}

#[test]
fn oversized_persisted_blob_is_rejected_by_preflight_while_summary_stays_header_only() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let installer = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    installer
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let package = package("1.0.0");
    let oversized =
        i64::try_from(intercept_proxy_protocol_scripting::MAX_FILE_BYTES_LIMIT).unwrap() + 1;
    store.replace_protocol_package_file_with_zeroblob_for_test(
        &package,
        "protocol.rhai",
        oversized,
    );

    let restarted = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    assert_eq!(
        restarted.summary(&package).unwrap().unwrap().name,
        "Example Protocol"
    );
    let error = restarted.compiled(&package).unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageStorageErrorCode::StoredPackageInvalid
    );
    assert_eq!(error.detail_code(), Some("FILE_TOO_LARGE"));
}

#[test]
fn persisted_path_is_revalidated_before_compilation_and_enabled_state_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let package = package("1.0.0");
    {
        let store = Arc::new(SqliteStore::open(&path).unwrap());
        let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
        repository
            .install_zip(&package_zip(MANIFEST, SCRIPT))
            .unwrap();
        repository.set_enabled(&package, true).unwrap();
        store.rename_protocol_package_file_for_test(&package, "protocol.rhai", "../protocol.rhai");
    }

    let reopened = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::open(&path).unwrap(),
    ));
    assert!(reopened.summary(&package).unwrap().unwrap().enabled);
    let error = reopened.compiled(&package).unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageStorageErrorCode::StoredPackageInvalid
    );
    assert_eq!(error.detail_code(), Some("INVALID_PATH"));
    assert!(matches!(
        reopened.summary(&package).unwrap().unwrap().validation,
        ProtocolPackageValidationStatus::Invalid { ref code } if code == "INVALID_PATH"
    ));
}

#[test]
fn stored_manifest_identity_mismatch_is_invalidated_before_cache_insert() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let installer = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    installer
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let package = package("1.0.0");
    store.rename_protocol_package_for_test(&package, "Corrupted Display Name");

    let restarted = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    let error = restarted.compiled(&package).unwrap_err();
    assert_eq!(error.detail_code(), Some("STORED_IDENTITY_MISMATCH"));
    assert!(matches!(
        restarted.summary(&package).unwrap().unwrap().validation,
        ProtocolPackageValidationStatus::Invalid { ref code }
            if code == "STORED_IDENTITY_MISMATCH"
    ));
}

#[test]
fn stored_manifest_kind_mismatch_is_invalidated_before_cache_insert() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let installer = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    installer
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let package = package("1.0.0");
    store.corrupt_protocol_package_kind_for_test(&package, "http");

    let restarted = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    let error = restarted.compiled(&package).unwrap_err();
    assert_eq!(error.detail_code(), Some("STORED_IDENTITY_MISMATCH"));
    assert!(matches!(
        restarted.summary(&package).unwrap().unwrap().validation,
        ProtocolPackageValidationStatus::Invalid { ref code }
            if code == "STORED_IDENTITY_MISMATCH"
    ));
}

#[test]
fn runtime_snapshot_rejects_a_tampered_persisted_kind() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let installer = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    installer
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let package = package("1.0.0");
    installer.set_enabled(&package, true).unwrap();
    store.corrupt_protocol_package_kind_for_test(&package, "http");

    let restarted = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    let error = restarted.freeze_for_listener_start(&package).unwrap_err();
    assert_eq!(error.view_model.code, "STORED_IDENTITY_MISMATCH");
}

#[test]
fn one_package_id_cannot_mix_http_and_socket_versions() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let http_v2 = MANIFEST
        .replace("version = \"1.0.0\"", "version = \"2.0.0\"")
        .replace("frame = \"frame\"\n", "");

    let error = repository
        .install_zip(&package_zip(&http_v2, SCRIPT))
        .unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageStorageErrorCode::IdentityConflict
    );
    assert_eq!(store.protocol_package_row_counts_for_test(), (1, 4));
    assert_eq!(
        repository.list().unwrap()[0].kind,
        ProtocolPackageKind::Socket
    );
}

#[test]
fn missing_and_persistence_errors_keep_stable_coarse_and_detail_codes() {
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ));
    let missing = package("9.9.9");
    let compiled = repository.compiled(&missing).unwrap_err();
    assert_eq!(compiled.code(), ProtocolPackageStorageErrorCode::NotFound);
    assert_eq!(compiled.detail_code(), Some("PROTOCOL_PACKAGE_NOT_FOUND"));
    let enabled = repository.set_enabled(&missing, true).unwrap_err();
    assert_eq!(enabled.detail_code(), Some("PROTOCOL_PACKAGE_NOT_FOUND"));
    let deleted = repository.delete(&missing).unwrap_err();
    assert_eq!(deleted.detail_code(), Some("PROTOCOL_PACKAGE_NOT_FOUND"));
    let validation = repository
        .require_validation_update(&missing, None)
        .unwrap_err();
    assert_eq!(validation.detail_code(), Some("PROTOCOL_PACKAGE_NOT_FOUND"));

    let persistence =
        ProtocolPackageStorageError::Infrastructure(InfrastructureError::RevisionConflict);
    assert_eq!(
        persistence.code(),
        ProtocolPackageStorageErrorCode::PersistenceFailed
    );
    assert_eq!(persistence.detail_code(), None);
}

#[test]
fn concurrent_identical_import_has_one_install_one_reuse_and_no_half_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    SqliteStore::open(&path).unwrap();
    let zip = Arc::new(package_zip(MANIFEST, SCRIPT));
    let barrier = Arc::new(Barrier::new(2));
    // 两个连接先顺序完成幂等迁移，再并发进入安装事务；本测试只验证安装竞争，不把启动迁移锁竞争混入结果。
    let stores = [
        Arc::new(SqliteStore::open(&path).unwrap()),
        Arc::new(SqliteStore::open(&path).unwrap()),
    ];
    let threads = stores
        .into_iter()
        .map(|store| {
            let zip = Arc::clone(&zip);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let repository = ProtocolPackageRepositoryAdapter::with_default_limits(store);
                barrier.wait();
                repository.install_zip(&zip)
            })
        })
        .collect::<Vec<_>>();
    let outcomes = threads
        .into_iter()
        .map(|thread| thread.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ProtocolPackageInstallOutcome::Installed(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ProtocolPackageInstallOutcome::Reused(_)))
            .count(),
        1
    );
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.protocol_package_row_counts_for_test(), (1, 4));
}

#[test]
fn database_failure_rolls_back_header_and_every_file() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.reject_protocol_package_file_for_test("protocol.rhai");
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    let error = repository
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageStorageErrorCode::PersistenceFailed
    );
    assert_eq!(store.protocol_package_row_counts_for_test(), (0, 0));
}

fn package(version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("example-protocol").unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

fn package_zip(manifest: &str, script: &str) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    for (path, contents) in [
        ("manifest.toml", manifest.as_bytes()),
        ("document.toml", SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
        ("display.rhai", DISPLAY.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
