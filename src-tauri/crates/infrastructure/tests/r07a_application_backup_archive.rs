use std::io::{Cursor, Write};

use intercept_proxy_application::{PortableSettings, ProxyWorkspace, SettingsDraft};
use intercept_proxy_infrastructure::{
    ApplicationBackupArchive, ApplicationBackupArchiveErrorCode as Code,
    ApplicationBackupArchiveLimits,
};
use serde_json::json;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

#[derive(Clone)]
struct Entry {
    name: String,
    bytes: Vec<u8>,
    compression: CompressionMethod,
}

#[test]
fn deterministic_valid_fixture_reads_exact_references() {
    let zip = valid_zip();

    let first = ApplicationBackupArchive::read(&zip).expect("valid application backup");
    let second = ApplicationBackupArchive::read(&zip).expect("fixture is deterministic");

    assert_eq!(first, second);
    assert_eq!(first.files.len(), 3);
    assert_eq!(
        first
            .files
            .keys()
            .map(intercept_proxy_application::PortableArchivePath::as_str)
            .collect::<Vec<_>>(),
        [
            "portable-materials/server-identity.pem",
            "protocol-packages/sample/1.0.0/manifest.toml",
            "protocol-packages/sample/1.0.0/protocol.rhai",
        ]
    );
}

#[test]
fn archive_debug_redacts_configuration_payloads_and_passwords() {
    let workspace = ProxyWorkspace {
        name: "workspace-secret-marker".to_owned(),
        ..ProxyWorkspace::default()
    };
    let application = application_json_with(&workspace, Some("password-secret-marker"));
    let zip = build_zip(&[
        stored("application.json", application),
        stored(
            "protocol-packages/sample/1.0.0/manifest.toml",
            b"script-secret-marker",
        ),
        stored(
            "protocol-packages/sample/1.0.0/protocol.rhai",
            b"script-secret-marker",
        ),
        stored(
            "portable-materials/server-identity.pem",
            b"certificate-secret-marker",
        ),
    ]);
    let archive = ApplicationBackupArchive::read(&zip).expect("valid sensitive fixture");

    let debug = format!("{archive:?}");

    for secret in [
        "workspace-secret-marker",
        "script-secret-marker",
        "certificate-secret-marker",
        "password-secret-marker",
    ] {
        assert!(!debug.contains(secret), "Debug leaked {secret}");
    }
}

#[test]
fn non_zip_input_is_rejected() {
    assert_code(b"not a zip", Code::InvalidZip);
}

#[test]
fn empty_input_and_empty_zip_are_rejected() {
    assert_code(&[], Code::EmptyArchive);
    assert_code(&build_zip(&[]), Code::EmptyArchive);
}

#[test]
fn ordinary_protocol_package_zip_is_rejected() {
    let zip = build_zip(&[
        stored("manifest.toml", b"api = 1"),
        stored("protocol.rhai", b"fn frame() {}"),
    ]);

    assert_code(&zip, Code::UnknownTopLevel);
}

#[test]
fn archive_without_application_json_is_rejected() {
    let zip = build_zip(&[stored(
        "protocol-packages/sample/1.0.0/manifest.toml",
        b"api = 1",
    )]);

    assert_code(&zip, Code::ApplicationDocumentMissing);
}

#[test]
fn unknown_top_level_entry_is_rejected() {
    let mut entries = valid_entries();
    entries.push(stored("unexpected.txt", b"no"));

    assert_code(&build_zip(&entries), Code::UnknownTopLevel);
}

#[test]
fn traversal_and_absolute_entry_names_are_rejected_without_echoing_them() {
    for path in [
        "../application.json",
        "/application.json",
        "protocol-packages/sample/../manifest.toml",
        "C:/application.json",
        "protocol-packages\\sample\\manifest.toml",
    ] {
        let zip = build_zip(&[stored(path, b"secret-local-path")]);
        let error = ApplicationBackupArchive::read(&zip).expect_err("unsafe path rejected");
        assert_eq!(error.code(), Code::InvalidPath, "{path}");
        assert_eq!(error.path(), None);
        assert!(!error.to_string().contains(path));
        assert!(!error.to_string().contains("secret-local-path"));
    }
}

