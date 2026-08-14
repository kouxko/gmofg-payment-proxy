use std::error::Error;

use crate::{PackageFilePath, ProtocolArchiveError, ProtocolArchiveErrorCode};

#[test]
fn every_archive_error_code_has_a_stable_wire_value() {
    let cases = [
        (ProtocolArchiveErrorCode::InvalidLimits, "INVALID_LIMITS"),
        (
            ProtocolArchiveErrorCode::ArchiveTooLarge,
            "ARCHIVE_TOO_LARGE",
        ),
        (ProtocolArchiveErrorCode::InvalidZip, "INVALID_ZIP"),
        (ProtocolArchiveErrorCode::EmptyArchive, "EMPTY_ARCHIVE"),
        (ProtocolArchiveErrorCode::TooManyEntries, "TOO_MANY_ENTRIES"),
        (ProtocolArchiveErrorCode::NonUtf8Path, "NON_UTF8_PATH"),
        (ProtocolArchiveErrorCode::InvalidPath, "INVALID_PATH"),
        (ProtocolArchiveErrorCode::PathTooDeep, "PATH_TOO_DEEP"),
        (ProtocolArchiveErrorCode::DuplicatePath, "DUPLICATE_PATH"),
        (ProtocolArchiveErrorCode::CaseConflict, "CASE_CONFLICT"),
        (
            ProtocolArchiveErrorCode::PathTypeConflict,
            "PATH_TYPE_CONFLICT",
        ),
        (
            ProtocolArchiveErrorCode::SymlinkForbidden,
            "SYMLINK_FORBIDDEN",
        ),
        (
            ProtocolArchiveErrorCode::UnsupportedEntryType,
            "UNSUPPORTED_ENTRY_TYPE",
        ),
        (ProtocolArchiveErrorCode::EncryptedEntry, "ENCRYPTED_ENTRY"),
        (
            ProtocolArchiveErrorCode::UnsupportedCompression,
            "UNSUPPORTED_COMPRESSION",
        ),
        (ProtocolArchiveErrorCode::FileTooLarge, "FILE_TOO_LARGE"),
        (ProtocolArchiveErrorCode::TotalTooLarge, "TOTAL_TOO_LARGE"),
        (
            ProtocolArchiveErrorCode::CompressionRatioExceeded,
            "COMPRESSION_RATIO_EXCEEDED",
        ),
        (
            ProtocolArchiveErrorCode::OverlappingEntries,
            "OVERLAPPING_ENTRIES",
        ),
        (
            ProtocolArchiveErrorCode::ManifestMissing,
            "MANIFEST_MISSING",
        ),
    ];
    for (code, wire) in cases {
        assert_eq!(code.as_str(), wire);
        assert_eq!(code.to_string(), wire);
        assert_eq!(serde_json::to_value(code).unwrap(), wire);
    }
}

#[test]
fn archive_errors_only_expose_safe_entry_context() {
    let archive = ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip);
    assert_eq!(archive.code(), ProtocolArchiveErrorCode::InvalidZip);
    assert_eq!(archive.entry_index(), None);
    assert_eq!(archive.path(), None);
    assert!(archive.source().is_none());

    let entry = ProtocolArchiveError::entry(ProtocolArchiveErrorCode::InvalidPath, 7);
    assert_eq!(entry.entry_index(), Some(7));
    assert_eq!(entry.path(), None);
    assert_eq!(
        serde_json::to_value(&entry).unwrap(),
        serde_json::json!({
            "code": "INVALID_PATH",
            "entry_index": 7,
            "path": null
        })
    );

    let path = PackageFilePath::new("scripts/main.rhai").unwrap();
    let safe =
        ProtocolArchiveError::safe_path(ProtocolArchiveErrorCode::FileTooLarge, 2, path.clone());
    assert_eq!(safe.path(), Some(&path));
    assert_eq!(safe.to_string(), "协议包 ZIP 校验失败（FILE_TOO_LARGE）");
}
