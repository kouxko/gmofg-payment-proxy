use std::path::PathBuf;

use intercept_proxy_domain::{Document, ProtocolDirection};
use intercept_proxy_package_contract::PackageKind;
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};
use serde_json::{Value, json};

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples/protocol-packages/gmofg_cps")
}

fn envelope(case: &Value, body: &str) -> Value {
    let operation = case["operation"].as_str().unwrap();
    let version = if matches!(operation, "pay" | "confirm") {
        "version=1.0/"
    } else {
        ""
    };
    let token = case["token"].as_str().unwrap().replace(' ', "%20");
    json!({
        "url":format!("https://cps.example/gateway/{operation}/{version}B01/1/0/{token}"),
        "method":"POST", "code":200,
        "header":{"Content-Type":"application/json;charset=UTF-8","X-Keep":"yes"},
        "body":body
    })
}

#[tokio::test]
async fn actual_cps_component_matches_java_and_signs_encoded_rule_changes() {
    let bytes = std::fs::read(package_root().join("dist/gmofg-cps-1.0.0.wasm")).unwrap();
    let package = read_package_component(&bytes).unwrap();
    assert_eq!(package.manifest().kind(), PackageKind::Http);
    assert_eq!(
        package.manifest().package().identity().id.as_str(),
        "gmofg-cps"
    );
    let mut runtime = WasmPackageRuntime::load(&package).await.unwrap();
    assert!(!runtime.has_http_finalize(ProtocolDirection::Upstream));
    assert!(runtime.has_http_finalize(ProtocolDirection::Downstream));
    let cases: Vec<Value> = serde_json::from_slice(
        &std::fs::read(package_root().join("tests/fixtures/java-signatures.json")).unwrap(),
    )
    .unwrap();
    for case in &cases {
        let input = envelope(case, case["body"].as_str().unwrap());
        let input_text = format!(" \n{input}\n");
        let output = runtime
            .finalize_http(ProtocolDirection::Downstream, &input_text)
            .await
            .unwrap()
            .unwrap();
        if case["expect_error"] == true {
            assert_eq!(output, input_text, "{}", case["id"]);
        } else {
            let result: Value = serde_json::from_str(&output).unwrap();
            for field in ["url", "method", "code", "header"] {
                assert_eq!(result[field], input[field]);
            }
            let mut body: Value = serde_json::from_str(result["body"].as_str().unwrap()).unwrap();
            assert_eq!(body["data"]["sign"], case["sign"], "{}", case["id"]);
            let original: Value = serde_json::from_str(case["body"].as_str().unwrap()).unwrap();
            body["data"]["sign"] = original["data"]["sign"].clone();
            assert_eq!(body, original, "only sign changes: {}", case["id"]);
        }
        println!(
            "CPS_HOST_EVIDENCE {}",
            json!({"id":case["id"],"input":input_text,"output":output})
        );
    }
    let before = cases
        .iter()
        .find(|case| case["id"] == "pay-before")
        .unwrap();
    let after = cases.iter().find(|case| case["id"] == "pay-after").unwrap();
    let original = before["body"].as_str().unwrap();
    for direction in [ProtocolDirection::Upstream, ProtocolDirection::Downstream] {
        let document = runtime.decode_http(direction, original).await.unwrap();
        let display = runtime.display(direction, &document).await.unwrap();
        assert!(display.contains("amount") && display.contains("sign"));
        assert_eq!(
            runtime
                .encode_http(direction, original, &document)
                .await
                .unwrap(),
            original
        );
    }
    let decoded = runtime
        .decode_http(ProtocolDirection::Downstream, original)
        .await
        .unwrap();
    let mut changed: Value = serde_json::from_str(&decoded.to_json().unwrap()).unwrap();
    changed["data"]["result"]["amount"] = json!(250);
    let document = Document::parse_json(&changed.to_string()).unwrap();
    let encoded = runtime
        .encode_http(ProtocolDirection::Downstream, original, &document)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&encoded).unwrap()["data"]["sign"],
        "OLD"
    );
    let input = envelope(before, &encoded);
    let output = runtime
        .finalize_http(ProtocolDirection::Downstream, &input.to_string())
        .await
        .unwrap()
        .unwrap();
    let result: Value = serde_json::from_str(&output).unwrap();
    let result_body: Value = serde_json::from_str(result["body"].as_str().unwrap()).unwrap();
    assert_eq!(
        result_body["data"]["result"]["amount"].as_f64(),
        Some(250.0)
    );
    assert_eq!(result_body["data"]["sign"], after["sign"]);
    println!(
        "CPS_HOST_EDIT_EVIDENCE {}",
        json!({"original":original,"document":changed,"encoded":encoded,"finalized":result})
    );
}
