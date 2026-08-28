use intercept_proxy_application::{
    EnvironmentConfigurationCandidateV1, EnvironmentTerminalResult,
    parse_environment_configuration_candidate_v1,
};
use intercept_proxy_domain::TerminalAction;
use serde_json::{Value, json};

const FULL_SHAPE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
));
const EXISTING_TARGET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/existing-target-retained-selector.json"
));

fn full_shape_value() -> Value {
    serde_json::from_slice(FULL_SHAPE).expect("canonical fixture is valid JSON")
}

fn parse(value: &Value) -> Result<EnvironmentConfigurationCandidateV1, String> {
    parse_environment_configuration_candidate_v1(&serde_json::to_vec(value).unwrap())
        .map_err(|error| error.to_string())
}

fn workspace_rules(value: &Value) -> &[Value] {
    value["workspace"]["rules"].as_array().unwrap()
}

fn workspace_rules_mut(value: &mut Value) -> &mut Vec<Value> {
    value["workspace"]["rules"].as_array_mut().unwrap()
}

fn first_rule_mut<'a>(value: &'a mut Value, rule_type: &str) -> &'a mut Value {
    workspace_rules_mut(value)
        .iter_mut()
        .find(|rule| rule["type"] == rule_type)
        .unwrap()
}

#[test]
fn canonical_full_shape_candidate_round_trips_without_wire_drift() {
    let expected = full_shape_value();
    let candidate = parse(&expected).expect("canonical v1 candidate parses");

    assert_eq!(serde_json::to_value(candidate).unwrap(), expected);
}

#[test]
fn candidate_rejects_unknown_fields_at_nested_contract_boundaries() {
    for pointer in [
        "",
        "/workspace",
        "/workspace/listeners/0",
        "/workspace/listeners/0/data_plane/settings",
        "/workspace/android_network_profiles/0/weak_network",
        "/workspace/android_network_profiles/0/weak_network/path_mtu",
        "/materials",
        "/materials/secrets/0",
    ] {
        let mut value = full_shape_value();
        value
            .pointer_mut(pointer)
            .expect("fixture contract object")
            .as_object_mut()
            .expect("fixture contract object")
            .insert("unexpected".to_owned(), json!(true));

        assert!(
            parse(&value).is_err(),
            "unknown field accepted at {pointer}"
        );
    }
}

#[test]
fn weak_network_requires_every_field_even_when_optional_values_are_null() {
    for field in [
        "seed",
        "fixed_delay_millis",
        "uniform_jitter_millis",
        "upload_bytes_per_second",
        "download_bytes_per_second",
        "random_loss_basis_points",
        "burst_loss",
        "duplicate_basis_points",
        "reorder_basis_points",
        "maximum_reorder_hold_millis",
        "blackout_windows",
        "dns_blackhole",
        "nth_tcp_flag_drops",
        "path_mtu",
        "corruption",
    ] {
        let mut value = full_shape_value();
        value
            .pointer_mut("/workspace/android_network_profiles/0/weak_network")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);

        assert!(
            parse(&value).is_err(),
            "omitted weak-network field {field} accepted"
        );
    }
}

#[test]
fn weak_network_requires_every_nested_object_field() {
    for (pointer, field) in [
        (
            "/workspace/android_network_profiles/0/weak_network/burst_loss",
            "enter_bad_state_basis_points",
        ),
        (
            "/workspace/android_network_profiles/0/weak_network/blackout_windows/0",
            "duration_millis",
        ),
        (
            "/workspace/android_network_profiles/0/weak_network/nth_tcp_flag_drops/0",
            "nth",
        ),
        (
            "/workspace/android_network_profiles/0/weak_network/path_mtu",
            "mtu",
        ),
        (
            "/workspace/android_network_profiles/0/weak_network/corruption",
            "bits_per_packet",
        ),
    ] {
        let mut value = full_shape_value();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);

        assert!(
            parse(&value).is_err(),
            "omitted nested weak-network field {pointer}/{field} accepted"
        );
    }
}

#[test]
fn weak_network_rejects_scalar_shorthand_and_alternate_enum_tags() {
    for (pointer, invalid) in [
        (
            "/workspace/android_network_profiles/0/weak_network",
            json!(25),
        ),
        (
            "/workspace/android_network_profiles/0/weak_network/nth_tcp_flag_drops/0/direction",
            json!("Upstream"),
        ),
        (
            "/workspace/android_network_profiles/0/weak_network/nth_tcp_flag_drops/0/flag",
            json!("Syn"),
        ),
        (
            "/workspace/android_network_profiles/0/weak_network/path_mtu/mode",
            json!({"Blackhole": {}}),
        ),
    ] {
        let mut value = full_shape_value();
        *value.pointer_mut(pointer).unwrap() = invalid;

        assert!(
            parse(&value).is_err(),
            "alternate weak-network wire accepted at {pointer}"
        );
    }
}

#[test]
fn protocol_document_values_require_the_adjacent_type_value_wire() {
    for invalid in [
        json!("1000"),
        json!(1000),
        json!(true),
        json!([65, 66]),
        json!({"String": "1000"}),
        json!({"type": "unknown", "value": "1000"}),
    ] {
        let mut value = full_shape_value();
        first_rule_mut(&mut value, "socket")["conditions"][0]["value"] = invalid;

        assert!(
            parse(&value).is_err(),
            "noncanonical Document value accepted"
        );
    }
}

