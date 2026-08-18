use super::common::{
    compile, manifest_with_all_optionals, minimal_manifest, package, script_error,
    valid_full_script,
};
use crate::ProtocolScriptCompileErrorCode;

#[test]
fn missing_manifest_entry_is_rejected_after_real_script_compilation() {
    let upstream = concat!(
        "fn decode(origin, context) { () }\n",
        "fn encode(origin, document, context) { origin }\n",
    )
    .as_bytes();
    let files = package(minimal_manifest(), &[("protocol.rhai", upstream)]);

    let error = script_error(compile(&files));
    assert_eq!(
        error.code(),
        ProtocolScriptCompileErrorCode::EntryPointMissing
    );
    assert_eq!(error.file().unwrap().as_str(), "protocol.rhai");
    assert_eq!(error.entry().unwrap().as_str(), "frame");
    assert_eq!(error.expected_arity(), Some(2));
    assert!(error.available_arities().is_empty());
}

#[test]
fn frame_decode_encode_and_display_arity_contracts_are_checked_independently() {
    let cases = [
        (
            minimal_manifest(),
            "fn frame(reader) { () }\nfn decode(origin, context) { () }\nfn encode(origin, document, context) { origin }",
            None,
            "frame",
            2,
            vec![1],
        ),
        (
            minimal_manifest(),
            "fn frame(reader, context) { () }\nfn decode(origin, context, extra) { () }\nfn encode(origin, document, context) { origin }",
            None,
            "decode",
            2,
            vec![3],
        ),
        (
            manifest_with_all_optionals(),
            "fn frame(r, c) { () }\nfn decode(o, c) { () }\nfn encode(origin, document) { origin }",
            Some("fn display(document, context) { \"ok\" }"),
            "encode",
            3,
            vec![2],
        ),
        (
            manifest_with_all_optionals(),
            valid_full_script(),
            Some("fn display(document) { \"ok\" }"),
            "display",
            2,
            vec![1],
        ),
    ];

    for (manifest, protocol, display, entry, expected, available) in cases {
        let mut scripts = vec![("protocol.rhai", protocol.as_bytes())];
        if let Some(display) = display {
            scripts.push(("display.rhai", display.as_bytes()));
        }
        let error = script_error(compile(&package(manifest, &scripts)));
        assert_eq!(
            error.code(),
            ProtocolScriptCompileErrorCode::EntryPointArityMismatch
        );
        assert_eq!(error.entry().unwrap().as_str(), entry);
        assert_eq!(error.expected_arity(), Some(expected));
        assert_eq!(error.available_arities(), available);
    }
}

#[test]
fn private_entry_is_not_treated_as_a_host_callable_function() {
    let upstream = concat!(
        "private fn frame(reader, context) { () }\n",
        "fn decode(origin, context) { () }\n",
        "fn encode(origin, document, context) { origin }\n",
    )
    .as_bytes();
    let files = package(minimal_manifest(), &[("protocol.rhai", upstream)]);

    let error = script_error(compile(&files));
    assert_eq!(
        error.code(),
        ProtocolScriptCompileErrorCode::EntryPointNotPublic
    );
    assert_eq!(error.entry().unwrap().as_str(), "frame");
    assert!(error.available_arities().is_empty());
}

#[test]
fn function_nested_inside_another_function_is_rejected_by_standard_rhai_syntax() {
    let upstream = concat!(
        "fn wrapper() { fn frame(reader, context) { () } }\n",
        "fn decode(origin, context) { () }\n",
        "fn encode(origin, document, context) { origin }\n",
    );
    let files = package(
        minimal_manifest(),
        &[("protocol.rhai", upstream.as_bytes())],
    );

    let error = script_error(compile(&files));
    assert_eq!(
        error.code(),
        ProtocolScriptCompileErrorCode::ScriptSyntaxInvalid
    );
    assert_eq!(error.file().unwrap().as_str(), "protocol.rhai");
}

#[test]
fn overloads_are_allowed_when_one_public_top_level_signature_matches_host_api() {
    let upstream = concat!(
        "fn frame(reader) { () }\n",
        "fn frame(reader, context) { () }\n",
        "fn decode(origin, context) { () }\n",
        "fn encode(origin, document, context) { origin }\n",
    );
    let files = package(
        minimal_manifest(),
        &[("protocol.rhai", upstream.as_bytes())],
    );

    assert!(compile(&files).is_ok());
}