#[test]
fn symlink_entry_is_rejected() {
    let mut zip = valid_zip();
    patch_unix_mode(
        &mut zip,
        b"protocol-packages/sample/1.0.0/manifest.toml",
        0o120_777,
    );

    assert_code(&zip, Code::SymlinkForbidden);
}

#[test]
fn duplicate_entry_is_rejected() {
    let application = application_json();
    let mut zip = build_zip(&[
        stored("application.json", &application),
        stored("duplicatiox.json", &application),
    ]);
    patch_entry_name(&mut zip, b"duplicatiox.json", b"application.json");

    assert_code(&zip, Code::DuplicatePath);
}

#[test]
fn case_conflicting_entry_is_rejected() {
    let mut entries = valid_entries();
    entries.push(stored("APPLICATION.JSON", b"{}"));

    assert_code(&build_zip(&entries), Code::CaseConflict);
}

#[test]
fn file_parent_conflict_is_rejected_in_both_orders() {
    let application = application_json();
    for entries in [
        vec![
            stored("application.json", &application),
            stored("protocol-packages", b"file"),
            stored("protocol-packages/sample/1.0.0/manifest.toml", b"child"),
        ],
        vec![
            stored("application.json", &application),
            stored("protocol-packages/sample/1.0.0/manifest.toml", b"child"),
            stored("protocol-packages", b"file"),
        ],
    ] {
        assert_code(&build_zip(&entries), Code::PathTypeConflict);
    }
}

#[test]
fn exact_archive_limits_are_accepted() {
    let entries = valid_entries();
    let zip = build_zip(&entries);
    let file_bytes = entries.iter().map(|entry| entry.bytes.len()).max().unwrap() as u64;
    let total_bytes = entries.iter().map(|entry| entry.bytes.len() as u64).sum();
    let limits = ApplicationBackupArchiveLimits::new(
        zip.len() as u64,
        entries.len(),
        file_bytes,
        total_bytes,
        1,
        5,
    )
    .unwrap();

    ApplicationBackupArchive::read_with_limits(&zip, &limits).expect("exact limits accepted");
}

#[test]
fn zero_or_contradictory_limits_are_rejected() {
    for values in [
        (0, 1, 1, 1, 1, 1),
        (1, 0, 1, 1, 1, 1),
        (1, 1, 0, 1, 1, 1),
        (1, 1, 2, 1, 1, 1),
        (1, 1, 1, 1, 0, 1),
        (1, 1, 1, 1, 1, 0),
    ] {
        let error = ApplicationBackupArchiveLimits::new(
            values.0, values.1, values.2, values.3, values.4, values.5,
        )
        .expect_err("invalid limit rejected");
        assert_eq!(error.code(), Code::InvalidLimits);
    }
}

#[test]
fn archive_byte_limit_is_enforced() {
    let zip = valid_zip();
    let limits =
        ApplicationBackupArchiveLimits::new(zip.len() as u64 - 1, 10, 10_000, 40_000, 100, 10)
            .unwrap();

    assert_code_with_limits(&zip, &limits, Code::ArchiveTooLarge);
}

#[test]
fn entry_count_limit_is_enforced() {
    let zip = valid_zip();
    let limits = ApplicationBackupArchiveLimits::new(100_000, 3, 10_000, 40_000, 100, 10).unwrap();

    assert_code_with_limits(&zip, &limits, Code::TooManyEntries);
}

#[test]
fn per_file_limit_is_enforced() {
    let entries = valid_entries();
    let zip = build_zip(&entries);
    let largest = entries.iter().map(|entry| entry.bytes.len()).max().unwrap() as u64;
    let limits =
        ApplicationBackupArchiveLimits::new(100_000, 10, largest - 1, 100_000, 100, 10).unwrap();

    assert_code_with_limits(&zip, &limits, Code::FileTooLarge);
}

#[test]
fn total_uncompressed_limit_is_enforced() {
    let entries = valid_entries();
    let zip = build_zip(&entries);
    let largest = entries.iter().map(|entry| entry.bytes.len()).max().unwrap() as u64;
    let total = entries
        .iter()
        .map(|entry| entry.bytes.len() as u64)
        .sum::<u64>();
    let limits =
        ApplicationBackupArchiveLimits::new(100_000, 10, largest, total - 1, 100, 10).unwrap();

    assert_code_with_limits(&zip, &limits, Code::TotalTooLarge);
}

