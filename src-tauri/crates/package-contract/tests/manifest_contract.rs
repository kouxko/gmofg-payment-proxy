use intercept_proxy_package_contract::{PackageKind, PackageManifest};
use serde_json::{Value, json};

const HTTP: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);
const SOCKET: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/socket-manifest.json"
);
const VALIDATION: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/validation-corpus.json"
);

#[test]
fn http_manifest_accepts_empty_or_schema_directions_and_round_trips_exact_shape() {
    let manifest: PackageManifest = serde_json::from_str(HTTP).expect("valid HTTP manifest");
    assert_eq!(manifest.kind(), PackageKind::Http);
    assert_eq!(
        serde_json::to_value(manifest).expect("serialize"),
        serde_json::from_str::<Value>(HTTP).expect("fixture")
    );
}

#[test]
fn socket_manifest_requires_both_schemas() {
    let manifest: PackageManifest = serde_json::from_str(SOCKET).expect("valid Socket manifest");
    assert_eq!(manifest.kind(), PackageKind::Socket);
    let mut missing: Value = serde_json::from_str(SOCKET).expect("fixture");
    missing["document"]["upstream"] = json!({});
    assert!(serde_json::from_value::<PackageManifest>(missing).is_err());
}

#[test]
fn manifest_rejects_unknown_fields_wrong_api_and_invalid_schema_definition() {
    let mut unknown: Value = serde_json::from_str(HTTP).expect("fixture");
    unknown["hooks"] = json!({});
    assert!(serde_json::from_value::<PackageManifest>(unknown).is_err());
    let mut api: Value = serde_json::from_str(HTTP).expect("fixture");
    api["api"] = json!(2);
    assert!(serde_json::from_value::<PackageManifest>(api).is_err());
    let mut schema: Value = serde_json::from_str(HTTP).expect("fixture");
    schema["document"]["upstream"] = json!({"schema":{"type":"string","title":" "}});
    assert!(serde_json::from_value::<PackageManifest>(schema).is_err());
}

#[test]
fn manifest_package_id_matches_shared_domain_validation_corpus() {
    let fixture: Value = serde_json::from_str(HTTP).expect("fixture");
    let corpus: Value = serde_json::from_str(VALIDATION).expect("validation corpus");
    for test_case in corpus["id"].as_array().expect("id cases") {
        let mut manifest = fixture.clone();
        manifest["package"]["id"] = test_case["value"].clone();
        let accepted = serde_json::from_value::<PackageManifest>(manifest).is_ok();
        assert_eq!(accepted, test_case["valid"], "{test_case}");
    }
}
