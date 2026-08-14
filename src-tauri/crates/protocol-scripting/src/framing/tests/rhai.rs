#[test]
fn rhai_frame_entry_receives_reader_and_read_only_direction_context() {
    let upstream_script = r#"
fn frame(reader, context) {
    if context.direction() != "upstream" || context.stage() != "receive" {
        return framing::reject("wrong context");
    }
    if reader.available() < 2 { return framing::need_more(2); }
    let total = 2 + reader.peek_u16_be(0);
    if reader.available() < total { framing::need_more(total) }
    else { framing::complete(total) }
}
fn decode(origin, context) { () }
"#;
    let package = compile_package(upstream_script, valid_fixed_script());
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-1",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer =
        SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(64, 64).unwrap());

    assert!(framer.push(vec![0]).unwrap().is_empty());
    assert!(framer.push(vec![2, b'A']).unwrap().is_empty());
    assert_eq!(
        framer.push(vec![b'B']).unwrap(),
        vec![vec![0, 2, b'A', b'B']]
    );
}
#[test]
fn rhai_frame_wrong_return_type_is_fail_closed_and_clears_fifo() {
    let package = compile_package(
        "fn frame(reader, context) { () }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-2",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer = SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());

    assert_eq!(
        framer.push(vec![1]).unwrap_err(),
        ProtocolFramingError::FrameEntryFailed {
            package: package.package().clone(),
        }
    );
    assert_eq!(framer.buffered_bytes(), 0);
}

#[test]
fn rhai_reader_find_and_static_imports_work_without_a_runtime_file_resolver() {
    let upstream = r#"
import "libraries/framing" as framing_helpers;

fn frame(reader, context) {
    framing_helpers::delimiter_frame(reader)
}
fn decode(origin, context) { () }
"#;
    let library = r#"
fn delimiter_frame(reader) {
    let delimiter = "\r\n".to_blob();
    let offset = reader.find(delimiter, 0);
    if offset < 0 { framing::need_more(reader.available() + 1) }
    else { framing::complete(offset + delimiter.len()) }
}
"#;
    let package = compile_package_with_files(
        upstream,
        valid_fixed_script(),
        &[("libraries/framing.rhai", library.as_bytes())],
    );
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-import",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer =
        SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(64, 64).unwrap());

    assert!(framer.push(b"ONE\r".to_vec()).unwrap().is_empty());
    assert_eq!(
        framer.push(b"\nTWO\r\n".to_vec()).unwrap(),
        vec![b"ONE\r\n".to_vec(), b"TWO\r\n".to_vec()]
    );
}

#[test]
fn rhai_host_rejects_negative_lengths_empty_reasons_and_reader_misuse() {
    let invalid_bodies = [
        "framing::need_more(-1)",
        "framing::complete(-1)",
        "framing::reject(\"\")",
        "reader.peek(-1, 1); framing::complete(1)",
        "reader.peek(1, 99); framing::complete(1)",
        "reader.find(blob(), 0); framing::complete(1)",
    ];

    for (index, body) in invalid_bodies.into_iter().enumerate() {
        let script = format!(
            "fn frame(reader, context) {{ {body} }}\nfn decode(origin, context) {{ () }}\n"
        );
        let package = compile_package(&script, valid_fixed_script());
        let decider = RhaiFrameDecider::for_package(
            &package,
            ProtocolDirection::Upstream,
            format!("connection-invalid-{index}"),
            "listener-1",
            ProtocolRuntimeLimits::default(),
        );
        let mut framer =
            SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());
        assert_eq!(
            framer.push(vec![1]).unwrap_err().code(),
            ProtocolFramingErrorCode::FrameEntryFailed,
            "invalid Rhai body unexpectedly succeeded: {body}"
        );
        assert_eq!(framer.buffered_bytes(), 0);
    }
}

#[test]
fn rhai_reader_registers_every_integer_and_blob_method_for_downstream() {
    let downstream = r#"
fn frame(reader, context) {
    if context.direction() != "downstream" { return framing::reject("wrong direction"); }
    if reader.available() < 8 { return framing::need_more(8); }
    if reader.peek(1, 2).len() != 2 { return framing::reject("peek"); }
    if reader.peek_u8(0) != 1 { return framing::reject("u8"); }
    if reader.peek_u16_be(0) != 0x0102 { return framing::reject("u16be"); }
    if reader.peek_u16_le(0) != 0x0201 { return framing::reject("u16le"); }
    if reader.peek_u32_be(0) != 0x01020304 { return framing::reject("u32be"); }
    if reader.peek_u32_le(0) != 0x04030201 { return framing::reject("u32le"); }
    if reader.find(reader.peek(2, 2), 0) != 2 { return framing::reject("find"); }
    framing::complete(8)
}
fn decode(origin, context) { () }
"#;
    let package = compile_package(valid_fixed_script(), downstream);
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Downstream,
        "connection-reader",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer =
        SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(16, 16).unwrap());

    assert_eq!(
        framer.push(vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap(),
        vec![vec![1, 2, 3, 4, 5, 6, 7, 8]]
    );
}

#[test]
fn rhai_reject_constructor_accepts_a_bounded_reason() {
    let package = compile_package(
        "fn frame(reader, context) { framing::reject(\"not mine\") }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-reject",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer = SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());
    assert_eq!(
        framer.push(vec![1]).unwrap_err(),
        ProtocolFramingError::Rejected {
            reason: "not mine".to_owned(),
        }
    );
}

#[test]
fn official_iso8583_template_frame_executes_with_globals_and_embedded_imports() {
    let package = compile_official_iso8583_package();
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-iso8583",
        "listener-iso8583",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer = SingleDirectionFramer::new(
        decider,
        ProtocolFramingLimits::new(65_535, 131_070).unwrap(),
    );

    assert!(framer.push(vec![0]).unwrap().is_empty());
    assert!(framer.push(vec![4, b'0', b'2']).unwrap().is_empty());
    assert_eq!(
        framer.push(vec![b'0', b'0']).unwrap(),
        vec![vec![0, 4, b'0', b'2', b'0', b'0']]
    );
}

#[test]
fn rhai_operation_limit_stops_a_non_terminating_frame_entry() {
    let package = compile_package(
        "fn frame(reader, context) { while true {} }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let runtime_limits = ProtocolRuntimeLimits::new(100, 32, 1024, 1024, 250).unwrap();
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-loop",
        "listener-1",
        runtime_limits,
    );
    let mut framer = SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());

    assert_eq!(
        framer.push(vec![1]).unwrap_err().code(),
        ProtocolFramingErrorCode::FrameEntryFailed
    );
    assert_eq!(framer.buffered_bytes(), 0);
}
