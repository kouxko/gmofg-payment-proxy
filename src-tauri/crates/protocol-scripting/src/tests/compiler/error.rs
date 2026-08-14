use std::error::Error;

use rhai::Position;

use super::common::path;
use crate::{
    ProtocolFunctionName, ProtocolPackageCompilationError, ProtocolScriptCompileError,
    ProtocolScriptCompileErrorCode,
};

#[test]
fn every_script_compile_error_code_has_a_stable_wire_value() {
    for (code, wire) in [
        (
            ProtocolScriptCompileErrorCode::ScriptNotUtf8,
            "SCRIPT_NOT_UTF8",
        ),
        (
            ProtocolScriptCompileErrorCode::ScriptSyntaxInvalid,
            "SCRIPT_SYNTAX_INVALID",
        ),
        (
            ProtocolScriptCompileErrorCode::ForbiddenApi,
            "FORBIDDEN_API",
        ),
        (
            ProtocolScriptCompileErrorCode::ModulePathInvalid,
            "MODULE_PATH_INVALID",
        ),
        (
            ProtocolScriptCompileErrorCode::ModuleMissing,
            "MODULE_MISSING",
        ),
        (ProtocolScriptCompileErrorCode::ModuleCycle, "MODULE_CYCLE"),
        (
            ProtocolScriptCompileErrorCode::ModuleInitializationFailed,
            "MODULE_INITIALIZATION_FAILED",
        ),
        (
            ProtocolScriptCompileErrorCode::CompilationLimitExceeded,
            "COMPILATION_LIMIT_EXCEEDED",
        ),
        (
            ProtocolScriptCompileErrorCode::EntryPointMissing,
            "ENTRY_POINT_MISSING",
        ),
        (
            ProtocolScriptCompileErrorCode::EntryPointNotPublic,
            "ENTRY_POINT_NOT_PUBLIC",
        ),
        (
            ProtocolScriptCompileErrorCode::EntryPointArityMismatch,
            "ENTRY_POINT_ARITY_MISMATCH",
        ),
    ] {
        assert_eq!(code.as_str(), wire);
        assert_eq!(code.to_string(), wire);
        assert_eq!(serde_json::to_value(code).unwrap(), wire);
    }
}

#[test]
fn script_diagnostics_only_serialize_bounded_safe_context() {
    let error = ProtocolScriptCompileError::entry_failure(
        ProtocolScriptCompileErrorCode::EntryPointArityMismatch,
        path("scripts/main.rhai"),
        ProtocolFunctionName::new("decode").unwrap(),
        2,
        vec![4, 1, 4],
    );

    assert_eq!(error.available_arities(), &[1, 4]);
    assert!(error.source().is_none());
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({
            "code": "ENTRY_POINT_ARITY_MISMATCH",
            "file": "scripts/main.rhai",
            "entry": "decode",
            "expected_arity": 2,
            "available_arities": [1, 4]
        })
    );
}

#[test]
fn package_compilation_error_exposes_exactly_one_phase() {
    let script = ProtocolScriptCompileError::script(
        ProtocolScriptCompileErrorCode::ScriptSyntaxInvalid,
        path("main.rhai"),
        Position::new(3, 7),
    );
    let package_error = ProtocolPackageCompilationError::from(script.clone());

    assert!(package_error.declaration_error().is_none());
    assert_eq!(package_error.script_error(), Some(&script));
    assert!(package_error.source().is_none());
}

#[test]
fn diagnostics_without_a_safe_file_have_no_optional_context() {
    let error = ProtocolScriptCompileError::module_without_file(
        ProtocolScriptCompileErrorCode::ModulePathInvalid,
    );
    assert!(error.file().is_none());
    assert!(error.entry().is_none());
    assert_eq!(error.line(), None);
    assert_eq!(error.column(), None);
    assert_eq!(error.expected_arity(), None);
    assert!(error.available_arities().is_empty());
}
