use std::error::Error;

use crate::{ProtocolPackageFile, ProtocolPackageParseError, ProtocolPackageParseErrorCode};

#[test]
fn parse_error_codes_and_files_have_stable_wire_values() {
    let codes = [
        (ProtocolPackageParseErrorCode::TomlInvalid, "TOML_INVALID"),
        (
            ProtocolPackageParseErrorCode::InputTooLarge,
            "INPUT_TOO_LARGE",
        ),
        (
            ProtocolPackageParseErrorCode::UnsupportedHostApi,
            "UNSUPPORTED_HOST_API",
        ),
        (
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "MANIFEST_INVALID",
        ),
        (
            ProtocolPackageParseErrorCode::DocumentSchemaInvalid,
            "DOCUMENT_SCHEMA_INVALID",
        ),
        (
            ProtocolPackageParseErrorCode::ReferencedFileMissing,
            "REFERENCED_FILE_MISSING",
        ),
        (
            ProtocolPackageParseErrorCode::RequiredFileMissing,
            "REQUIRED_FILE_MISSING",
        ),
    ];
    for (code, wire) in codes {
        assert_eq!(code.as_str(), wire);
        assert_eq!(code.to_string(), wire);
        assert_eq!(serde_json::to_value(code).unwrap(), wire);
    }

    for (file, name, wire) in [
        (ProtocolPackageFile::Manifest, "manifest.toml", "manifest"),
        (
            ProtocolPackageFile::DocumentSchema,
            "document.toml",
            "document_schema",
        ),
    ] {
        assert_eq!(file.file_name(), name);
        assert_eq!(file.to_string(), name);
        assert_eq!(serde_json::to_value(file).unwrap(), wire);
    }
}

#[test]
fn parse_errors_serialize_only_controlled_diagnostics() {
    let error = ProtocolPackageParseError::new(
        ProtocolPackageParseErrorCode::ManifestInvalid,
        ProtocolPackageFile::Manifest,
        "hooks.upstream.receive.frame",
    );
    assert_eq!(error.code(), ProtocolPackageParseErrorCode::ManifestInvalid);
    assert_eq!(error.file(), ProtocolPackageFile::Manifest);
    assert_eq!(error.field(), "hooks.upstream.receive.frame");
    assert_eq!(
        error.to_string(),
        "manifest.toml 的 hooks.upstream.receive.frame 无效（MANIFEST_INVALID）"
    );
    assert!(error.source().is_none());
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({
            "code": "MANIFEST_INVALID",
            "file": "manifest",
            "field": "hooks.upstream.receive.frame"
        })
    );
}

#[test]
fn untrusted_or_long_field_paths_are_collapsed_to_root() {
    for field in [
        "/Users/alice/private/protocol.rhai",
        "字段",
        "line\nsecret = 1234",
    ] {
        let error = ProtocolPackageParseError::new(
            ProtocolPackageParseErrorCode::TomlInvalid,
            ProtocolPackageFile::Manifest,
            field,
        );
        assert_eq!(error.field(), "$");
        assert!(!error.to_string().contains(field));
    }
    let error = ProtocolPackageParseError::new(
        ProtocolPackageParseErrorCode::TomlInvalid,
        ProtocolPackageFile::Manifest,
        &"x".repeat(161),
    );
    assert_eq!(error.field(), "$");
}

#[test]
fn fixed_missing_file_errors_are_safe_for_zip_importer_reuse() {
    for file in [
        ProtocolPackageFile::Manifest,
        ProtocolPackageFile::DocumentSchema,
    ] {
        let error = ProtocolPackageParseError::required_file_missing(file);
        assert_eq!(
            error.code(),
            ProtocolPackageParseErrorCode::RequiredFileMissing
        );
        assert_eq!(error.file(), file);
        assert_eq!(error.field(), "$");
    }
}