#[test]
fn path_depth_limit_is_enforced() {
    let zip = valid_zip();
    let limits = ApplicationBackupArchiveLimits::new(100_000, 10, 10_000, 40_000, 100, 3).unwrap();

    assert_code_with_limits(&zip, &limits, Code::PathTooDeep);
}

#[test]
fn compression_ratio_limit_is_enforced() {
    let application = application_json();
    let zip = build_zip(&[
        stored("application.json", &application),
        deflated(
            "protocol-packages/sample/1.0.0/manifest.toml",
            vec![0; 16 * 1024],
        ),
        stored(
            "protocol-packages/sample/1.0.0/protocol.rhai",
            b"fn frame() {}",
        ),
        stored("portable-materials/server-identity.pem", b"identity"),
    ]);
    let limits = ApplicationBackupArchiveLimits::new(100_000, 10, 20_000, 40_000, 2, 10).unwrap();

    assert_code_with_limits(&zip, &limits, Code::CompressionRatioExceeded);
}

#[test]
fn invalid_application_json_and_version_are_redacted_to_one_stable_code() {
    for application in [b"{".as_slice(), br#"{"format_version":2}"#.as_slice()] {
        let zip = build_zip(&[stored("application.json", application)]);
        let error = ApplicationBackupArchive::read(&zip).expect_err("invalid document rejected");
        assert_eq!(error.code(), Code::ApplicationDocumentInvalid);
        assert_eq!(error.path(), None);
        assert!(
            !error
                .to_string()
                .contains(std::str::from_utf8(application).unwrap())
        );
    }
}

#[test]
fn referenced_payload_must_be_present() {
    let entries = valid_entries()
        .into_iter()
        .filter(|entry| !entry.name.ends_with("protocol.rhai"))
        .collect::<Vec<_>>();

    assert_code(&build_zip(&entries), Code::ReferencedFileMissing);
}

#[test]
fn unreferenced_payload_is_rejected_as_orphan() {
    let mut entries = valid_entries();
    entries.push(stored(
        "protocol-packages/sample/1.0.0/orphan.txt",
        b"orphan",
    ));

    assert_code(&build_zip(&entries), Code::UnreferencedFile);
}

#[test]
fn error_codes_have_stable_wire_values() {
    let cases = [
        (Code::InvalidLimits, "INVALID_LIMITS"),
        (Code::ArchiveTooLarge, "ARCHIVE_TOO_LARGE"),
        (Code::InvalidZip, "INVALID_ZIP"),
        (Code::EmptyArchive, "EMPTY_ARCHIVE"),
        (Code::TooManyEntries, "TOO_MANY_ENTRIES"),
        (Code::NonUtf8Path, "NON_UTF8_PATH"),
        (Code::InvalidPath, "INVALID_PATH"),
        (Code::PathTooDeep, "PATH_TOO_DEEP"),
        (Code::DuplicatePath, "DUPLICATE_PATH"),
        (Code::CaseConflict, "CASE_CONFLICT"),
        (Code::PathTypeConflict, "PATH_TYPE_CONFLICT"),
        (Code::SymlinkForbidden, "SYMLINK_FORBIDDEN"),
        (Code::UnsupportedEntryType, "UNSUPPORTED_ENTRY_TYPE"),
        (Code::EncryptedEntry, "ENCRYPTED_ENTRY"),
        (Code::UnsupportedCompression, "UNSUPPORTED_COMPRESSION"),
        (Code::FileTooLarge, "FILE_TOO_LARGE"),
        (Code::TotalTooLarge, "TOTAL_TOO_LARGE"),
        (Code::CompressionRatioExceeded, "COMPRESSION_RATIO_EXCEEDED"),
        (Code::OverlappingEntries, "OVERLAPPING_ENTRIES"),
        (Code::UnknownTopLevel, "UNKNOWN_TOP_LEVEL"),
        (
            Code::ApplicationDocumentMissing,
            "APPLICATION_DOCUMENT_MISSING",
        ),
        (
            Code::ApplicationDocumentInvalid,
            "APPLICATION_DOCUMENT_INVALID",
        ),
        (Code::ReferencedFileMissing, "REFERENCED_FILE_MISSING"),
        (Code::UnreferencedFile, "UNREFERENCED_FILE"),
    ];
    for (code, wire) in cases {
        assert_eq!(code.as_str(), wire);
        assert_eq!(code.to_string(), wire);
        assert_eq!(serde_json::to_value(code).unwrap(), wire);
    }
}

