use std::collections::BTreeMap;

use crate::{
    CompiledProtocolPackage, PackageFilePath, ProtocolPackageCompilationError,
    ProtocolPackageCompiler, ProtocolPackageFiles, ProtocolScriptCompileError,
};

pub(super) const DOCUMENT_SCHEMA: &str = r#"id = "test-message"
version = 1
title = "Test Message"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;

pub(super) fn minimal_manifest() -> String {
    r#"api = 1

[package]
id = "compiler-test"
name = "Compiler Test"
version = "1.0.0"

[document.upstream]
schema = "document.toml"
display = "display"

[document.downstream]
schema = "document.toml"
display = "display"

[hooks.upstream]
frame = "frame"
decode = "decode"
encode = "encode"

[hooks.downstream]
frame = "frame"
decode = "decode"
encode = "encode"
"#
    .to_owned()
}

pub(super) fn manifest_with_all_optionals() -> String {
    minimal_manifest()
}

pub(super) fn valid_full_script() -> &'static str {
    concat!(
        "fn frame(reader, context) { () }\n",
        "fn decode(origin, context) { () }\n",
        "fn encode(origin, document, context) { origin }\n",
    )
}

pub(super) fn package(
    manifest: impl Into<Vec<u8>>,
    extra_files: &[(&str, &[u8])],
) -> ProtocolPackageFiles {
    let mut files = BTreeMap::from([
        (path("manifest.toml"), manifest.into()),
        (path("document.toml"), DOCUMENT_SCHEMA.as_bytes().to_vec()),
        (
            path("protocol.rhai"),
            valid_full_script().as_bytes().to_vec(),
        ),
        (
            path("display.rhai"),
            b"fn display(document, context) { \"<p>ok</p>\" }".to_vec(),
        ),
    ]);
    for (name, bytes) in extra_files {
        files.insert(path(name), bytes.to_vec());
    }
    let total = files.values().map(Vec::len).sum::<usize>();
    ProtocolPackageFiles::new(files, u64::try_from(total).unwrap())
}

pub(super) fn valid_minimal_package() -> ProtocolPackageFiles {
    package(
        minimal_manifest(),
        &[
            ("protocol.rhai", valid_full_script().as_bytes()),
            (
                "display.rhai",
                b"fn display(document, context) { \"<p>ok</p>\" }",
            ),
        ],
    )
}

pub(super) fn compile(
    files: &ProtocolPackageFiles,
) -> Result<CompiledProtocolPackage, ProtocolPackageCompilationError> {
    ProtocolPackageCompiler::default().compile(files)
}

pub(super) fn script_error(
    result: Result<CompiledProtocolPackage, ProtocolPackageCompilationError>,
) -> ProtocolScriptCompileError {
    match result.unwrap_err() {
        ProtocolPackageCompilationError::Script(error) => error,
        ProtocolPackageCompilationError::Declaration(error) => {
            panic!("expected script error, got declaration error: {error}")
        }
    }
}

pub(super) fn path(value: &str) -> PackageFilePath {
    PackageFilePath::new(value).unwrap()
}
