use std::{collections::BTreeMap, fmt::Write};

use intercept_proxy_domain::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema, DocumentSchemaId,
    DocumentValue,
};
use serde::Deserialize;

use crate::{
    DirectionExecutionPlan, PackageFilePath, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolDisplayResult, ProtocolExecutionCancellation, ProtocolFrameInspection,
    ProtocolFrameInspector, ProtocolFramingLimits, ProtocolPackageCompilationError,
    ProtocolPackageCompiler, ProtocolPackageFiles, ProtocolRuntimeLimits,
    test_support::CompiledProtocolPackageTestBuilder,
    tests::fixtures::{
        TEMPLATE_DISPLAY, TEMPLATE_LIBRARY, TEMPLATE_MANIFEST, TEMPLATE_PROTOCOL,
        TEMPLATE_REQUEST_SAMPLE, TEMPLATE_RESPONSE_SAMPLE, TEMPLATE_SCHEMA,
    },
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Iso8583Sample {
    description: String,
    tcp_chunks_hex: Vec<String>,
    complete_frame_hex: String,
    expected_document: ExpectedIso8583Document,
    expected_encode: String,
    expected_display: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedIso8583Document {
    message_type: String,
    processing_code: String,
    amount: i64,
    transmission_time: String,
    stan: String,
    terminal_id: String,
    currency: String,
}

#[test]
fn official_iso8583_request_sample_runs_frame_decode_encode_and_display() {
    let package = compile_official_template();
    let sample = sample(TEMPLATE_REQUEST_SAMPLE);
    assert!(sample.description.contains("0200 financial request"));
    assert_eq!(sample.expected_encode, "same_as_complete_frame");
    assert_eq!(sample.expected_display, "html");

    let frames = frame_chunks(
        &package,
        ProtocolDirection::Upstream,
        &sample.tcp_chunks_hex,
    );
    assert_eq!(frames, vec![hex(&sample.complete_frame_hex)]);

    let mut executor = executor(&package, ProtocolDirection::Upstream);
    let output = executor.execute_frame(frames[0].clone()).unwrap();
    assert_document(
        output.decoded_document().unwrap(),
        &sample.expected_document,
    );
    assert_eq!(output.written(), hex(&sample.complete_frame_hex));
    let ProtocolDisplayResult::UntrustedHtml(html) = executor.render_display(&output) else {
        panic!("official template declares Display and must return untrusted HTML");
    };
    for expected in ["ISO 8583:1987 Message", "0200", "1000", "TERM0001", "392"] {
        assert!(html.contains(expected), "HTML does not contain {expected}");
    }
}

#[test]
fn official_iso8583_secondary_bitmap_field_round_trips() {
    let package = compile_official_template();
    let mut payload = b"0800".to_vec();
    payload.extend_from_slice(&[0xe0, 0, 0, 0, 0, 0, 0x02, 0]); // secondary + DE2/3/55
    payload.extend_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0]); // DE70
    payload.extend_from_slice(b"164761739001010010000000004");
    payload.extend_from_slice(&[0x9f, 0x02, 0x06, 0x00]);
    payload.extend_from_slice(b"301");
    let mut frame = u16::try_from(payload.len()).unwrap().to_be_bytes().to_vec();
    frame.extend_from_slice(&payload);

    let mut executor = executor(&package, ProtocolDirection::Upstream);
    let output = executor.execute_frame(frame.clone()).unwrap();
    let document = output.decoded_document().unwrap();
    assert_eq!(
        document.get("primary_account_number").unwrap(),
        &DocumentValue::String("4761739001010010".to_owned())
    );
    assert_eq!(
        document.get("processing_code").unwrap(),
        &DocumentValue::String("000000".to_owned())
    );
    assert_eq!(
        document.get("network_management_code").unwrap(),
        &DocumentValue::String("301".to_owned())
    );
    assert_eq!(
        document.get("icc_data").unwrap(),
        &DocumentValue::Blob(vec![0x9f, 0x02, 0x06, 0x00])
    );
    assert_eq!(output.written(), frame);
}

