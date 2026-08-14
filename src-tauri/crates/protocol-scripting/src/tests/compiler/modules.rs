use std::fmt::Write;

use super::common::{compile, minimal_manifest, package, script_error, valid_receive_script};
use crate::ProtocolScriptCompileErrorCode;

#[test]
fn nested_package_modules_with_or_without_rhai_extension_compile() {
    let upstream = concat!(
        "import \"libraries/one\" as one;\n",
        "import \"libraries/two.rhai\" as two;\n",
        "fn frame(reader, context) { one::answer() + two::answer() }\n",
        "fn decode(origin, context) { () }\n",
    );
    let one = b"import \"libraries/two\" as two;\nfn answer() { two::answer() }";
    let two = b"fn answer() { 21 }";
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream.as_bytes()),
            ("downstream.rhai", valid_receive_script().as_bytes()),
            ("libraries/one.rhai", one),
            ("libraries/two.rhai", two),
        ],
    );

    assert!(compile(&files).is_ok());
}

#[test]
fn missing_module_is_rejected_during_import_validation() {
    let upstream =
        b"import \"libraries/missing\" as missing;\nfn frame(r, c) { () }\nfn decode(o, c) { () }";
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream),
            ("downstream.rhai", valid_receive_script().as_bytes()),
        ],
    );

    let error = script_error(compile(&files));
    assert_eq!(error.code(), ProtocolScriptCompileErrorCode::ModuleMissing);
    assert_eq!(error.file().unwrap().as_str(), "libraries/missing.rhai");
}

#[test]
fn absolute_parent_windows_backslash_wrong_extension_and_directory_imports_are_rejected() {
    for import in [
        "../outside",
        "/outside",
        "C:/outside",
        r"libraries\\outside",
        "libraries/outside.js",
        "libraries/",
    ] {
        let upstream = format!(
            "import \"{import}\" as outside;\nfn frame(r, c) {{ () }}\nfn decode(o, c) {{ () }}"
        );
        let files = package(
            minimal_manifest(),
            &[
                ("upstream.rhai", upstream.as_bytes()),
                ("downstream.rhai", valid_receive_script().as_bytes()),
            ],
        );
        let error = script_error(compile(&files));
        assert_eq!(
            error.code(),
            ProtocolScriptCompileErrorCode::ModulePathInvalid,
            "import should fail closed: {import}"
        );
    }
}

#[test]
fn dynamic_import_expression_is_rejected_including_inside_entry_functions() {
    let upstream = concat!(
        "fn frame(reader, context) {\n",
        "    let module_name = \"libraries/one\";\n",
        "    import module_name as one;\n",
        "    one::answer()\n",
        "}\n",
        "fn decode(origin, context) { () }\n",
    );
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream.as_bytes()),
            ("downstream.rhai", valid_receive_script().as_bytes()),
            ("libraries/one.rhai", b"fn answer() { 42 }"),
        ],
    );

    let error = script_error(compile(&files));
    assert_eq!(
        error.code(),
        ProtocolScriptCompileErrorCode::ModulePathInvalid
    );
    assert_eq!(error.file().unwrap().as_str(), "upstream.rhai");
    assert_eq!(error.line(), Some(3));
}

#[test]
fn dynamic_import_inside_an_imported_module_is_rejected_before_ast_is_frozen() {
    let upstream = b"import \"library\" as library;\nfn frame(r, c) { () }\nfn decode(o, c) { () }";
    let library = concat!(
        "fn value() {\n",
        "    let name = \"nested\";\n",
        "    import name as nested;\n",
        "    nested::value()\n",
        "}\n",
    );
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream),
            ("downstream.rhai", valid_receive_script().as_bytes()),
            ("library.rhai", library.as_bytes()),
            ("nested.rhai", b"fn value() { 1 }"),
        ],
    );

    let error = script_error(compile(&files));
    assert_eq!(
        error.code(),
        ProtocolScriptCompileErrorCode::ModulePathInvalid
    );
    assert_eq!(error.file().unwrap().as_str(), "library.rhai");
    assert_eq!(error.line(), Some(3));
}

#[test]
fn direct_and_deep_import_cycles_are_rejected_without_recursion_overflow() {
    for (one, two) in [
        (
            "import \"libraries/one\" as one;\nfn answer() { 1 }",
            "fn answer() { 2 }",
        ),
        (
            "import \"libraries/two\" as two;\nfn answer() { two::answer() }",
            "import \"libraries/one\" as one;\nfn answer() { one::answer() }",
        ),
    ] {
        let upstream = b"import \"libraries/one\" as one;\nfn frame(r, c) { one::answer() }\nfn decode(o, c) { () }";
        let files = package(
            minimal_manifest(),
            &[
                ("upstream.rhai", upstream),
                ("downstream.rhai", valid_receive_script().as_bytes()),
                ("libraries/one.rhai", one.as_bytes()),
                ("libraries/two.rhai", two.as_bytes()),
            ],
        );
        let error = script_error(compile(&files));
        assert_eq!(error.code(), ProtocolScriptCompileErrorCode::ModuleCycle);
    }
}

