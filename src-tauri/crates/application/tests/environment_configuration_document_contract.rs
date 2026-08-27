use intercept_proxy_application::{
    EnvironmentTerminalResult, parse_environment_configuration_candidate_v1,
};
use intercept_proxy_domain::{DocumentAction, DocumentCondition, DocumentValue};
use serde_json::{Value, json};

fn round_trip<T>(expected: &Value)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let typed: T = serde_json::from_value(expected.clone()).expect("canonical wire deserializes");
    assert_eq!(serde_json::to_value(typed).unwrap(), *expected);
}

#[test]
fn document_value_four_variant_wire_round_trips_without_drift() {
    for (typed, expected) in [
        (
            DocumentValue::String("abc".to_owned()),
            json!({"type":"string","value":"abc"}),
        ),
        (DocumentValue::Int(7), json!({"type":"int","value":7})),
        (
            DocumentValue::Bool(true),
            json!({"type":"bool","value":true}),
        ),
        (
            DocumentValue::Blob(vec![0, 255]),
            json!({"type":"blob","value":[0,255]}),
        ),
    ] {
        assert_eq!(serde_json::to_value(&typed).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<DocumentValue>(expected).unwrap(),
            typed
        );
    }
}

#[test]
fn document_condition_and_action_preserve_adjacent_value_wire() {
    round_trip::<DocumentCondition>(&json!({
        "operator":"equals", "field":"amount", "value":{"type":"int","value":7}
    }));
    round_trip::<DocumentAction>(&json!({
        "type":"set_field", "field":"approval_code",
        "value":{"type":"string","value":"abc"}
    }));
}

#[test]
fn document_contract_rejects_variant_and_tag_drift() {
    for invalid in [
        json!({"operator":"Equals","field":"amount","value":{"type":"int","value":7}}),
        json!({"operator":"equals","field":"amount","value":{"type":"integer","value":7}}),
        json!({"type":"SetField","field":"amount","value":{"type":"int","value":7}}),
        json!({"type":"set_field","field":"amount","value":{"Int":7}}),
    ] {
        let accepted = serde_json::from_value::<DocumentCondition>(invalid.clone()).is_ok()
            || serde_json::from_value::<DocumentAction>(invalid).is_ok();
        assert!(!accepted);
    }
}

#[test]
fn expected_preview_contains_all_document_variants_and_terminal_field_contract() {
    let preview: Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/expected-preview.json"
    )))
    .unwrap();
    let actual_values = preview["protocol_document_values"].as_array().unwrap();
    let expected_values = [
        json!({"type":"string","value":"abc"}),
        json!({"type":"int","value":7}),
        json!({"type":"bool","value":true}),
        json!({"type":"blob","value":[0,255]}),
    ];
    assert_eq!(actual_values, &expected_values);
    assert_eq!(
        preview["terminal_action_fields"]["TruncateResponse"],
        json!(["bytes"])
    );
    assert_eq!(
        preview["terminal_action_fields"]["DisconnectDuringUpstreamWrite"],
        json!(["after_bytes"])
    );
}

#[test]
fn terminal_action_rejects_wrong_offset_fields_and_unknown_payload_fields() {
    let fixture: Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
    )))
    .unwrap();
    for (variant, invalid) in [
        (
            "TruncateResponse",
            json!({"TruncateResponse":{"after_bytes":1}}),
        ),
        (
            "TruncateResponse",
            json!({"TruncateResponse":{"bytes":1,"unexpected":true}}),
        ),
        (
            "DisconnectDuringUpstreamWrite",
            json!({"DisconnectDuringUpstreamWrite":{"bytes":1}}),
        ),
        (
            "DisconnectDuringDownstreamWrite",
            json!({"DisconnectDuringDownstreamWrite":{"after_bytes":1,"unexpected":true}}),
        ),
    ] {
        let mut candidate = fixture.clone();
        let action = candidate["workspace"]["http_rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .flat_map(|rule| rule["actions"].as_array_mut().unwrap())
            .find(|action| action["Terminal"].get(variant).is_some())
            .expect("canonical fixture contains the requested terminal variant");
        action["Terminal"] = invalid;
        assert!(
            parse_environment_configuration_candidate_v1(&serde_json::to_vec(&candidate).unwrap())
                .is_err()
        );
    }
}

#[test]
fn terminal_result_union_rejects_status_literal_and_shape_drift() {
    for invalid in [
        json!({"result":"Committed","workspace_id":"00000000-0000-0000-0000-000000000001","revision":1,"selected_workspace_id":null,"apply_task_id":null,"status_code":null,"diagnostics":[]}),
        json!({"result":"cancelled","status_code":"candidate_cancelled","diagnostics":[]}),
        json!({"result":"stale","status":"CANDIDATE_STALE","diagnostics":[]}),
        json!({"result":"rolled_back","status_code":"COMMIT_ROLLED_BACK","workspace_id":"00000000-0000-0000-0000-000000000001","diagnostics":[]}),
    ] {
        assert!(serde_json::from_value::<EnvironmentTerminalResult>(invalid).is_err());
    }
}

#[test]
fn candidate_schema_version_is_exact_literal_one() {
    let fixture: Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
    )))
    .unwrap();
    for invalid in [json!(0), json!(2), json!("1"), Value::Null] {
        let mut candidate = fixture.clone();
        candidate["schema_version"] = invalid;
        assert!(
            parse_environment_configuration_candidate_v1(&serde_json::to_vec(&candidate).unwrap())
                .is_err()
        );
    }
}
