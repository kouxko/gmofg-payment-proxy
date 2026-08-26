use intercept_proxy_application::parse_environment_configuration_candidate_v1;
use serde::Deserialize;
use serde_json::Value;

const FIXTURE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/"
);

fn fixture(name: &str) -> Value {
    let bytes = match name {
        "full-shape.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
        ))
        .as_slice(),
        "weak-network-null.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/weak-network-null.json"
        ))
        .as_slice(),
        "existing-target-retained-selector.json" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/existing-target-retained-selector.json"
        ))
        .as_slice(),
        _ => panic!("unknown fixture below {FIXTURE_ROOT}: {name}"),
    };
    serde_json::from_slice(bytes).expect("contract fixture is valid JSON")
}

fn parses(value: &Value) -> bool {
    parse_environment_configuration_candidate_v1(&serde_json::to_vec(value).unwrap()).is_ok()
}

#[derive(Debug, Deserialize)]
struct NegativeCase {
    name: String,
    pointer: String,
    operation: String,
    value: Value,
}

fn negative_cases() -> Vec<NegativeCase> {
    serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/negative-cases.json"
    )))
    .expect("negative case manifest is valid JSON")
}

fn apply_case(candidate: &mut Value, case: &NegativeCase) {
    match case.operation.as_str() {
        "replace" => *candidate.pointer_mut(&case.pointer).expect(&case.name) = case.value.clone(),
        "add" => {
            let (parent, field) = case
                .pointer
                .rsplit_once('/')
                .expect("object member pointer");
            candidate
                .pointer_mut(parent)
                .expect(&case.name)
                .as_object_mut()
                .expect("negative add targets an object")
                .insert(field.to_owned(), case.value.clone());
        }
        operation => panic!("unsupported fixture operation: {operation}"),
    }
}

#[test]
fn accepts_explicit_null_weak_network_fixture() {
    let mut candidate = fixture("full-shape.json");
    candidate["workspace"]["android_network_profiles"][0]["weak_network"] =
        fixture("weak-network-null.json");

    assert!(parses(&candidate));
}

#[test]
fn accepts_independent_existing_target_retained_selector_fixture() {
    assert!(parses(&fixture("existing-target-retained-selector.json")));
}

#[test]
fn accepts_new_target_with_http_relay_local_responder_and_materials() {
    let candidate = fixture("full-shape.json");

    assert!(parses(&candidate));
    let topologies = candidate["workspace"]["listeners"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|listener| listener.pointer("/data_plane/settings/topology/mode"))
        .collect::<Vec<_>>();
    assert!(topologies.contains(&&Value::String("relay".to_owned())));
    assert!(topologies.contains(&&Value::String("local_responder".to_owned())));
    assert!(
        !candidate["materials"]["certificates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !candidate["materials"]["secrets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejects_every_named_negative_contract_fixture() {
    for case in negative_cases() {
        let mut candidate = fixture("full-shape.json");
        apply_case(&mut candidate, &case);
        assert!(!parses(&candidate), "negative case accepted: {}", case.name);
    }
}

#[test]
fn rejects_omitted_weak_network_and_explicit_null_root() {
    let mut omitted = fixture("full-shape.json");
    omitted["workspace"]["android_network_profiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("weak_network");
    let mut null = fixture("full-shape.json");
    null["workspace"]["android_network_profiles"][0]["weak_network"] = Value::Null;

    assert!(!parses(&omitted));
    assert!(!parses(&null));
}