#[test]
fn module_top_level_only_allows_static_imports_and_scalar_constants() {
    let upstream = b"import \"library\" as library;\nfn frame(r, c) { () }\nfn decode(o, c) { () }";
    let valid = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream),
            ("downstream.rhai", valid_receive_script().as_bytes()),
            (
                "library.rhai",
                b"const ANSWER = 42;\nconst LABEL = \"safe\";\nfn value() { ANSWER }",
            ),
        ],
    );
    assert!(compile(&valid).is_ok());

    for module in [
        "throw \"initialization failed\";\nfn value() { 1 }",
        "while true { }\nfn value() { 1 }",
        "let value = 1;\nfn read() { value }",
        "const VALUES = [1, 2, 3];\nfn read() { VALUES }",
        "const VALUE = make_value();\nfn make_value() { 1 }",
    ] {
        let files = package(
            minimal_manifest(),
            &[
                ("upstream.rhai", upstream),
                ("downstream.rhai", valid_receive_script().as_bytes()),
                ("library.rhai", module.as_bytes()),
            ],
        );
        let error = script_error(compile(&files));
        assert_eq!(error.code(), ProtocolScriptCompileErrorCode::ForbiddenApi);
        assert_eq!(error.file().unwrap().as_str(), "library.rhai");
    }
}

#[test]
fn deeply_nested_module_initialization_is_rejected_before_evaluation() {
    let upstream = b"import \"library\" as library;\nfn frame(r, c) { () }\nfn decode(o, c) { () }";
    let module = concat!(
        "let value = [];\n",
        "for depth in 0..50000 { value = [value]; }\n",
        "fn read() { value }\n",
    );
    let files = package(
        minimal_manifest(),
        &[
            ("upstream.rhai", upstream),
            ("downstream.rhai", valid_receive_script().as_bytes()),
            ("library.rhai", module.as_bytes()),
        ],
    );

    let error = script_error(compile(&files));
    assert_eq!(error.code(), ProtocolScriptCompileErrorCode::ForbiddenApi);
    assert_eq!(error.file().unwrap().as_str(), "library.rhai");
    assert_eq!(error.line(), Some(1));
}

#[test]
fn package_function_count_accepts_512_and_rejects_513_across_modules() {
    for (module_one_count, should_pass) in [(254_usize, true), (255, false)] {
        let upstream = concat!(
            "import \"modules/one\" as one;\n",
            "import \"modules/two\" as two;\n",
            "fn frame(r, c) { () }\n",
            "fn decode(o, c) { () }\n",
        );
        let module_one = functions(module_one_count, "one");
        let module_two = functions(254, "two");
        let files = package(
            minimal_manifest(),
            &[
                ("upstream.rhai", upstream.as_bytes()),
                ("downstream.rhai", valid_receive_script().as_bytes()),
                ("modules/one.rhai", module_one.as_bytes()),
                ("modules/two.rhai", module_two.as_bytes()),
            ],
        );

        let result = compile(&files);
        if should_pass {
            assert!(result.is_ok());
        } else {
            let error = script_error(result);
            assert_eq!(
                error.code(),
                ProtocolScriptCompileErrorCode::CompilationLimitExceeded
            );
            assert_eq!(error.file().unwrap().as_str(), "upstream.rhai");
        }
    }
}

#[test]
fn package_module_count_accepts_exact_limit_and_rejects_one_above() {
    for (count, should_pass) in [(64_usize, true), (65, false)] {
        let mut upstream = String::new();
        let mut owned_modules = Vec::new();
        for index in 0..count {
            writeln!(upstream, "import \"modules/m{index}\" as m{index};").unwrap();
            owned_modules.push((
                format!("modules/m{index}.rhai"),
                format!("fn value() {{ {index} }}"),
            ));
        }
        upstream.push_str("fn frame(r, c) { () }\nfn decode(o, c) { () }\n");

        let mut entries = vec![
            ("upstream.rhai", upstream.as_bytes()),
            ("downstream.rhai", valid_receive_script().as_bytes()),
        ];
        for (name, module) in &owned_modules {
            entries.push((name.as_str(), module.as_bytes()));
        }
        let result = compile(&package(minimal_manifest(), &entries));
        if should_pass {
            assert!(result.is_ok());
        } else {
            let error = script_error(result);
            assert_eq!(
                error.code(),
                ProtocolScriptCompileErrorCode::CompilationLimitExceeded
            );
            assert!(error.file().unwrap().as_str().starts_with("modules/m"));
        }
    }
}

fn functions(count: usize, prefix: &str) -> String {
    let mut output = String::new();
    for index in 0..count {
        writeln!(output, "fn {prefix}_{index}() {{ {index} }}").unwrap();
    }
    output
}