#[test]
fn official_iso8583_rule_change_rebuilds_length_bitmap_and_amount() {
    let package = compile_official_template();
    let sample = sample(TEMPLATE_REQUEST_SAMPLE);
    let mut origin = hex(&sample.complete_frame_hex);
    // 删除固定 12 字节 DE4，并清除主位图中的 DE4 位，先构造一个本来没有 amount 的合法 Frame。
    // 这样规则新增 amount 后，测试能真实证明 Encode 重建了长度和位图，而不只是覆盖原字段。
    origin[6] &= !0x10;
    origin.drain(20..32);
    let payload_length = u16::try_from(origin.len() - 2).unwrap().to_be_bytes();
    origin[0..2].copy_from_slice(&payload_length);
    assert_eq!(usize::from(u16::from_be_bytes([origin[0], origin[1]])), 45);
    assert_eq!(origin[6] & 0x10, 0);
    assert_eq!(origin.len(), 47);
    let origin_length = origin.len();
    let mut transform_executor = executor(&package, ProtocolDirection::Upstream);
    let output = transform_executor
        .execute_frame_with_rules(origin, |document| {
            document.set("amount", DocumentValue::Int(2500)).unwrap();
            Ok(())
        })
        .unwrap();

    let written = output.written();
    assert_eq!(written.len(), origin_length + 12);
    assert_eq!(
        usize::from(u16::from_be_bytes([written[0], written[1]])),
        written.len() - 2
    );
    // Frame 头 2 字节、MTI 4 字节；主位图第一个字节的 0x10 是 DE4 amount。
    assert_ne!(written[6] & 0x10, 0);
    // MTI(4) + bitmap(8) + DE3(6) 后紧跟 12 位 DE4。
    assert_eq!(&written[20..32], b"000000002500");

    let mut verifier = executor(&package, ProtocolDirection::Upstream);
    let decoded = verifier.execute_frame(written.to_vec()).unwrap();
    assert_eq!(
        decoded.decoded_document().unwrap().get("amount").unwrap(),
        &DocumentValue::Int(2500)
    );
}

#[test]
fn official_iso8583_downstream_sticky_responses_stay_independent() {
    let package = compile_official_template();
    let sample = sample(TEMPLATE_RESPONSE_SAMPLE);
    assert!(sample.description.contains("0210 financial response"));
    assert_eq!(sample.expected_encode, "same_as_complete_frame");
    assert_eq!(sample.expected_display, "html");
    let frame = hex(&sample.complete_frame_hex);
    let sample_chunks = sample
        .tcp_chunks_hex
        .iter()
        .flat_map(|chunk| hex(chunk))
        .collect::<Vec<_>>();
    assert_eq!(sample_chunks, frame);
    let sticky = [frame.as_slice(), frame.as_slice()].concat();
    let sticky_hex = encode_hex(&sticky);
    let frames = frame_chunks(&package, ProtocolDirection::Downstream, &[sticky_hex]);
    assert_eq!(frames, vec![frame.clone(), frame.clone()]);

    let mut downstream = executor(&package, ProtocolDirection::Downstream);
    let first_output = downstream
        .execute_frame_with_rules(frames[0].clone(), |document| {
            document.set("amount", DocumentValue::Int(2000)).unwrap();
            Ok(())
        })
        .unwrap();
    let second_output = downstream.execute_frame(frames[1].clone()).unwrap();
    let mut first_expected = sample.expected_document.clone();
    first_expected.amount = 2000;
    assert_document(first_output.decoded_document().unwrap(), &first_expected);
    assert_document(
        second_output.decoded_document().unwrap(),
        &sample.expected_document,
    );
    assert_ne!(first_output.written(), frame);
    assert_eq!(&first_output.written()[20..32], b"000000002000");
    assert_eq!(second_output.written(), frame);
    let ProtocolDisplayResult::UntrustedHtml(first_html) = downstream.render_display(&first_output)
    else {
        panic!("downstream response must use the declared Display entry");
    };
    let ProtocolDisplayResult::UntrustedHtml(second_html) =
        downstream.render_display(&second_output)
    else {
        panic!("each sticky response must retain its own Display observation");
    };
    assert!(first_html.contains("2000"));
    assert!(first_html.contains("0210"));
    assert!(second_html.contains("1000"));
    assert_ne!(first_html, second_html);
}

