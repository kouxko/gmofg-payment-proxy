use std::collections::BTreeSet;

use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};

use super::fixtures::{TEMPLATE_MANIFEST, minimal_manifest};
use crate::{
    MAX_MANIFEST_TOML_BYTES, PackageFilePath, ProtocolPackageFile, ProtocolPackageKind,
    ProtocolPackageParseErrorCode, SUPPORTED_PROTOCOL_HOST_API, parse_protocol_manifest,
};

#[test]
fn official_template_is_a_complete_directional_socket_package() {
    let manifest = parse_protocol_manifest(TEMPLATE_MANIFEST).unwrap();
    assert_eq!(manifest.api(), SUPPORTED_PROTOCOL_HOST_API);
    assert_eq!(manifest.kind(), ProtocolPackageKind::Socket);
    assert_eq!(
        manifest.package().package().id,
        ProtocolPackageId::new("iso8583-ascii-standard").unwrap()
    );
    assert_eq!(
        manifest.package().package().version,
        ProtocolPackageVersion::new("1.0.0").unwrap()
    );
    assert_eq!(manifest.package().name(), "ISO 8583:1987 ASCII Profile");
    for document in [
        manifest.document().upstream(),
        manifest.document().downstream(),
    ] {
        assert_eq!(document.schema().as_str(), "document.toml");
        assert_eq!(document.display().script().as_str(), "display.rhai");
        assert_eq!(document.display().function().as_str(), "display");
    }
    for hooks in [manifest.hooks().upstream(), manifest.hooks().downstream()] {
        assert_eq!(hooks.script().as_str(), "protocol.rhai");
        assert_eq!(hooks.frame().unwrap().as_str(), "frame");
        assert_eq!(hooks.decode().as_str(), "decode");
        assert_eq!(hooks.encode().as_str(), "encode");
    }
}

#[test]
fn no_frame_in_either_direction_is_http_and_directional_schemas_stay_distinct() {
    let input = minimal_manifest()
        .replace("frame = \"upstream_frame\"\n", "")
        .replace("frame = \"downstream_frame\"\n", "");
    let manifest = parse_protocol_manifest(&input).unwrap();
    assert_eq!(manifest.kind(), ProtocolPackageKind::Http);
    assert_eq!(
        manifest.document().upstream().schema().as_str(),
        "schemas/upstream.toml"
    );
    assert_eq!(
        manifest.document().downstream().schema().as_str(),
        "schemas/downstream.toml"
    );
    assert_eq!(
        manifest.document().upstream().display().function().as_str(),
        "render_upstream"
    );
    assert_eq!(
        manifest
            .document()
            .downstream()
            .display()
            .function()
            .as_str(),
        "render_downstream"
    );
}

#[test]
fn frame_must_be_declared_by_both_directions_or_neither() {
    for missing in [
        "frame = \"upstream_frame\"\n",
        "frame = \"downstream_frame\"\n",
    ] {
        let error = parse_protocol_manifest(&minimal_manifest().replace(missing, "")).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::ManifestInvalid);
        assert_eq!(error.field(), "hooks.frame");
    }
}

#[test]
fn old_manifest_shapes_and_content_type_routing_are_rejected() {
    let cases = [
        minimal_manifest().replace("[document.upstream]", "[document]"),
        format!(
            "{}\n[http]\ncontent_types = [\"application/json\"]\n",
            minimal_manifest()
        ),
        minimal_manifest().replace(
            "display = \"render_upstream\"",
            "display = { script = \"display.rhai\", function = \"render_upstream\" }",
        ),
        minimal_manifest().replace(
            "[hooks.upstream]",
            "[hooks.upstream.receive]\nscript = \"protocol.rhai\"",
        ),
    ];
    for input in cases {
        let error = parse_protocol_manifest(&input).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
    }
}

#[test]
fn strict_manifest_rejects_missing_unknown_duplicate_and_wrong_shapes() {
    let base = minimal_manifest();
    let cases = [
        String::new(),
        base.replace("api = 1\n", ""),
        base.replace("[hooks.downstream]", "[hooks.sideways]"),
        base.replace("decode = \"downstream_decode\"\n", ""),
        base.replace("encode = \"upstream_encode\"\n", ""),
        base.replace("api = 1", "api = \"one\""),
        base.replace(
            "id = \"example-protocol\"",
            "id = \"example-protocol\"\nid = \"duplicate\"",
        ),
        format!("{base}\nunknown = true\n"),
    ];
    for input in cases {
        let error = parse_protocol_manifest(&input).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
        assert_eq!(error.file(), ProtocolPackageFile::Manifest);
    }
}

#[test]
fn manifest_semantic_errors_report_stable_fields() {
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
            base.replace(
                "name = \"Example Protocol\"",
                &format!("name = \"{long_name}\""),
            ),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "package.name",
        ),
        (
            base.replace("schemas/upstream.toml", "../upstream.toml"),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "document.upstream.schema",
        ),
        (
            base.replace("frame = \"upstream_frame\"", "frame = \"while\""),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "hooks.upstream.frame",
        ),
        (
            base.replace(
                "decode = \"downstream_decode\"",
                &format!("decode = \"{long_function}\""),
            ),
            ProtocolPackageParseErrorCode::ManifestInvalid,
            "hooks.downstream.decode",
        ),
    ];
    for (input, code, field) in cases {
        let error = parse_protocol_manifest(&input).unwrap_err();
        assert_eq!(error.code(), code);
        assert_eq!(error.field(), field);
    }
}

#[test]
fn manifest_referenced_files_are_deduplicated_and_validated() {
    let manifest = parse_protocol_manifest(TEMPLATE_MANIFEST).unwrap();
    let referenced = manifest.referenced_files();
    assert_eq!(referenced.len(), 3);
    let available: BTreeSet<PackageFilePath> = referenced.into_iter().cloned().collect();
    manifest.validate_referenced_files(&available).unwrap();

    let mut missing_schema = available.clone();
    missing_schema.remove(&PackageFilePath::new("document.toml").unwrap());
    let error = manifest
        .validate_referenced_files(&missing_schema)
        .unwrap_err();
    assert_eq!(error.field(), "document.upstream.schema");

    let mut missing_script = available;
    missing_script.remove(&PackageFilePath::new("protocol.rhai").unwrap());
    let error = manifest
        .validate_referenced_files(&missing_script)
        .unwrap_err();
    assert_eq!(error.field(), "hooks.upstream");
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

    let secret = "/Users/alice/private/payment/document.toml";
    let error =
        parse_protocol_manifest(&minimal_manifest().replace("schemas/upstream.toml", secret))
            .unwrap_err();
    assert!(!error.to_string().contains(secret));
}
