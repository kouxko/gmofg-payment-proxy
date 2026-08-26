use std::{
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use intercept_proxy_domain::{Document, DocumentValue};
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolArchiveLimits, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolExecutionCancellation, ProtocolFrameInspection, ProtocolFrameInspector,
    ProtocolFramingLimits, ProtocolPackageCompiler, ProtocolRuntimeLimits,
    read_protocol_package_zip,
};
use serde_json::{Value, json};

const CONTROL: [u8; 4] = [0x01, 0x00, 0x01, 0x00];
const SEQUENCE: &[u8; 8] = b"00000020";

#[test]
fn same_frames_match_python_oracle_and_preserve_raw_json_values() {
    let package = compile_source_package();

    for (direction, fixture, message_type) in [
        (
            ProtocolDirection::Upstream,
            "request.json",
            "AccptrAuthstnReq",
        ),
        (
            ProtocolDirection::Downstream,
            "response.json",
            "AccptrAuthstnRspn",
        ),
    ] {
        let payload = fixture_bytes(fixture);
        let frame = frame(&payload, CONTROL, SEQUENCE);
        let python = python_oracle(direction, &frame);

        assert_eq!(
            inspect(&package, direction, &frame),
            ProtocolFrameInspection::Complete { bytes: frame.len() }
        );
        assert_eq!(python["frame"]["status"], "complete");
        assert_eq!(python["frame"]["consumed_bytes"], frame.len());

        let mut executor = executor(&package, direction);
        let document = executor.decode_document(&frame).unwrap();
        assert_eq!(python["decode"]["status"], "ok");
        assert_eq!(int(&document, "frame_length"), frame.len() as i64 - 4);
        assert_eq!(
            python["decode"]["frame_length"],
            int(&document, "frame_length")
        );
        assert_eq!(blob(&document, "control_header"), CONTROL);
        assert_eq!(python["decode"]["control_header_hex"], hex(&CONTROL));
        assert_eq!(string(&document, "sequence"), "00000020");
        assert_eq!(python["decode"]["sequence"], "00000020");
        assert_eq!(string(&document, "message_type"), message_type);
        assert_eq!(python["decode"]["message_type"], message_type);
        assert_eq!(python["decode"]["json_preview_type"], "string");
        assert_eq!(python["decode"]["encoding_context_type"], "blob");

        let preview: Value = serde_json::from_str(string(&document, "json_preview")).unwrap();
        assert_eq!(preview, serde_json::from_slice::<Value>(&payload).unwrap());
        assert!(!string(&document, "json_preview").contains("[redacted]"));

        let encoded = executor.encode_document(&frame, document.clone()).unwrap();
        assert_eq!(encoded, frame);
        assert_eq!(python["decode"]["encode_hex"], hex(&frame));

        let html = executor.display_document(&document).unwrap();
        assert!(html.contains(message_type));
        assert!(html.contains("synthetic-"));
        assert!(!html.contains("[redacted]"));
        assert!(!html.contains("<synthetic>"));
        if fixture == "request.json" {
            assert!(html.contains("&lt;synthetic&gt;"));
            assert!(html.contains("synthetic-pan"));
            assert!(html.contains("synthetic-track-data"));
            assert!(html.contains("synthetic-mac"));
            assert!(html.contains("synthetic-key"));
        }
    }
}

#[test]
fn fragmentation_sticky_frames_and_length_boundaries_match_python() {
    let package = compile_source_package();
    let frame = frame(&fixture_bytes("request.json"), CONTROL, SEQUENCE);

    for fragment in [&frame[..3], &frame[..frame.len() - 1]] {
        assert!(matches!(
            inspect(&package, ProtocolDirection::Upstream, fragment),
            ProtocolFrameInspection::NeedMore { .. }
        ));
        assert_eq!(
            python_oracle(ProtocolDirection::Upstream, fragment)["frame"]["status"],
            "need_more"
        );
    }

    let sticky = [frame.as_slice(), b"next"].concat();
    assert_eq!(
        inspect(&package, ProtocolDirection::Upstream, &sticky),
        ProtocolFrameInspection::Complete { bytes: frame.len() }
    );
    assert_eq!(
        python_oracle(ProtocolDirection::Upstream, &sticky)["frame"]["consumed_bytes"],
        frame.len()
    );

    let too_small = 13_u32.to_be_bytes();
    assert!(matches!(
        inspect(&package, ProtocolDirection::Upstream, &too_small),
        ProtocolFrameInspection::Reject { .. }
    ));
    assert!(
        python_oracle(ProtocolDirection::Upstream, &too_small)
            .get("process_error")
            .is_some()
    );

    let maximum = 1_048_572_u32.to_be_bytes();
    assert!(matches!(
        inspect(&package, ProtocolDirection::Upstream, &maximum),
        ProtocolFrameInspection::NeedMore { total: 1_048_576 }
    ));
    assert_eq!(
        python_oracle(ProtocolDirection::Upstream, &maximum)["frame"]["status"],
        "need_more"
    );

    let too_large = 1_048_573_u32.to_be_bytes();
    assert!(matches!(
        inspect(&package, ProtocolDirection::Upstream, &too_large),
        ProtocolFrameInspection::Reject { .. }
    ));
    assert!(
        python_oracle(ProtocolDirection::Upstream, &too_large)
            .get("process_error")
            .is_some()
    );
}

