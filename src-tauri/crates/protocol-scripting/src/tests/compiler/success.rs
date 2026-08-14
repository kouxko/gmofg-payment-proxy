use std::ptr;

use super::common::{
    compile, manifest_with_all_optionals, package, valid_full_script, valid_minimal_package,
};
use crate::tests::fixtures::{
    TEMPLATE_DISPLAY, TEMPLATE_LIBRARY, TEMPLATE_MANIFEST, TEMPLATE_PROTOCOL, TEMPLATE_SCHEMA,
};

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
    assert_eq!(compiled.schema().id().as_str(), "iso8583-financial-message");
    assert!(compiled.supports_display());
    assert!(compiled.supports_upstream_encode());
    assert!(compiled.supports_downstream_encode());

    let upstream = compiled.upstream();
    assert_eq!(upstream.frame().script().as_str(), "protocol.rhai");
    assert_eq!(upstream.frame().function().as_str(), "frame");
    assert_eq!(upstream.frame().ast().source(), Some("protocol.rhai"));
    assert!(ptr::eq(upstream.frame().ast(), upstream.decode().ast()));
    assert_eq!(compiled.display().unwrap().function().as_str(), "display");
}

#[test]
fn same_entry_names_in_distinct_direction_scripts_are_compiled_independently() {
    let compiled = compile(&valid_minimal_package()).unwrap();

    assert_eq!(compiled.upstream().frame().function().as_str(), "frame");
    assert_eq!(compiled.downstream().frame().function().as_str(), "frame");
    assert_eq!(
        compiled.upstream().frame().ast().source(),
        Some("upstream.rhai")
    );
    assert_eq!(
        compiled.downstream().frame().ast().source(),
        Some("downstream.rhai")
    );
    assert!(!ptr::eq(
        compiled.upstream().frame().ast(),
        compiled.downstream().frame().ast()
    ));
    assert!(!compiled.supports_display());
    assert!(!compiled.supports_upstream_encode());
    assert!(!compiled.supports_downstream_encode());
}

#[test]
fn optional_display_and_direction_encoders_only_exist_when_declared_and_valid() {
    let display = b"fn display(document, context) { \"<p>ok</p>\" }";
    let files = package(
        manifest_with_all_optionals(),
        &[
            ("upstream.rhai", valid_full_script().as_bytes()),
            ("downstream.rhai", valid_full_script().as_bytes()),
            ("display.rhai", display),
        ],
    );

    let compiled = compile(&files).unwrap();
    assert!(compiled.supports_display());
    assert!(compiled.supports_upstream_encode());
    assert!(compiled.supports_downstream_encode());
    assert_eq!(
        compiled.upstream().encode().unwrap().function().as_str(),
        "encode"
    );
    assert_eq!(compiled.display().unwrap().entry().to_string(), "display");
    let entry_debug = format!("{:?}", compiled.display().unwrap());
    assert!(entry_debug.contains("display.rhai"));
    assert!(!entry_debug.contains("<p>ok</p>"));

    let debug = format!("{compiled:?}");
    assert!(debug.contains("iso8583") || debug.contains("compiler-test"));
    assert!(!debug.contains("fn display"));
    assert!(!debug.contains("<p>ok</p>"));
}
