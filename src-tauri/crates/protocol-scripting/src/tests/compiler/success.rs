use std::ptr;

use super::common::{
    compile, manifest_with_all_optionals, package, valid_full_script, valid_minimal_package,
};
use crate::ProtocolDirection;
use crate::tests::fixtures::{
    TEMPLATE_DISPLAY, TEMPLATE_LIBRARY, TEMPLATE_MANIFEST, TEMPLATE_PROTOCOL, TEMPLATE_SCHEMA,
};

const UPSTREAM_SCHEMA: &str = r#"id = "upstream-message"
version = 1
title = "Upstream Message"

[[fields]]
name = "request_code"
label = "Request Code"
type = "string"
"#;

const DOWNSTREAM_SCHEMA: &str = r#"id = "downstream-message"
version = 2
title = "Downstream Message"

[[fields]]
name = "response_code"
label = "Response Code"
type = "int"
"#;

fn directional_manifest(frame: bool) -> String {
    let frame = if frame { "frame = \"frame\"\n" } else { "" };
    format!(
        r#"api = 1

[package]
id = "directional-test"
name = "Directional Test"
version = "1.0.0"

[document.upstream]
schema = "schemas/upstream.toml"
display = "display"

[document.downstream]
schema = "schemas/downstream.toml"
display = "display"

[hooks.upstream]
{frame}decode = "decode"
encode = "encode"

[hooks.downstream]
{frame}decode = "decode"
encode = "encode"
"#
    )
}

fn assert_distinct_directional_schemas(compiled: &crate::CompiledProtocolPackage) {
    let upstream = compiled.schema(ProtocolDirection::Upstream);
    let downstream = compiled.schema(ProtocolDirection::Downstream);

    assert_eq!(upstream.id().as_str(), "upstream-message");
    assert_eq!(upstream.version(), 1);
    assert_eq!(upstream.fields()[0].name().as_str(), "request_code");
    assert_eq!(downstream.id().as_str(), "downstream-message");
    assert_eq!(downstream.version(), 2);
    assert_eq!(downstream.fields()[0].name().as_str(), "response_code");
    assert!(!ptr::eq(upstream, downstream));
    assert_eq!(compiled.upstream().decode().function().as_str(), "decode");
    assert_eq!(compiled.downstream().decode().function().as_str(), "decode");
    assert_eq!(compiled.upstream().encode().function().as_str(), "encode");
    assert_eq!(compiled.downstream().encode().function().as_str(), "encode");
    assert_eq!(
        compiled
            .display(ProtocolDirection::Upstream)
            .function()
            .as_str(),
        "display"
    );
    assert_eq!(
        compiled
            .display(ProtocolDirection::Downstream)
            .function()
            .as_str(),
        "display"
    );
}

#[test]
fn http_package_compiles_distinct_upstream_and_downstream_schemas() {
    let files = package(
        directional_manifest(false),
        &[
            ("schemas/upstream.toml", UPSTREAM_SCHEMA.as_bytes()),
            ("schemas/downstream.toml", DOWNSTREAM_SCHEMA.as_bytes()),
        ],
    );

    let compiled = compile(&files).unwrap();
    assert_eq!(compiled.kind(), crate::ProtocolPackageKind::Http);
    assert_distinct_directional_schemas(&compiled);
}

#[test]
fn socket_package_compiles_distinct_upstream_and_downstream_schemas() {
    let files = package(
        directional_manifest(true),
        &[
            ("schemas/upstream.toml", UPSTREAM_SCHEMA.as_bytes()),
            ("schemas/downstream.toml", DOWNSTREAM_SCHEMA.as_bytes()),
        ],
    );

    let compiled = compile(&files).unwrap();
    assert_eq!(compiled.kind(), crate::ProtocolPackageKind::Socket);
    assert_distinct_directional_schemas(&compiled);
    assert_eq!(
        compiled.upstream().frame().unwrap().function().as_str(),
        "frame"
    );
    assert_eq!(
        compiled.downstream().frame().unwrap().function().as_str(),
        "frame"
    );
}

#[test]
fn official_iso8583_template_compiles_with_real_rhai_and_package_modules() {
    let files = package(
        TEMPLATE_MANIFEST,
        &[
            ("document.toml", TEMPLATE_SCHEMA.as_bytes()),
            ("protocol.rhai", TEMPLATE_PROTOCOL),
            ("display.rhai", TEMPLATE_DISPLAY),
            ("libraries/iso8583.rhai", TEMPLATE_LIBRARY),
        ],
    );

    let compiled = compile(&files).unwrap();
    assert_eq!(compiled.package().id.as_str(), "iso8583-ascii-standard");
    assert_eq!(compiled.package().version.as_str(), "1.0.0");
    assert_eq!(
        compiled.schema(ProtocolDirection::Upstream).id().as_str(),
        "iso8583-financial-message"
    );
    assert!(compiled.supports_display());
    assert!(compiled.supports_upstream_encode());
    assert!(compiled.supports_downstream_encode());

    let upstream = compiled.upstream();
    assert_eq!(upstream.frame().unwrap().script().as_str(), "protocol.rhai");
    assert_eq!(upstream.frame().unwrap().function().as_str(), "frame");
    assert_eq!(
        upstream.frame().unwrap().ast().source(),
        Some("protocol.rhai")
    );
    assert!(ptr::eq(
        upstream.frame().unwrap().ast(),
        upstream.decode().ast()
    ));
    assert_eq!(
        compiled
            .display(ProtocolDirection::Upstream)
            .function()
            .as_str(),
        "display"
    );
}

#[test]
fn same_entry_names_share_the_fixed_protocol_script() {
    let compiled = compile(&valid_minimal_package()).unwrap();

    assert_eq!(
        compiled.upstream().frame().unwrap().function().as_str(),
        "frame"
    );
    assert_eq!(
        compiled.downstream().frame().unwrap().function().as_str(),
        "frame"
    );
    assert_eq!(
        compiled.upstream().frame().unwrap().ast().source(),
        Some("protocol.rhai")
    );
    assert_eq!(
        compiled.downstream().frame().unwrap().ast().source(),
        Some("protocol.rhai")
    );
    assert!(ptr::eq(
        compiled.upstream().frame().unwrap().ast(),
        compiled.downstream().frame().unwrap().ast()
    ));
    assert!(compiled.supports_display());
    assert!(compiled.supports_upstream_encode());
    assert!(compiled.supports_downstream_encode());
}

#[test]
fn optional_display_and_direction_encoders_only_exist_when_declared_and_valid() {
    let display = b"fn display(document, context) { \"<p>ok</p>\" }";
    let files = package(
        manifest_with_all_optionals(),
        &[
            ("protocol.rhai", valid_full_script().as_bytes()),
            ("display.rhai", display),
        ],
    );

    let compiled = compile(&files).unwrap();
    assert!(compiled.supports_display());
    assert!(compiled.supports_upstream_encode());
    assert!(compiled.supports_downstream_encode());
    assert_eq!(compiled.upstream().encode().function().as_str(), "encode");
    assert_eq!(
        compiled
            .display(ProtocolDirection::Upstream)
            .entry()
            .to_string(),
        "display"
    );
    let entry_debug = format!("{:?}", compiled.display(ProtocolDirection::Upstream));
    assert!(entry_debug.contains("display.rhai"));
    assert!(!entry_debug.contains("<p>ok</p>"));

    let debug = format!("{compiled:?}");
    assert!(debug.contains("iso8583") || debug.contains("compiler-test"));
    assert!(!debug.contains("fn display"));
    assert!(!debug.contains("<p>ok</p>"));
}