#[test]
fn invalid_sequence_json_and_top_level_shape_fail_closed_like_python() {
    let package = compile_source_package();
    let cases = [
        frame(br#"{"Message":{}}"#, CONTROL, b"ABCDEFGH"),
        frame(br#"{"Message":]"#, CONTROL, SEQUENCE),
        frame(br#"[]"#, CONTROL, SEQUENCE),
        frame(br#"{}"#, CONTROL, SEQUENCE),
    ];

    for (index, invalid) in cases.into_iter().enumerate() {
        let python = python_oracle(ProtocolDirection::Upstream, &invalid);
        assert_eq!(python["decode"]["status"], "error");
        let mut executor = executor(&package, ProtocolDirection::Upstream);
        assert!(
            executor.decode_document(&invalid).is_err(),
            "invalid case {index} unexpectedly decoded"
        );
    }
}

#[test]
fn every_field_change_removal_and_cross_direction_reuse_fail_closed() {
    let package = compile_source_package();
    let frame = frame(&fixture_bytes("request.json"), CONTROL, SEQUENCE);
    let mut upstream = executor(&package, ProtocolDirection::Upstream);
    let original = upstream.decode_document(&frame).unwrap();

    for (field, replacement) in [
        ("frame_length", DocumentValue::Int(1)),
        ("control_header", DocumentValue::Blob(vec![0, 0, 0, 0])),
        ("sequence", DocumentValue::String("99999999".to_owned())),
        ("message_type", DocumentValue::String("Changed".to_owned())),
        ("json_preview", DocumentValue::String("{}".to_owned())),
        (
            "encoding_context",
            DocumentValue::Blob(b"tampered".to_vec()),
        ),
    ] {
        let mut changed = original.clone();
        changed.set(field, replacement).unwrap();
        assert!(
            upstream.encode_document(&frame, changed).is_err(),
            "changed field {field} must fail before producing bytes"
        );

        let mut removed = original.clone();
        removed.clear_field(field).unwrap();
        assert!(
            upstream.encode_document(&frame, removed).is_err(),
            "removed field {field} must fail before producing bytes"
        );
    }

    let mut downstream = executor(&package, ProtocolDirection::Downstream);
    assert!(downstream.encode_document(&frame, original).is_err());
}

#[test]
fn known_exchange_sizes_execute_in_both_directions_without_changing_bytes() {
    let package = compile_source_package();
    for (direction, total_bytes, message_type) in [
        (ProtocolDirection::Upstream, 1_602, "AccptrAuthstnReq"),
        (ProtocolDirection::Downstream, 647, "AccptrAuthstnRspn"),
        (ProtocolDirection::Upstream, 1_602, "AccptrCmpltnAdvc"),
        (ProtocolDirection::Downstream, 914, "AccptrCmpltnAdvcRspn"),
        (ProtocolDirection::Upstream, 1_322, "AccptrAuthstnReq"),
        (ProtocolDirection::Downstream, 896, "AccptrAuthstnRspn"),
    ] {
        let frame = sized_frame(total_bytes, message_type);
        let python = python_oracle(direction, &frame);
        assert_eq!(python["decode"]["status"], "ok");

        let mut runtime = executor(&package, direction);
        let output = runtime.execute_frame(frame.clone()).unwrap();
        assert_eq!(output.written(), frame);
        assert_eq!(
            string(output.decoded_document().unwrap(), "message_type"),
            message_type
        );
    }
}

#[test]
fn deterministic_zip_imports_and_executes_through_the_host_runtime() {
    let root = package_root();
    let first = temporary_directory("first");
    let second = temporary_directory("second");
    let first_output = run_builder(&root, &first);
    let second_output = run_builder(&root, &second);
    assert!(
        first_output.status.success(),
        "{}",
        String::from_utf8_lossy(&first_output.stderr)
    );
    assert!(
        second_output.status.success(),
        "{}",
        String::from_utf8_lossy(&second_output.stderr)
    );

    let archive_name = "nuvei-tango-json-rhai-1.0.0.zip";
    let first_bytes = std::fs::read(first.join(archive_name)).unwrap();
    let second_bytes = std::fs::read(second.join(archive_name)).unwrap();
    assert_eq!(first_bytes, second_bytes);

    let files =
        read_protocol_package_zip(Cursor::new(first_bytes), &ProtocolArchiveLimits::default())
            .unwrap();
    let package = ProtocolPackageCompiler::default().compile(&files).unwrap();
    assert_eq!(package.package().id.as_str(), "nuvei-tango-json-rhai");
    assert_eq!(package.package().version.as_str(), "1.0.0");

    let frame = frame(&fixture_bytes("response.json"), CONTROL, SEQUENCE);
    let mut runtime = executor(&package, ProtocolDirection::Downstream);
    let output = runtime.execute_frame(frame.clone()).unwrap();
    assert_eq!(output.written(), frame);

    std::fs::remove_dir_all(first).unwrap();
    std::fs::remove_dir_all(second).unwrap();
}

fn compile_source_package() -> intercept_proxy_protocol_scripting::CompiledProtocolPackage {
    let root = package_root();
    let output = temporary_directory("source");
    let build = run_builder(&root, &output);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let archive = std::fs::read(output.join("nuvei-tango-json-rhai-1.0.0.zip")).unwrap();
    let files =
        read_protocol_package_zip(Cursor::new(archive), &ProtocolArchiveLimits::default()).unwrap();
    let package = ProtocolPackageCompiler::default().compile(&files).unwrap();
    std::fs::remove_dir_all(output).unwrap();
    package
}

fn inspect(
    package: &intercept_proxy_protocol_scripting::CompiledProtocolPackage,
    direction: ProtocolDirection,
    bytes: &[u8],
) -> ProtocolFrameInspection {
    let mut inspector = ProtocolFrameInspector::new_with_cancellation(
        package,
        direction,
        "nuvei-test-connection",
        "nuvei-test-listener",
        ProtocolRuntimeLimits::default(),
        ProtocolFramingLimits::new(1_048_576, 2_097_152).unwrap(),
        ProtocolExecutionCancellation::new(),
    );
    inspector.inspect_owned(Arc::from(bytes)).unwrap()
}

fn executor(
    package: &intercept_proxy_protocol_scripting::CompiledProtocolPackage,
    direction: ProtocolDirection,
) -> ProtocolDirectionExecutor {
    ProtocolDirectionExecutor::new(
        package,
        DirectionExecutionPlan::new(direction),
        "nuvei-test-connection",
        "nuvei-test-listener",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap()
}

fn python_oracle(direction: ProtocolDirection, frame: &[u8]) -> Value {
    let output = Command::new("python3")
        .arg(package_root().join("tests/python_oracle.py"))
        .arg(direction.as_str())
        .arg(hex(frame))
        .output()
        .unwrap();
    if output.status.success() {
        serde_json::from_slice(&output.stdout).unwrap()
    } else {
        json!({
            "process_error": String::from_utf8_lossy(&output.stderr),
            "status": output.status.code(),
        })
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(package_root().join("tests/fixtures").join(name)).unwrap()
}

fn frame(payload: &[u8], control: [u8; 4], sequence: &[u8; 8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(12 + payload.len());
    body.extend(control);
    body.extend(sequence);
    body.extend(payload);
    let mut result = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
    result.extend(body);
    result
}

fn sized_frame(total_bytes: usize, message_type: &str) -> Vec<u8> {
    let prefix = format!(r#"{{"{message_type}":{{"Padding":""#);
    let suffix = r#""}}"#;
    let payload_bytes = total_bytes - 16;
    let padding_bytes = payload_bytes - prefix.len() - suffix.len();
    let payload = format!("{prefix}{}{suffix}", "x".repeat(padding_bytes));
    let result = frame(payload.as_bytes(), CONTROL, SEQUENCE);
    assert_eq!(result.len(), total_bytes);
    result
}

fn int(document: &Document, name: &str) -> i64 {
    let DocumentValue::Int(value) = document.get(name).unwrap() else {
        panic!("field {name} is not int");
    };
    *value
}

fn string<'a>(document: &'a Document, name: &str) -> &'a str {
    let DocumentValue::String(value) = document.get(name).unwrap() else {
        panic!("field {name} is not string");
    };
    value
}

fn blob<'a>(document: &'a Document, name: &str) -> &'a [u8] {
    let DocumentValue::Blob(value) = document.get(name).unwrap() else {
        panic!("field {name} is not blob");
    };
    value
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn package_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn temporary_directory(label: &str) -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "nuvei-tango-rhai-{label}-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    if path.exists() {
        std::fs::remove_dir_all(&path).unwrap();
    }
    path
}

fn run_builder(root: &Path, output: &Path) -> std::process::Output {
    Command::new("python3")
        .arg(root.join("build_package.py"))
        .arg("--output-directory")
        .arg(output)
        .output()
        .unwrap()
}
