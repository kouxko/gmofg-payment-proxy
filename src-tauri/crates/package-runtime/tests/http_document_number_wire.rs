use std::path::{Path, PathBuf};

use intercept_proxy_domain::{DocumentValue, JsonPointer, ProtocolDirection};
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};

const ORIGINAL: &str = " { \"integer\": 2.0, \"fraction\": 2.5, \"text\": \"2.0\", \"values\": [0.0, 1, 2.5], \"result\": \"old\" }\n";
const CHANGED: &str = r#"{
  "fraction": 2.5,
  "integer": 2,
  "result": "changed",
  "text": "2.0",
  "values": [
    0,
    1,
    2.5
  ]
}"#;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

async fn load_runtime(relative_path: &Path) -> WasmPackageRuntime {
    let bytes = std::fs::read(repository_root().join(relative_path)).unwrap();
    let package = read_package_component(&bytes).unwrap();
    WasmPackageRuntime::load(&package).await.unwrap()
}

#[tokio::test]
async fn actual_json_pretty_and_cps_hosts_emit_integral_numbers_without_decimal_suffixes() {
    for package in [
        Path::new("examples/protocol-packages/json_pretty/dist/json-pretty-1.0.0.wasm"),
        Path::new("examples/protocol-packages/gmofg_cps/dist/gmofg-cps-1.0.0.wasm"),
    ] {
        let mut runtime = load_runtime(package).await;
        for direction in [ProtocolDirection::Upstream, ProtocolDirection::Downstream] {
            let unchanged = runtime.decode_http(direction, ORIGINAL).await.unwrap();
            assert_eq!(
                runtime
                    .encode_http(direction, ORIGINAL, &unchanged)
                    .await
                    .unwrap(),
                ORIGINAL,
                "unchanged input must retain its original bytes for {package:?} {direction:?}"
            );

            let mut same_value = unchanged.clone();
            same_value
                .set(
                    &JsonPointer::property("integer"),
                    DocumentValue::integer(2).unwrap(),
                )
                .unwrap();
            assert_eq!(
                runtime
                    .encode_http(direction, ORIGINAL, &same_value)
                    .await
                    .unwrap(),
                ORIGINAL,
                "same-value Set must retain original bytes for {package:?} {direction:?}"
            );

            let mut changed = unchanged;
            changed
                .set(
                    &JsonPointer::property("result"),
                    DocumentValue::String("changed".into()),
                )
                .unwrap();
            let encoded = runtime
                .encode_http(direction, ORIGINAL, &changed)
                .await
                .unwrap();
            println!(
                "{}",
                serde_json::json!({
                    "package": package,
                    "direction": format!("{direction:?}"),
                    "original": ORIGINAL,
                    "document_json": changed.to_json().unwrap(),
                    "encoded": encoded,
                    "expected": CHANGED,
                })
            );
            assert_eq!(
                encoded, CHANGED,
                "changed body must use canonical integer JSON for {package:?} {direction:?}"
            );
        }
    }
}
