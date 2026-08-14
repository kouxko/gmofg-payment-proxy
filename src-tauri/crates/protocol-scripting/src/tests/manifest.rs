use std::collections::BTreeSet;

use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};

use super::fixtures::{TEMPLATE_MANIFEST, minimal_manifest};
use crate::{
    MAX_MANIFEST_TOML_BYTES, PackageFilePath, ProtocolPackageFile, ProtocolPackageParseErrorCode,
    SUPPORTED_PROTOCOL_HOST_API, parse_protocol_manifest,
};

#[test]
fn official_template_exposes_complete_v1_capabilities() {
    let manifest = parse_protocol_manifest(TEMPLATE_MANIFEST).unwrap();
    assert_eq!(manifest.api(), SUPPORTED_PROTOCOL_HOST_API);
    assert_eq!(
        manifest.package().package().id,
        ProtocolPackageId::new("iso8583-ascii-standard").unwrap()
    );
    assert_eq!(
        manifest.package().package().version,
        ProtocolPackageVersion::new("1.0.0").unwrap()
    );
    assert_eq!(manifest.package().name(), "ISO 8583 ASCII Standard");
    assert_eq!(manifest.document().schema().as_str(), "document.toml");

    let display = manifest.document().display().unwrap();
    assert_eq!(display.script().as_str(), "display.rhai");
    assert_eq!(display.function().as_str(), "display");

    let upstream = manifest.hooks().upstream();
    assert_eq!(upstream.receive().script().as_str(), "protocol.rhai");
    assert_eq!(upstream.receive().frame().as_str(), "frame");
    assert_eq!(upstream.receive().decode().as_str(), "decode");
    assert_eq!(upstream.send().unwrap().encode().as_str(), "encode");
    let downstream = manifest.hooks().downstream();
    assert_eq!(downstream.receive().script().as_str(), "protocol.rhai");
    assert_eq!(downstream.receive().frame().as_str(), "frame");
    assert_eq!(downstream.receive().decode().as_str(), "decode");
    assert_eq!(
        downstream.send().unwrap().script().as_str(),
        "protocol.rhai"
    );
}

#[test]
fn directions_can_use_distinct_scripts_and_functions_while_send_is_optional() {
    let manifest = parse_protocol_manifest(&minimal_manifest()).unwrap();
    assert!(manifest.document().display().is_none());
    assert!(manifest.hooks().upstream().send().is_none());
    assert!(manifest.hooks().downstream().send().is_none());
    assert_eq!(
        manifest.hooks().upstream().receive().script().as_str(),
        "scripts/upstream.rhai"
    );
    assert_eq!(
        manifest.hooks().upstream().receive().frame().as_str(),
        "upstream_frame"
    );
    assert_eq!(
        manifest.hooks().downstream().receive().script().as_str(),
        "scripts/downstream.rhai"
    );
    assert_eq!(
        manifest.hooks().downstream().receive().decode().as_str(),
        "downstream_decode"
    );
    assert_eq!(manifest.referenced_files().len(), 3);
}

#[test]
fn optional_send_and_display_tables_are_parsed_independently() {
    let input = manifest_with_optionals();
    let manifest = parse_protocol_manifest(&input).unwrap();
    assert_eq!(
        manifest.document().display().unwrap().function().as_str(),
        "render_document"
    );
    assert_eq!(
        manifest
            .hooks()
            .upstream()
            .send()
            .unwrap()
            .encode()
            .as_str(),
        "encode_upstream"
    );
    assert!(manifest.hooks().downstream().send().is_none());
}

fn manifest_with_optionals() -> String {
    format!(
        r#"{}
[document.display]
script = "scripts/display.rhai"
function = "render_document"

[hooks.upstream.send]
script = "scripts/upstream_encode.rhai"
encode = "encode_upstream"
"#,
        minimal_manifest()
    )
}

#[test]
fn optional_entry_paths_and_functions_are_validated_at_their_own_fields() {
    let base = manifest_with_optionals();
    let cases = [
        (
            base.replace("scripts/display.rhai", "../display.rhai"),
            "document.display.script",
        ),
        (
            base.replace("function = \"render_document\"", "function = \"while\""),
            "document.display.function",
        ),
        (
            base.replace("scripts/upstream_encode.rhai", "../encode.rhai"),
            "hooks.upstream.send.script",
        ),
        (
            base.replace("encode = \"encode_upstream\"", "encode = \"1encode\""),
            "hooks.upstream.send.encode",
        ),
    ];
    for (input, field) in cases {
        let error = parse_protocol_manifest(&input).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::ManifestInvalid);
        assert_eq!(error.field(), field);
    }
}

#[test]
fn strict_manifest_toml_rejects_missing_unknown_duplicate_and_wrong_shapes() {
    let base = minimal_manifest();
    let cases = [
        String::new(),
        base.replace("api = 1\n", ""),
        base.replace("[hooks.downstream.receive]", "[hooks.sideways.receive]"),
        base.replace("frame = \"upstream_frame\"\n", ""),
        base.replace("decode = \"downstream_decode\"\n", ""),
        base.replace("api = 1", "api = \"one\""),
        base.replace(
            "id = \"example-protocol\"",
            "id = \"example-protocol\"\nid = \"duplicate\"",
        ),
        format!("{base}\nunknown = true\n"),
        format!("{base}\n[hooks.upstream.receive.extra]\nvalue = true\n"),
        base.replace(
            "[hooks.upstream.receive]",
            "[hooks.upstream]\nreceive = []\n\n[hooks.upstream.receive]",
        ),
        format!("{base}\n[hooks.upstream.send]\nscript = \"encode.rhai\"\n"),
        format!("{base}\n[document.display]\nscript = \"display.rhai\"\n"),
    ];
    for input in cases {
        let error = parse_protocol_manifest(&input).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
        assert_eq!(error.file(), ProtocolPackageFile::Manifest);
    }
}

