fn reader(parts: &[&[u8]]) -> ProtocolReader {
    ProtocolReader::new(Arc::from(parts.concat()))
}

fn frame_inspector(
    package: &crate::CompiledProtocolPackage,
    direction: ProtocolDirection,
    connection_id: impl Into<String>,
    runtime_limits: ProtocolRuntimeLimits,
    framing_limits: ProtocolFramingLimits,
) -> ProtocolFrameInspector {
    ProtocolFrameInspector::new_with_cancellation(
        package,
        direction,
        connection_id,
        "listener-1",
        runtime_limits,
        framing_limits,
        ProtocolExecutionCancellation::new(),
    )
}

fn inspect_chunks(
    inspector: &mut ProtocolFrameInspector,
    chunks: impl IntoIterator<Item = Vec<u8>>,
) -> Result<Vec<Vec<u8>>, ProtocolFramingError> {
    let mut buffered = Vec::new();
    let mut frames = Vec::new();
    for chunk in chunks {
        buffered.extend(chunk);
        loop {
            match inspector.inspect(&buffered)? {
                ProtocolFrameInspection::NeedMore { .. } => break,
                ProtocolFrameInspection::Complete { bytes } => {
                    frames.push(buffered.drain(..bytes).collect());
                    if buffered.is_empty() {
                        break;
                    }
                }
                ProtocolFrameInspection::Reject { reason } => {
                    return Err(ProtocolFramingError::Rejected { reason });
                }
            }
        }
    }
    Ok(frames)
}

fn valid_fixed_script() -> &'static str {
    concat!(
        "fn frame(reader, context) { if reader.available() < 1 { framing::need_more(1) } else { framing::complete(1) } }\n",
        "fn decode(origin, context) { document::create() }\n",
        "fn encode(origin, document, context) { origin }\n",
    )
}

fn compile_package(
    upstream_script: &str,
    downstream_script: &str,
) -> crate::CompiledProtocolPackage {
    compile_package_with_files(upstream_script, downstream_script, &[])
}

fn compile_package_with_files(
    upstream_script: &str,
    downstream_script: &str,
    extra_files: &[(&str, &[u8])],
) -> crate::CompiledProtocolPackage {
    let protocol_script = match (
        upstream_script == valid_fixed_script(),
        downstream_script == valid_fixed_script(),
    ) {
        (false, true) => upstream_script,
        (true, false) => downstream_script,
        _ if upstream_script == downstream_script => upstream_script,
        _ => panic!("fixed protocol.rhai cannot vary by direction"),
    };
    let mut protocol_script = protocol_script.to_owned();
    if !protocol_script.contains("fn encode(") {
        protocol_script.push_str("fn encode(origin, document, context) { origin }\n");
    }
    let manifest = r#"api = 1

[package]
id = "framing-test"
name = "Framing Test"
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
"#;
    let mut files = BTreeMap::from([
        (path("manifest.toml"), manifest.as_bytes().to_vec()),
        (path("document.toml"), DOCUMENT_SCHEMA.as_bytes().to_vec()),
        (path("protocol.rhai"), protocol_script.into_bytes()),
        (
            path("display.rhai"),
            b"fn display(document, context) { \"<p>ok</p>\" }".to_vec(),
        ),
    ]);
    for (name, bytes) in extra_files {
        files.insert(path(name), bytes.to_vec());
    }
    let total_bytes = files.values().map(Vec::len).sum::<usize>();
    let files = ProtocolPackageFiles::new(files, u64::try_from(total_bytes).unwrap());
    ProtocolPackageCompiler::default().compile(&files).unwrap()
}

fn compile_official_iso8583_package() -> crate::CompiledProtocolPackage {
    let manifest =
        include_str!("../../../../../../templates/socket-protocol/iso8583-standard/manifest.toml");
    let schema = include_bytes!(
        "../../../../../../templates/socket-protocol/iso8583-standard/document.toml"
    );
    let protocol = include_bytes!(
        "../../../../../../templates/socket-protocol/iso8583-standard/protocol.rhai"
    );
    let display =
        include_bytes!("../../../../../../templates/socket-protocol/iso8583-standard/display.rhai");
    let library = include_bytes!(
        "../../../../../../templates/socket-protocol/iso8583-standard/libraries/iso8583.rhai"
    );
    let files = BTreeMap::from([
        (path("manifest.toml"), manifest.as_bytes().to_vec()),
        (path("document.toml"), schema.to_vec()),
        (path("protocol.rhai"), protocol.to_vec()),
        (path("display.rhai"), display.to_vec()),
        (path("libraries/iso8583.rhai"), library.to_vec()),
    ]);
    let total_bytes = files.values().map(Vec::len).sum::<usize>();
    let files = ProtocolPackageFiles::new(files, u64::try_from(total_bytes).unwrap());
    ProtocolPackageCompiler::default().compile(&files).unwrap()
}

fn path(value: &str) -> PackageFilePath {
    PackageFilePath::new(value).unwrap()
}
