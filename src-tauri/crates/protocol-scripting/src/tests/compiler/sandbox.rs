use super::common::{compile, minimal_manifest, package, script_error};
use crate::{ProtocolRuntimeLimits, ProtocolScriptCompileErrorCode, compiler::build_engine};

#[test]
fn compiler_engine_is_send_sync_and_disables_source_evaluation_and_output_symbols() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<rhai::Engine>();
    assert_send_sync::<rhai::AST>();

    let engine = build_engine(ProtocolRuntimeLimits::default());
    assert!(engine.is_symbol_disabled("eval"));
    assert!(engine.is_symbol_disabled("print"));
    assert!(engine.is_symbol_disabled("debug"));
    assert!(engine.is_symbol_disabled("timestamp"));
    assert!(engine.is_symbol_disabled("read_file"));
    assert!(engine.is_symbol_disabled("socket"));
    assert_eq!(engine.max_operations(), 100_000);
    assert_eq!(engine.max_call_levels(), 32);
    assert_eq!(engine.max_string_size(), 64 * 1024);
    assert_eq!(engine.max_array_size(), 1024 * 1024);
    assert_eq!(engine.max_modules(), 64);
}

#[test]
fn disabled_float_time_and_closure_language_features_fail_during_compilation() {
    for expression in [
        "let value = 1.5;",
        "let value = timestamp();",
        "let value = |x| x + 1;",
    ] {
        let upstream = format!(
            "fn frame(reader, context) {{ {expression} }}\nfn decode(origin, context) {{ () }}\nfn encode(origin, document, context) {{ origin }}"
        );
        let files = package(
            minimal_manifest(),
            &[("protocol.rhai", upstream.as_bytes())],
        );
        let result = compile(&files);
        assert!(
            result.is_err(),
            "disabled expression compiled: {expression}"
        );
        let error = script_error(result);
        assert!(matches!(
            error.code(),
            ProtocolScriptCompileErrorCode::ScriptSyntaxInvalid
                | ProtocolScriptCompileErrorCode::ForbiddenApi
        ));
    }
}

#[test]
fn file_network_process_and_environment_capability_names_are_forbidden() {
    for expression in [
        "read_file(\"secret.txt\");",
        "write_file(\"secret.txt\", \"data\");",
        "socket(\"127.0.0.1:1\");",
        "process::spawn(\"command\");",
        "env(\"HOME\");",
    ] {
        let upstream = format!(
            "fn frame(reader, context) {{ {expression} }}\nfn decode(origin, context) {{ () }}\nfn encode(origin, document, context) {{ origin }}"
        );
        let files = package(
            minimal_manifest(),
            &[("protocol.rhai", upstream.as_bytes())],
        );
        let result = compile(&files);
        assert!(
            result.is_err(),
            "forbidden capability compiled: {expression}"
        );
        let error = script_error(result);
        assert_eq!(error.code(), ProtocolScriptCompileErrorCode::ForbiddenApi);
    }
}

#[test]
fn entry_bodies_are_compiled_but_never_executed_during_t08() {
    let upstream = concat!(
        "fn frame(reader, context) { while true { } }\n",
        "fn decode(origin, context) { throw \"not executed during import\"; }\n",
        "fn encode(origin, document, context) { origin }\n",
    );
    let files = package(
        minimal_manifest(),
        &[("protocol.rhai", upstream.as_bytes())],
    );

    assert!(compile(&files).is_ok());
}
