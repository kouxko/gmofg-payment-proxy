fn reader(parts: &[&[u8]]) -> ProtocolReader {
    let mut available = 0;
    let segments = parts
        .iter()
        .map(|part| {
            available += part.len();
            let bytes: Arc<[u8]> = part.to_vec().into();
            ReaderSegment::new(bytes, 0..part.len())
        })
        .collect();
    ProtocolReader::from_segments(segments, available)
}

fn closure_framer<F>(max_frame: u64, max_fifo: u64, decider: F) -> SingleDirectionFramer<F>
where
    F: FnMut(ProtocolReader) -> Result<FramingDecision, ProtocolFramingError>,
{
    SingleDirectionFramer::new(
        decider,
        ProtocolFramingLimits::new(max_frame, max_fifo).unwrap(),
    )
}

fn assert_state_error(decision: FramingDecision, expected: &ProtocolFramingError) {
    let mut decision = Some(decision);
    let mut framer = closure_framer(8, 8, move |_| Ok(decision.take().unwrap()));
    assert_eq!(&framer.push(vec![1]).unwrap_err(), expected);
    assert_eq!(framer.buffered_bytes(), 0);
}

fn valid_fixed_script() -> &'static str {
    "fn frame(reader, context) { if reader.available() < 1 { framing::need_more(1) } else { framing::complete(1) } }\nfn decode(origin, context) { () }\n"
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
    let manifest = r#"api = 1

[package]
id = "framing-test"
name = "Framing Test"
version = "1.0.0"

[document]
schema = "document.toml"

[hooks.upstream.receive]
script = "upstream.rhai"
frame = "frame"
decode = "decode"

[hooks.downstream.receive]
script = "downstream.rhai"
frame = "frame"
decode = "decode"
"#;
    let mut files = BTreeMap::from([
        (path("manifest.toml"), manifest.as_bytes().to_vec()),
        (path("document.toml"), DOCUMENT_SCHEMA.as_bytes().to_vec()),
        (path("upstream.rhai"), upstream_script.as_bytes().to_vec()),
        (
            path("downstream.rhai"),
            downstream_script.as_bytes().to_vec(),
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
    let schema =
        include_bytes!("../../../../../../templates/socket-protocol/iso8583-standard/document.toml");
    let protocol =
        include_bytes!("../../../../../../templates/socket-protocol/iso8583-standard/protocol.rhai");
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