#[test]
fn template_syntax_schema_and_manifest_entry_damage_fail_during_import() {
    let syntax = String::from_utf8(TEMPLATE_PROTOCOL.to_vec())
        .unwrap()
        .replace(
            "fn decode(origin, context) {",
            "fn decode(origin, context) [",
        );
    assert!(matches!(
        compile_template_with(TEMPLATE_MANIFEST, TEMPLATE_SCHEMA, syntax.as_bytes()),
        Err(ProtocolPackageCompilationError::Script(_))
    ));

    let schema = TEMPLATE_SCHEMA.replace("type = \"int\"", "type = \"decimal\"");
    assert!(matches!(
        compile_template_with(TEMPLATE_MANIFEST, &schema, TEMPLATE_PROTOCOL),
        Err(ProtocolPackageCompilationError::Declaration(_))
    ));

    let manifest =
        TEMPLATE_MANIFEST.replacen("encode = \"encode\"", "encode = \"missing_encode\"", 1);
    assert!(matches!(
        compile_template_with(&manifest, TEMPLATE_SCHEMA, TEMPLATE_PROTOCOL),
        Err(ProtocolPackageCompilationError::Script(_))
    ));
}

#[test]
fn non_iso_length_prefixed_tlv_uses_the_same_host_contract() {
    let schema = DocumentSchema::new(
        DocumentSchemaId::new("length-prefixed-tlv").unwrap(),
        1,
        "Length-prefixed TLV",
        vec![
            DocumentField::new(
                DocumentFieldName::new("tag").unwrap(),
                DocumentFieldType::Int,
                "Tag",
            )
            .unwrap(),
            DocumentField::new(
                DocumentFieldName::new("value").unwrap(),
                DocumentFieldType::Blob,
                "Value",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_schema(schema)
        .with_script(TLV_SCRIPT)
        .with_upstream_encode()
        .build();
    let chunks = ["000501", "0341", "4243"];
    let frames = frame_chunks(
        &package,
        ProtocolDirection::Upstream,
        &chunks.map(str::to_owned),
    );
    assert_eq!(frames, vec![vec![0, 5, 1, 3, b'A', b'B', b'C']]);

    let mut executor = executor(&package, ProtocolDirection::Upstream);
    let output = executor.execute_frame(frames[0].clone()).unwrap();
    let document = output.decoded_document().unwrap();
    assert_eq!(document.get("tag").unwrap(), &DocumentValue::Int(1));
    assert_eq!(
        document.get("value").unwrap(),
        &DocumentValue::Blob(b"ABC".to_vec())
    );
    assert_eq!(output.written(), frames[0]);
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::UntrustedHtml("<p>ok</p>".to_owned())
    );
}

const TLV_SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 2 { return framing::need_more(2); }
    let total = 2 + reader.peek_u16_be(0);
    if reader.available() < total { framing::need_more(total) } else { framing::complete(total) }
}

fn decode(origin, context) {
    if origin.len() < 4 { throw "TLV frame is shorter than header"; }
    let payload_length = origin[0].to_int() * 256 + origin[1].to_int();
    let value_length = origin[3].to_int();
    if payload_length != origin.len() - 2 || value_length != payload_length - 2 {
        throw "TLV length mismatch";
    }
    let document = document::create();
    document.set("tag", origin[2].to_int());
    document.set("value", origin.extract(4, value_length));
    document
}

fn encode(origin, document, context) {
    let value = document.get("value");
    let payload_length = 2 + value.len();
    if payload_length > 65535 || value.len() > 255 { throw "TLV value is too large"; }
    let result = blob(4, 0);
    result[0] = (payload_length >> 8) & 0xff;
    result[1] = payload_length & 0xff;
    result[2] = document.get("tag");
    result[3] = value.len();
    result += value;
    result
}
"#;

fn compile_official_template() -> crate::CompiledProtocolPackage {
    compile_template_with(TEMPLATE_MANIFEST, TEMPLATE_SCHEMA, TEMPLATE_PROTOCOL).unwrap()
}

fn compile_template_with(
    manifest: &str,
    schema: &str,
    protocol: &[u8],
) -> Result<crate::CompiledProtocolPackage, ProtocolPackageCompilationError> {
    let files = BTreeMap::from([
        (path("manifest.toml"), manifest.as_bytes().to_vec()),
        (path("document.toml"), schema.as_bytes().to_vec()),
        (path("protocol.rhai"), protocol.to_vec()),
        (path("display.rhai"), TEMPLATE_DISPLAY.to_vec()),
        (path("libraries/iso8583.rhai"), TEMPLATE_LIBRARY.to_vec()),
    ]);
    let total_bytes = files.values().map(Vec::len).sum::<usize>();
    ProtocolPackageCompiler::default().compile(&ProtocolPackageFiles::new(
        files,
        u64::try_from(total_bytes).unwrap(),
    ))
}

fn frame_chunks(
    package: &crate::CompiledProtocolPackage,
    direction: ProtocolDirection,
    chunks: &[String],
) -> Vec<Vec<u8>> {
    let mut inspector = ProtocolFrameInspector::new_with_cancellation(
        package,
        direction,
        "conformance-connection",
        "conformance-listener",
        ProtocolRuntimeLimits::default(),
        ProtocolFramingLimits::new(65_535, 131_070).unwrap(),
        ProtocolExecutionCancellation::new(),
    );
    let mut buffered = Vec::new();
    let mut completed = Vec::new();
    for chunk in chunks {
        buffered.extend(hex(chunk));
        loop {
            match inspector.inspect(&buffered).unwrap() {
                ProtocolFrameInspection::NeedMore { .. } => break,
                ProtocolFrameInspection::Complete { bytes } => {
                    completed.push(buffered.drain(..bytes).collect());
                    if buffered.is_empty() {
                        break;
                    }
                }
                ProtocolFrameInspection::Reject { reason } => {
                    panic!("official package rejected its fixture: {reason}")
                }
            }
        }
    }
    completed
}

fn executor(
    package: &crate::CompiledProtocolPackage,
    direction: ProtocolDirection,
) -> ProtocolDirectionExecutor {
    let plan = DirectionExecutionPlan::new(direction);
    ProtocolDirectionExecutor::new(
        package,
        plan,
        "conformance-connection",
        "conformance-listener",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap()
}

fn sample(json: &str) -> Iso8583Sample {
    serde_json::from_str(json).unwrap()
}

fn assert_document(
    document: &intercept_proxy_domain::Document,
    expected: &ExpectedIso8583Document,
) {
    for (name, value) in [
        (
            "message_type",
            DocumentValue::String(expected.message_type.clone()),
        ),
        (
            "processing_code",
            DocumentValue::String(expected.processing_code.clone()),
        ),
        ("amount", DocumentValue::Int(expected.amount)),
        (
            "transmission_time",
            DocumentValue::String(expected.transmission_time.clone()),
        ),
        ("stan", DocumentValue::String(expected.stan.clone())),
        (
            "terminal_id",
            DocumentValue::String(expected.terminal_id.clone()),
        ),
        ("currency", DocumentValue::String(expected.currency.clone())),
    ] {
        assert_eq!(document.get(name).unwrap(), &value, "field {name}");
    }
}

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "hex must contain whole bytes");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(pair, 16).unwrap()
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            write!(output, "{byte:02x}").unwrap();
            output
        },
    )
}

fn path(value: &str) -> PackageFilePath {
    PackageFilePath::new(value).unwrap()
}
