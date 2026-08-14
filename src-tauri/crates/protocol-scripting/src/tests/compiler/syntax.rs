use std::error::Error;

use super::common::{compile, minimal_manifest, package, script_error, valid_receive_script};
use crate::{
    ProtocolPackageCompilationError, ProtocolPackageFile, ProtocolPackageParseErrorCode,
    ProtocolScriptCompileErrorCode,
};

#[test]
fn main_script_syntax_error_reports_safe_file_line_and_column() {
    let broken =
        b"fn frame(reader, context) {\n    let value = ;\n}\nfn decode(origin, context) { () }";
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", broken),
            ("downstream.rhai", valid_receive_script().as_bytes()),
        ],
    );

    let error = script_error(compile(&files));
    assert_eq!(
        error.code(),
        ProtocolScriptCompileErrorCode::ScriptSyntaxInvalid
    );
    assert_eq!(error.file().unwrap().as_str(), "upstream.rhai");
    assert_eq!(error.line(), Some(2));
    assert!(error.column().is_some());
    assert!(error.entry().is_none());
    assert!(error.source().is_none());

    let wire = serde_json::to_string(&error).unwrap();
    assert!(!wire.contains("let value"));
    assert!(!wire.contains("/Users/"));
}

#[test]
fn syntax_error_in_a_deep_import_reports_the_actual_module() {
    let upstream = concat!(
        "import \"libraries/one\" as one;\n",
        "fn frame(reader, context) { one::value() }\n",
        "fn decode(origin, context) { () }\n",
    );
    let one = b"import \"libraries/two\" as two;\nfn value() { two::value() }";
    let two = b"fn value( { 42 }";
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream.as_bytes()),
            ("downstream.rhai", valid_receive_script().as_bytes()),
            ("libraries/one.rhai", one),
            ("libraries/two.rhai", two),
        ],
    );

    let error = script_error(compile(&files));
    assert_eq!(
        error.code(),
        ProtocolScriptCompileErrorCode::ScriptSyntaxInvalid
    );
    assert_eq!(error.file().unwrap().as_str(), "libraries/two.rhai");
    assert_eq!(error.line(), Some(1));
}

#[test]
fn non_utf8_main_and_module_scripts_are_rejected_without_echoing_bytes() {
    let main_files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", &[0xff, 0xfe]),
            ("downstream.rhai", valid_receive_script().as_bytes()),
        ],
    );
    let main_error = script_error(compile(&main_files));
    assert_eq!(
        main_error.code(),
        ProtocolScriptCompileErrorCode::ScriptNotUtf8
    );
    assert_eq!(main_error.file().unwrap().as_str(), "upstream.rhai");

    let upstream = b"import \"library\" as library;\nfn frame(r, c) { () }\nfn decode(o, c) { () }";
    let module_files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream),
            ("downstream.rhai", valid_receive_script().as_bytes()),
            ("library.rhai", &[0xff]),
        ],
    );
    let module_error = script_error(compile(&module_files));
    assert_eq!(
        module_error.code(),
        ProtocolScriptCompileErrorCode::ScriptNotUtf8
    );
    assert_eq!(module_error.file().unwrap().as_str(), "library.rhai");
}

#[test]
fn eval_print_and_debug_are_forbidden_at_compile_time() {
    for forbidden in ["eval(\"40 + 2\")", "print(\"secret\")", "debug(\"secret\")"] {
        let upstream = format!(
            "fn frame(reader, context) {{ {forbidden} }}\nfn decode(origin, context) {{ () }}"
        );
        let files = package(
            minimal_manifest(),
            &[
                ("upstream.rhai", upstream.as_bytes()),
                ("downstream.rhai", valid_receive_script().as_bytes()),
            ],
        );
        let error = script_error(compile(&files));
        assert_eq!(error.code(), ProtocolScriptCompileErrorCode::ForbiddenApi);
        assert_eq!(error.file().unwrap().as_str(), "upstream.rhai");
    }
}

#[test]
fn declaration_failures_remain_typed_and_separate_from_rhai_errors() {
    let invalid_manifest = package(
        [0xff, 0xfe],
        &[
            ("upstream.rhai", valid_receive_script().as_bytes()),
            ("downstream.rhai", valid_receive_script().as_bytes()),
        ],
    );
    let error = compile(&invalid_manifest).unwrap_err();
    assert!(error.script_error().is_none());
    let declaration = error.declaration_error().unwrap();
    assert_eq!(
        declaration.code(),
        ProtocolPackageParseErrorCode::TomlInvalid
    );
    assert_eq!(declaration.file(), ProtocolPackageFile::Manifest);

    let invalid_schema = package(
        minimal_manifest(),
        &[
            ("document.toml", &[0xff, 0xfe]),
            ("upstream.rhai", valid_receive_script().as_bytes()),
            ("downstream.rhai", valid_receive_script().as_bytes()),
        ],
    );
    let schema_error = compile(&invalid_schema).unwrap_err();
    let declaration = schema_error.declaration_error().unwrap();
    assert_eq!(
        declaration.code(),
        ProtocolPackageParseErrorCode::TomlInvalid
    );
    assert_eq!(declaration.file(), ProtocolPackageFile::DocumentSchema);

    let missing_script = package(
        minimal_manifest(),
        &[("upstream.rhai", valid_receive_script().as_bytes())],
    );
    match compile(&missing_script).unwrap_err() {
        ProtocolPackageCompilationError::Declaration(error) => {
            assert_eq!(
                error.code(),
                ProtocolPackageParseErrorCode::ReferencedFileMissing
            );
            assert_eq!(error.field(), "hooks.downstream.receive.script");
        }
        ProtocolPackageCompilationError::Script(error) => {
            panic!("expected reference error before Rhai compilation: {error}")
        }
    }
}

#[test]
fn missing_schema_is_reported_before_other_manifest_references_are_compiled() {
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", valid_receive_script().as_bytes()),
            ("downstream.rhai", valid_receive_script().as_bytes()),
        ],
    );
    let mut without_schema = std::collections::BTreeMap::new();
    for (path, bytes) in files.iter() {
        if path.as_str() != "document.toml" {
            without_schema.insert(path.clone(), bytes.to_vec());
        }
    }
    let total = without_schema.values().map(Vec::len).sum::<usize>();
    let files = crate::ProtocolPackageFiles::new(without_schema, u64::try_from(total).unwrap());

    let error = compile(&files).unwrap_err();
    let declaration = error.declaration_error().unwrap();
    assert_eq!(
        declaration.code(),
        ProtocolPackageParseErrorCode::ReferencedFileMissing
    );
    assert_eq!(declaration.field(), "document.schema");
}