#[test]
fn existing_rule_id_is_forbidden_for_new_workspace_targets() {
    let mut value = full_shape_value();
    first_rule_mut(&mut value, "http")["existing_rule_id"] =
        json!("00000000-0000-0000-0000-000000000020");

    assert!(
        parse(&value).is_err(),
        "new target accepted retained rule selectors"
    );

    first_rule_mut(&mut value, "http")
        .as_object_mut()
        .unwrap()
        .remove("existing_rule_id");
    parse(&value).expect("omitted selector represents a new rule");

    first_rule_mut(&mut value, "http")["existing_rule_id"] = Value::Null;
    parse(&value).expect("explicit null selector also represents a new rule");
}

#[test]
fn existing_rule_id_may_not_be_reused_twice_in_one_candidate() {
    for rule_type in ["http", "socket"] {
        let mut value: Value = serde_json::from_slice(EXISTING_TARGET).unwrap();
        let duplicate = workspace_rules(&value)
            .iter()
            .find(|rule| rule["type"] == rule_type)
            .unwrap()
            .clone();
        workspace_rules_mut(&mut value).push(duplicate);

        assert!(
            parse(&value).is_err(),
            "duplicate selector accepted for {rule_type} rule"
        );
    }
}

#[test]
fn created_order_is_never_accepted_from_candidate_rules() {
    for rule_type in ["http", "socket"] {
        let mut value = full_shape_value();
        first_rule_mut(&mut value, rule_type)
            .as_object_mut()
            .unwrap()
            .insert("created_order".to_owned(), json!(42));

        assert!(
            parse(&value).is_err(),
            "submitted created_order accepted for {rule_type} rule"
        );
    }
}

#[test]
fn current_domain_terminal_byte_offsets_round_trip_with_exact_field_names() {
    for (action, expected) in [
        (
            TerminalAction::TruncateResponse { bytes: 1 },
            json!({"TruncateResponse": {"bytes": 1}}),
        ),
        (
            TerminalAction::DisconnectDuringUpstreamWrite { after_bytes: 2 },
            json!({"DisconnectDuringUpstreamWrite": {"after_bytes": 2}}),
        ),
        (
            TerminalAction::DisconnectDuringDownstreamWrite { after_bytes: 3 },
            json!({"DisconnectDuringDownstreamWrite": {"after_bytes": 3}}),
        ),
    ] {
        assert_eq!(serde_json::to_value(&action).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<TerminalAction>(expected).unwrap(),
            action
        );
    }
}

#[test]
fn canonical_fixture_contains_every_terminal_action_variant_once() {
    let fixture = full_shape_value();
    let actions = workspace_rules(&fixture)
        .iter()
        .filter(|rule| rule["type"] == "http")
        .flat_map(|rule| rule["actions"].as_array().unwrap())
        .filter_map(|action| action.get("Terminal"))
        .map(|terminal| match terminal {
            Value::String(name) => name.as_str(),
            Value::Object(object) => object.keys().next().unwrap().as_str(),
            _ => panic!("terminal action must use the canonical external tag"),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        actions.len(),
        12,
        "each terminal variant appears exactly once"
    );
    let actions = actions
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        actions,
        [
            "DisconnectBeforeUpstream",
            "DisconnectDuringDownstreamWrite",
            "DisconnectDuringUpstreamWrite",
            "DropUpstreamResponse",
            "IncorrectContentLength",
            "InvalidJson",
            "MockResponse",
            "RejectTlsHandshake",
            "TruncateResponse",
            "UpstreamConnectTimeout",
            "UpstreamReadTimeout",
            "UpstreamWriteTimeout",
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn terminal_results_are_explicit_tagged_variants() {
    let variants = [
        json!({"result":"committed","workspace_id":"00000000-0000-0000-0000-000000000001","revision":8,"selected_workspace_id":"00000000-0000-0000-0000-000000000001","apply_task_id":"apply-1","status_code":null,"diagnostics":[]}),
        json!({"result":"validation_failed","status_code":"SCHEMA_INVALID","diagnostics":[]}),
        json!({"result":"stale","status_code":"CANDIDATE_STALE","diagnostics":[]}),
        json!({"result":"cancelled","status_code":"CANDIDATE_CANCELLED","diagnostics":[]}),
        json!({"result":"cancelled_by_shutdown","status_code":"CANDIDATE_CANCELLED_BY_SHUTDOWN","diagnostics":[]}),
        json!({"result":"failed_before_commit","status_code":"PROTECTED_MATERIAL_PREPARE_FAILED","diagnostics":[]}),
        json!({"result":"rolled_back","status_code":"COMMIT_ROLLED_BACK","diagnostics":[]}),
    ];

    for expected in variants {
        let result: EnvironmentTerminalResult = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), expected);
    }
}

#[test]
fn terminal_results_reject_null_failure_codes_and_non_null_committed_codes() {
    for invalid in [
        json!({"result":"stale","status_code":null,"diagnostics":[]}),
        json!({"result":"committed","workspace_id":"00000000-0000-0000-0000-000000000001","revision":8,"selected_workspace_id":null,"apply_task_id":null,"status_code":"COMMIT_FAILED","diagnostics":[]}),
        json!({"result":"stale","status_code":"UNREGISTERED_CODE","diagnostics":[]}),
    ] {
        assert!(serde_json::from_value::<EnvironmentTerminalResult>(invalid).is_err());
    }
}

#[test]
fn non_committed_terminal_results_reject_persisted_workspace_identifiers() {
    let invalid = json!({
        "result": "rolled_back",
        "workspace_id": "00000000-0000-0000-0000-000000000001",
        "revision": 8,
        "status_code": "COMMIT_ROLLED_BACK",
        "diagnostics": []
    });

    assert!(serde_json::from_value::<EnvironmentTerminalResult>(invalid).is_err());
}
