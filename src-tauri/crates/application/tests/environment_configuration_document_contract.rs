use intercept_proxy_application::{
    EnvironmentTerminalResult, parse_environment_configuration_candidate_v1,
};
use intercept_proxy_domain::{Condition, DocumentNumber, DocumentValue, UnifiedAction};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn round_trip<T>(expected: &Value)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let typed: T = serde_json::from_value(expected.clone()).expect("canonical wire deserializes");
    assert_eq!(serde_json::to_value(typed).unwrap(), *expected);
}

#[test]
fn document_value_recursive_json_wire_round_trips_without_drift() {
    for (typed, expected) in [
        (DocumentValue::String("abc".to_owned()), json!("abc")),
        (
            DocumentValue::Number(DocumentNumber::new(7.5).unwrap()),
            json!(7.5),
        ),
        (DocumentValue::Boolean(true), json!(true)),
        (DocumentValue::null(), Value::Null),
        (
            DocumentValue::Object(BTreeMap::from([(
                "nested".to_owned(),
                DocumentValue::String("value".to_owned()),
            )])),
            json!({"nested":"value"}),
        ),
        (
            DocumentValue::Array(vec![DocumentValue::Number(
                DocumentNumber::new(0.0).unwrap(),
            )]),
            json!([0.0]),
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
fn document_condition_and_action_use_authoritative_typed_wire() {
    round_trip::<Condition>(&json!({
        "source": "document",
        "path": "/amount",
        "predicate": {
            "type": "number",
            "value": { "operator": "equal", "value": 7.0 }
        }
    }));
    round_trip::<UnifiedAction>(&json!({
        "source": "document",
        "value": { "type": "set", "path": "/approval_code", "value": "abc" }
    }));
}

#[test]
fn document_contract_rejects_variant_and_tag_drift() {
    for invalid in [
        json!({"source":"document","path":"amount","predicate":{"type":"number","value":{"operator":"equal","value":7}}}),
        json!({"source":"document","path":"/amount","predicate":{"type":"number","value":{"operator":"Equals","value":7}}}),
        json!({"source":"document","value":{"type":"Set","path":"/amount","value":7}}),
        json!({"source":"document","value":{"type":"set","path":"amount","value":7}}),
    ] {
        let accepted = serde_json::from_value::<Condition>(invalid.clone()).is_ok()
            || serde_json::from_value::<UnifiedAction>(invalid).is_ok();
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
        json!("abc"),
        json!(7.5),
        json!(true),
        Value::Null,
        json!({"nested":"value"}),
        json!([0.0]),
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
fn terminal_action_rejects_wrong_offset_fields() {
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
            "DisconnectDuringUpstreamWrite",
            json!({"DisconnectDuringUpstreamWrite":{"bytes":1}}),
        ),
    ] {
        let mut candidate = fixture.clone();
        let action = candidate["workspace"]["rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|rule| rule["content"]["type"] == "http")
            .flat_map(|rule| rule["content"]["value"]["actions"].as_array_mut().unwrap())
            .find(|action| action["source"] == "terminal" && action["value"].get(variant).is_some())
            .expect("canonical fixture contains the requested terminal variant");
        action["value"] = invalid;
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