#[test]
fn strict_manifest_type_errors_keep_the_serde_field_path() {
    let error = parse_protocol_manifest(&minimal_manifest().replace("api = 1", "api = \"one\""))
        .unwrap_err();
    assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
    assert_eq!(error.field(), "api");
}

#[test]
fn manifest_semantic_matrix_reports_stable_fields() {
    let base = minimal_manifest();
    let long_name = "n".repeat(129);
    let long_function = format!("f{}", "x".repeat(64));
    let cases = [
        (
            base.replace("api = 1", "api = 2"),
            ProtocolPackageParseErrorCode::UnsupportedHostApi,
            "api",
        ),
        (
            base.replace("id = \"example-protocol\"", "id = \"Invalid_ID\""),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "package.id",
        ),
        (
            base.replace("version = \"1.2.3-beta.1+build.7\"", "version = \"latest\""),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "package.version",
        ),
        (
            base.replace("name = \"Example Protocol\"", "name = \"   \""),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "package.name",
        ),
        (
            base.replace(
                "name = \"Example Protocol\"",
                "name = \"Example\\u0007Protocol\"",
            ),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "package.name",
        ),
        (
            base.replace(
                "name = \"Example Protocol\"",
                &format!("name = \"{long_name}\""),
            ),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "package.name",
        ),
        (
            base.replace(
                "schema = \"schemas/document.toml\"",
                "schema = \"../document.toml\"",
            ),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "document.schema",
        ),
        (
            base.replace(
                "script = \"scripts/upstream.rhai\"",
                "script = \"C:\\\\private\\\\upstream.rhai\"",
            ),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "hooks.upstream.receive.script",
        ),
        (
            base.replace("frame = \"upstream_frame\"", "frame = \"while\""),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "hooks.upstream.receive.frame",
        ),
        (
            base.replace(
                "decode = \"downstream_decode\"",
                &format!("decode = \"{long_function}\""),
            ),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "hooks.downstream.receive.decode",
        ),
    ];
    for (input, code, field) in cases {
        let error = parse_protocol_manifest(&input).unwrap_err();
        assert_eq!(error.code(), code);
        assert_eq!(error.field(), field);
    }
}

#[test]
fn manifest_display_name_accepts_exact_unicode_character_limit() {
    let name = "协".repeat(128);
    let input =
        minimal_manifest().replace("name = \"Example Protocol\"", &format!("name = \"{name}\""));
    assert_eq!(
        parse_protocol_manifest(&input).unwrap().package().name(),
        name
    );
}

#[test]
fn manifest_referenced_file_set_is_deduplicated_and_validated() {
    let manifest = parse_protocol_manifest(TEMPLATE_MANIFEST).unwrap();
    let referenced = manifest.referenced_files();
    assert_eq!(referenced.len(), 3);
    assert!(
        referenced
            .iter()
            .any(|path| path.as_str() == "document.toml")
    );
    assert!(
        referenced
            .iter()
            .any(|path| path.as_str() == "display.rhai")
    );
    assert!(
        referenced
            .iter()
            .any(|path| path.as_str() == "protocol.rhai")
    );

    let available: BTreeSet<PackageFilePath> = referenced.into_iter().cloned().collect();
    manifest.validate_referenced_files(&available).unwrap();

    let mut missing_schema = available.clone();
    missing_schema.remove(&PackageFilePath::new("document.toml").unwrap());
    let error = manifest
        .validate_referenced_files(&missing_schema)
        .unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageParseErrorCode::ReferencedFileMissing
    );
    assert_eq!(error.field(), "document.schema");

    let mut missing_script = available;
    missing_script.remove(&PackageFilePath::new("protocol.rhai").unwrap());
    let error = manifest
        .validate_referenced_files(&missing_script)
        .unwrap_err();
    assert_eq!(error.field(), "hooks.upstream.receive.script");
}

#[test]
fn oversized_or_sensitive_manifest_input_never_leaks_values() {
    let oversized = format!(
        "{}#{}",
        minimal_manifest(),
        "x".repeat(MAX_MANIFEST_TOML_BYTES)
    );
    let error = parse_protocol_manifest(&oversized).unwrap_err();
    assert_eq!(error.code(), ProtocolPackageParseErrorCode::InputTooLarge);
    assert_eq!(error.field(), "$");

    let secret_path = "/Users/alice/private/payment/protocol.rhai";
    let input = minimal_manifest().replace("scripts/upstream.rhai", secret_path);
    let error = parse_protocol_manifest(&input).unwrap_err();
    let diagnostic = error.to_string();
    assert!(!diagnostic.contains(secret_path));
    assert!(!diagnostic.contains("/Users/alice"));

    let source = "fn steal() { let card = 4111111111111111; }";
    let input = format!("{}\nscript_source = {source:?}\n", minimal_manifest());
    let error = parse_protocol_manifest(&input).unwrap_err();
    assert!(!error.to_string().contains("4111111111111111"));
}
