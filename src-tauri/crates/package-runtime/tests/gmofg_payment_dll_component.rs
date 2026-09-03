use std::path::{Path, PathBuf};

use intercept_proxy_domain::{Document, ProtocolDirection};
use intercept_proxy_package_contract::PackageKind;
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};
use serde_json::Value;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[tokio::test]
async fn gmofg_payment_dll_component_runs_downstream_decode_encode_and_json_tree_display() {
    let package_root = repository_root().join("examples/protocol-packages/gmofg_payment_dll");
    let mut runtime = load_runtime(&package_root).await;

    assert_d48_round_trip(&mut runtime, &package_root).await;
    assert_credit_round_trip_display_and_modification(&mut runtime, &package_root).await;
}

async fn load_runtime(package_root: &Path) -> WasmPackageRuntime {
    let bytes = std::fs::read(package_root.join("dist/gmofg-payment-dll-1.0.0.wasm"))
        .expect("read built GMO-FG Payment DLL Component");
    let package = read_package_component(&bytes).expect("validate embedded package manifest");
    assert_eq!(
        package.manifest().package().identity().id.as_str(),
        "gmofg-payment-dll"
    );
    assert_eq!(package.manifest().kind(), PackageKind::Http);
    WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate GMO-FG Payment DLL HTTP Component")
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