fn valid_entries() -> Vec<Entry> {
    vec![
        stored("application.json", application_json()),
        stored("protocol-packages/sample/1.0.0/manifest.toml", b"api = 1"),
        stored(
            "protocol-packages/sample/1.0.0/protocol.rhai",
            b"fn frame() {}",
        ),
        stored("portable-materials/server-identity.pem", b"identity"),
    ]
}

fn valid_zip() -> Vec<u8> {
    build_zip(&valid_entries())
}

fn application_json() -> Vec<u8> {
    application_json_with(&ProxyWorkspace::default(), None)
}

fn application_json_with(workspace: &ProxyWorkspace, password: Option<&str>) -> Vec<u8> {
    let settings = PortableSettings::from(&SettingsDraft::default());
    serde_json::to_vec(&json!({
        "format_version": 1,
        "application": {
            "selected_workspace_id": workspace.id,
            "workspaces": [workspace],
            "settings": settings
        },
        "protocol_packages": [{
            "package": { "id": "sample", "version": "1.0.0" },
            "enabled": true,
            "files": [
                "protocol-packages/sample/1.0.0/manifest.toml",
                "protocol-packages/sample/1.0.0/protocol.rhai"
            ]
        }],
        "portable_materials": [{
            "reference_id": "00000000-0000-0000-0000-000000000002",
            "label": "server identity",
            "kind": "reverse_server_identity",
            "path": "portable-materials/server-identity.pem",
            "password": password
        }]
    }))
    .unwrap()
}

fn stored(name: &str, bytes: impl AsRef<[u8]>) -> Entry {
    entry(name, bytes, CompressionMethod::Stored)
}

fn deflated(name: &str, bytes: impl AsRef<[u8]>) -> Entry {
    entry(name, bytes, CompressionMethod::Deflated)
}

fn entry(name: &str, bytes: impl AsRef<[u8]>, compression: CompressionMethod) -> Entry {
    Entry {
        name: name.to_owned(),
        bytes: bytes.as_ref().to_vec(),
        compression,
    }
}

fn build_zip(entries: &[Entry]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for entry in entries {
        writer
            .start_file(
                &entry.name,
                SimpleFileOptions::default().compression_method(entry.compression),
            )
            .unwrap();
        writer.write_all(&entry.bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn assert_code(bytes: &[u8], expected: Code) {
    let error = ApplicationBackupArchive::read(bytes).expect_err("archive rejected");
    assert_eq!(error.code(), expected);
}

fn assert_code_with_limits(bytes: &[u8], limits: &ApplicationBackupArchiveLimits, expected: Code) {
    let error =
        ApplicationBackupArchive::read_with_limits(bytes, limits).expect_err("archive rejected");
    assert_eq!(error.code(), expected);
}

fn patch_unix_mode(bytes: &mut [u8], name: &[u8], mode: u32) {
    let central = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == [0x50, 0x4b, 0x01, 0x02])
        .map(|(offset, _)| offset)
        .find(|offset| {
            let name_len = u16::from_le_bytes([bytes[*offset + 28], bytes[*offset + 29]]) as usize;
            name_len == name.len() && &bytes[*offset + 46..*offset + 46 + name_len] == name
        })
        .expect("central entry");
    bytes[central + 4..central + 6].copy_from_slice(&[20, 3]);
    bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
}

fn patch_entry_name(bytes: &mut [u8], old: &[u8], new: &[u8]) {
    assert_eq!(old.len(), new.len());
    let mut replacements = 0;
    for offset in 0..=bytes.len() - old.len() {
        if &bytes[offset..offset + old.len()] == old {
            bytes[offset..offset + new.len()].copy_from_slice(new);
            replacements += 1;
        }
    }
    assert_eq!(replacements, 2, "local and central names patched");
}
