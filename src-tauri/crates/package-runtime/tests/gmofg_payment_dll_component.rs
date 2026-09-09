use std::path::{Path, PathBuf};

use intercept_proxy_domain::{Document, ErrorCode, ProtocolDirection};
use intercept_proxy_package_contract::PackageKind;
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};
use serde_json::Value;

const SYNTHETIC_RECEIPT_RESPONSES: [&str; 4] = [
    r#"{"ResponseID":"A","TransactionType":"U010","ResponseDateTime":"20260907155726","ErrorCode":"   ","ErrorMessage":"電子レシート送信完了"}"#,
    r#"{"ResponseID":"A","TransactionType":"U010","ResponseDateTime":"20260907155727","ErrorCode":"E01","ErrorMessage":"電子レシート送信に失敗しました"}"#,
    r#"{"ResponseID":"A","TransactionType":"U011","ResponseDateTime":"20260907155728","ErrorCode":"   ","ErrorMessage":"電子レシート再送信完了"}"#,
    r#"{"ResponseID":"A","TransactionType":"U011","ResponseDateTime":"20260907155729","ErrorCode":"E02","ErrorMessage":"対象伝票が見つかりません"}"#,
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[tokio::test]
async fn gmofg_payment_dll_component_runs_downstream_decode_encode_and_json_tree_display() {
    let package_root = repository_root().join("examples/protocol-packages/gmofg_payment_dll");
    let mut runtime = load_runtime(&package_root).await;

    assert_d48_round_trip(&mut runtime, &package_root).await;
    assert_credit_round_trip_display_and_modification(&mut runtime, &package_root).await;
    assert_synthetic_receipt_round_trip_display_and_modification(&mut runtime).await;
    assert_invalid_receipt_inputs_are_rejected(&mut runtime).await;
}

async fn load_runtime(package_root: &Path) -> WasmPackageRuntime {
    let bytes = std::fs::read(package_root.join("dist/gmofg-payment-dll-1.0.1.wasm"))
        .expect("read built GMO-FG Payment DLL Component");
    let package = read_package_component(&bytes).expect("validate embedded package manifest");
    let identity = package.manifest().package().identity();
    assert_eq!(identity.id.as_str(), "gmofg-payment-dll");
    assert_eq!(identity.version.as_str(), "1.0.1");
    assert_eq!(package.manifest().kind(), PackageKind::Http);
    WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate GMO-FG Payment DLL HTTP Component")
}

async fn assert_synthetic_receipt_round_trip_display_and_modification(
    runtime: &mut WasmPackageRuntime,
) {
    for input in SYNTHETIC_RECEIPT_RESPONSES {
        let document = runtime
            .decode_http(ProtocolDirection::Downstream, input)
            .await
            .expect("decode synthetic receipt response through Host");
        let decoded: Value = serde_json::from_str(
            &document
                .to_json()
                .expect("serialize synthetic receipt Document"),
        )
        .expect("parse synthetic receipt Document JSON");
        let expected: Value = serde_json::from_str(input).expect("parse synthetic receipt input");
        assert_eq!(decoded, expected);
        assert!(decoded.get("ResponseTimeStamp").is_none());

        let display = runtime
            .display(ProtocolDirection::Downstream, &document)
            .await
            .expect("display synthetic receipt Document through Host");
        assert!(display.contains("ErrorMessage"));
        assert!(display.contains(decoded["ErrorMessage"].as_str().unwrap()));
        let encoded = runtime
            .encode_http(ProtocolDirection::Downstream, input, &document)
            .await
            .expect("encode unchanged synthetic receipt through Host");
        assert_eq!(encoded, input);
        println!(
            "RECEIPT_EVIDENCE {}",
            serde_json::json!({
                "kind": "synthetic_unchanged", "input": input,
                "document": decoded, "encoded": encoded, "display": display,
            })
        );

        assert_modified_receipt_round_trip_and_display(runtime, input, decoded).await;
    }
}

async fn assert_modified_receipt_round_trip_and_display(
    runtime: &mut WasmPackageRuntime,
    original_input: &str,
    mut changed: Value,
) {
    changed["ErrorCode"] = Value::String("D48".to_owned());
    changed["ErrorMessage"] =
        Value::String("送信失敗 <script>alert('x')</script> & \"確認\"".to_owned());
    let changed_document =
        Document::parse_json(&changed.to_string()).expect("parse modified receipt Document");
    let encoded = runtime
        .encode_http(
            ProtocolDirection::Downstream,
            original_input,
            &changed_document,
        )
        .await
        .expect("encode modified receipt through Host");
    let decoded_again = runtime
        .decode_http(ProtocolDirection::Downstream, &encoded)
        .await
        .expect("decode modified receipt through Host");
    let decoded_again: Value = serde_json::from_str(
        &decoded_again
            .to_json()
            .expect("serialize modified receipt Document"),
    )
    .expect("parse modified receipt Document JSON");
    assert_eq!(decoded_again, changed);

    let display = runtime
        .display(ProtocolDirection::Downstream, &changed_document)
        .await
        .expect("display modified receipt through Host");
    assert!(display.contains("送信失敗"));
    assert!(display.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
    assert!(display.contains("&amp; &quot;確認&quot;"));
    assert!(!display.contains("<script>"));
    println!(
        "RECEIPT_EVIDENCE {}",
        serde_json::json!({
            "kind": "synthetic_modified", "input": original_input,
            "document": changed, "encoded": encoded, "decoded_again": decoded_again,
            "display": display,
        })
    );
}

async fn assert_invalid_receipt_inputs_are_rejected(runtime: &mut WasmPackageRuntime) {
    let original: Value =
        serde_json::from_str(SYNTHETIC_RECEIPT_RESPONSES[0]).expect("parse base receipt");
    let mut invalid = vec![Value::Null, Value::Array(Vec::new()), serde_json::json!({})];

    for field in [
        "ResponseID",
        "TransactionType",
        "ResponseDateTime",
        "ErrorCode",
        "ErrorMessage",
    ] {
        let mut missing = original.clone();
        missing
            .as_object_mut()
            .expect("receipt is an object")
            .remove(field);
        invalid.push(missing);

        for wrong_type in [Value::Null, serde_json::json!(123), serde_json::json!({})] {
            let mut changed = original.clone();
            changed[field] = wrong_type;
            invalid.push(changed);
        }
    }

    for (field, value) in [
        ("TransactionType", serde_json::json!("U012")),
        ("Unexpected", serde_json::json!("value")),
        ("KCCI_01", serde_json::json!({"Length": "1", "Data": "X"})),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        invalid.push(changed);
    }

    for input in invalid {
        let input = input.to_string();
        let error = runtime
            .decode_http(ProtocolDirection::Downstream, &input)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BodyDecodeFailed, "input={input}");
    }
}

async fn assert_d48_round_trip(runtime: &mut WasmPackageRuntime, package_root: &Path) {
    let d48 = std::fs::read_to_string(package_root.join("tests/fixtures/d48.json"))
        .expect("read D48 fixture");
    let document = runtime
        .decode_http(ProtocolDirection::Downstream, &d48)
        .await
        .expect("decode D48 downstream response through Host");
    let value: Value = serde_json::from_str(&document.to_json().expect("serialize D48 Document"))
        .expect("parse D48 Document JSON");
    assert_eq!(value["ErrorCode"], "D48");
    assert_eq!(
        runtime
            .encode_http(ProtocolDirection::Downstream, &d48, &document)
            .await
            .expect("encode unchanged D48 response through Host"),
        d48
    );
}

async fn assert_credit_round_trip_display_and_modification(
    runtime: &mut WasmPackageRuntime,
    package_root: &Path,
) {
    let credit = std::fs::read_to_string(package_root.join("tests/fixtures/credit-success.json"))
        .expect("read Credit fixture");
    let document = runtime
        .decode_http(ProtocolDirection::Downstream, &credit)
        .await
        .expect("decode complete Credit response through Host");
    let display = runtime
        .display(ProtocolDirection::Downstream, &document)
        .await
        .expect("display complete Credit Document through Host");
    assert_complete_json_tree_display_fits_proxy_limits(&display);
    assert_eq!(
        runtime
            .encode_http(ProtocolDirection::Downstream, &credit, &document)
            .await
            .expect("encode unchanged Credit response through Host"),
        credit
    );
    assert_credit_card_range_modification(runtime, &credit, &document).await;
}

fn assert_complete_json_tree_display_fits_proxy_limits(display: &str) {
    assert!(display.contains("<table"));
    assert!(display.contains("<details open>"));
    assert!(display.contains("<summary><strong>基本信息</strong>"));
    assert!(display.contains("<summary><strong>KCCI_01</strong><span>Array · "));
    assert!(display.contains("<summary><strong>card_ranges</strong><span>Array · "));
    assert!(display.contains("<summary><strong>[0]</strong><span>Object · "));
    assert!(display.contains("<thead>"));
    assert!(display.contains("<tbody>"));
    assert!(display.contains("reserved_155_156"));
    assert!(!display.contains(">$</caption>"));
    assert!(!display.contains(">$."));
    assert!(!display.contains("<pre"));
    assert!(
        display.len() <= 1024 * 1024,
        "complete Credit display must remain below the UI source limit, actual={} bytes",
        display.len()
    );
    let element_nodes = [
        "<section", "<h3", "<details", "<summary", "<strong", "<span", "<table", "<thead",
        "<tbody", "<tr", "<th", "<td", "<p",
    ]
    .iter()
    .map(|tag| display.matches(tag).count())
    .sum::<usize>();
    let text_nodes = ["<h3", "<strong", "<span", "<th", "<td", "<p"]
        .iter()
        .map(|tag| display.matches(tag).count())
        .sum::<usize>();
    let display_nodes = element_nodes + text_nodes;
    assert!(
        display_nodes <= 8_192,
        "complete Credit display must remain below the UI node limit, actual={display_nodes}"
    );
}

async fn assert_credit_card_range_modification(
    runtime: &mut WasmPackageRuntime,
    credit: &str,
    document: &Document,
) {
    let mut changed: Value =
        serde_json::from_str(&document.to_json().expect("serialize Credit Document"))
            .expect("parse Credit Document JSON");
    let card_ranges = changed["KCCI_01"][0]["card_ranges"]
        .as_array_mut()
        .expect("first card company ranges");
    let original_range_count = card_ranges.len();
    card_ranges.push(card_ranges[0].clone());
    let changed_document =
        Document::parse_json(&changed.to_string()).expect("parse modified Credit Document");
    let encoded = runtime
        .encode_http(ProtocolDirection::Downstream, credit, &changed_document)
        .await
        .expect("encode modified Credit response through Host");
    let encoded_wire: Value = serde_json::from_str(&encoded).expect("parse modified wire JSON");
    assert_eq!(encoded_wire["KCCI_01"]["Length"], "2510");

    let decoded_again = runtime
        .decode_http(ProtocolDirection::Downstream, &encoded)
        .await
        .expect("decode modified Credit response through Host");
    let decoded_again: Value = serde_json::from_str(
        &decoded_again
            .to_json()
            .expect("serialize modified Document"),
    )
    .expect("parse modified Document JSON");
    assert_eq!(
        decoded_again["KCCI_01"][0]["card_ranges"]
            .as_array()
            .expect("modified card ranges")
            .len(),
        original_range_count + 1
    );
}
